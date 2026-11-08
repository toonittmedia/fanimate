//! Drawing a frame that uses compositing: pictures drawn on their own and laid
//! over the rest (see `fanimate_core::compositing`).
//!
//! A frame with no more than the root scope is drawn in one pass, as it always
//! was. With more, each scope is drawn into a texture of its own, the way the
//! root is drawn into the target (4x MSAA, then resolved), and laid over the
//! scope it was opened in with a pass that draws the texture: faded, mixed as
//! its blend mode says, and held to a mask when it has one. The mask is itself
//! a scope, whose coverage (its alpha) is what lets the others through.
//!
//! Textures for scopes are kept between frames and handed out as scopes are
//! drawn and laid down, so a frame needs as many as are open at once, not as
//! many as there are scopes.

use fanimate_core::{Blend, Color, Ink, Media, Scope, ScopeId, ScopeKind};

use super::{RenderTarget, Renderer};
use crate::tess::{Piece, Run};

/// The scopes of a frame and the stretches of the mesh that belong to each.
#[derive(Clone, Copy)]
pub struct Composition<'a> {
    pub scopes: &'a [Scope],
    pub pieces: &'a [Piece],
}

impl Composition<'static> {
    /// A frame with no compositing: everything is in the root scope.
    pub const NONE: Composition<'static> = Composition {
        scopes: &[],
        pieces: &[],
    };
}

/// Most pictures laid over others in one frame; further ones are left out
/// rather than drawn wrong.
const MAX_COMPOSITES: usize = 512;
/// Bytes between one picture's settings and the next in the settings buffer
/// (the smallest step a uniform buffer allows).
const PARAMS_STRIDE: u64 = 256;

const SHADER: &str = r#"
struct Params {
    opacity: f32,
    use_mask: f32,
    pad0: f32,
    pad1: f32,
};
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var msk: texture_2d<f32>;
@group(0) @binding(3) var<uniform> p: Params;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// one triangle that covers the whole target
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: VOut;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // premultiplied: fading and masking scale every channel alike
    let c = textureSampleLevel(src, samp, i.uv, 0.0);
    var m = 1.0;
    if (p.use_mask > 0.5) {
        m = textureSampleLevel(msk, samp, i.uv, 0.0).a;
    }
    return c * (p.opacity * m);
}
"#;

/// A texture a scope is drawn into: multisampled when the renderer is, with the
/// resolved picture, which is what gets laid down.
struct Slot {
    msaa: Option<wgpu::TextureView>,
    view: wgpu::TextureView,
    _textures: Vec<wgpu::Texture>,
}

/// What compositing needs beyond the plain renderer, made the first time a
/// frame uses it.
pub(super) struct Compositor {
    atop: wgpu::RenderPipeline,
    erase: wgpu::RenderPipeline,
    /// Lay a picture over another, one pipeline for each blend mode (in the
    /// order of `Blend::ALL`).
    composite: Vec<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    slots: Vec<Slot>,
    free: Vec<usize>,
    size: (u32, u32),
}

/// Where a scope is drawn: the frame's target, or one of the compositor's textures.
#[derive(Clone, Copy)]
enum Dest {
    Root,
    Slot(usize),
}

/// One thing a scope does, in order: draw a piece of the mesh, or lay down a
/// scope that was opened inside it.
#[derive(Clone, Copy)]
enum Event {
    Draw(usize),
    Child(ScopeId),
}

/// Each scope's events: its own pieces and the scopes opened in it, in the
/// order they come in the mesh. A scope that has no pieces, and none of its
/// children has any, is never drawn.
fn plan(scopes: &[Scope], pieces: &[Piece]) -> Vec<Vec<Event>> {
    let mut events = vec![Vec::new(); scopes.len()];
    let mut entered = vec![false; scopes.len()];
    entered[0] = true;
    // the parent is always an earlier scope; one that is not is taken as the root
    let parent = |s: usize| (scopes[s].parent as usize).min(s.saturating_sub(1));
    fn enter(s: usize, events: &mut [Vec<Event>], entered: &mut [bool], parent: &dyn Fn(usize) -> usize) {
        if entered[s] {
            return;
        }
        let p = parent(s);
        enter(p, events, entered, parent);
        entered[s] = true;
        events[p].push(Event::Child(s as ScopeId));
    }
    for (i, piece) in pieces.iter().enumerate() {
        if piece.first >= piece.end {
            continue;
        }
        let s = (piece.scope as usize).min(scopes.len() - 1);
        enter(s, &mut events, &mut entered, &parent);
        events[s].push(Event::Draw(i));
    }
    events
}

fn blend_state(blend: Blend) -> wgpu::BlendState {
    use wgpu::BlendFactor as F;
    let component = |src, dst| wgpu::BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation: wgpu::BlendOperation::Add,
    };
    let over_alpha = component(F::One, F::OneMinusSrcAlpha);
    match blend {
        Blend::Normal => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        // premultiplied: src * dst + dst * (1 - src alpha)
        Blend::Multiply => wgpu::BlendState {
            color: component(F::Dst, F::OneMinusSrcAlpha),
            alpha: over_alpha,
        },
        // src + dst * (1 - src)
        Blend::Screen => wgpu::BlendState {
            color: component(F::One, F::OneMinusSrc),
            alpha: over_alpha,
        },
        Blend::Add => wgpu::BlendState {
            color: component(F::One, F::One),
            alpha: over_alpha,
        },
    }
}

/// Source atop destination: only where something is already drawn, keeping
/// what is drawn there as the coverage.
const ATOP: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::DstAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Takes away what is already drawn, as much as it covers.
const ERASE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

impl Compositor {
    fn new(r: &Renderer, device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fanimate composite"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fanimate composite"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fanimate composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let composite = Blend::ALL
            .iter()
            .map(|blend| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("fanimate composite"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState {
                        count: r.sample_count,
                        ..Default::default()
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: r.format,
                            blend: Some(blend_state(*blend)),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        Self {
            atop: r.scene_pipeline(device, ATOP, "fanimate atop"),
            erase: r.scene_pipeline(device, ERASE, "fanimate erase"),
            composite,
            layout,
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fanimate composite settings"),
                size: PARAMS_STRIDE * MAX_COMPOSITES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            slots: Vec::new(),
            free: Vec::new(),
            size: (0, 0),
        }
    }

    /// Forget the textures when the target changes size.
    fn resize(&mut self, size: (u32, u32)) {
        if self.size != size {
            self.slots.clear();
            self.free.clear();
            self.size = size;
        }
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// What stays the same while a frame's scopes are drawn.
struct Frame<'a, 'b> {
    target: RenderTarget<'a>,
    composition: Composition<'b>,
    events: Vec<Vec<Event>>,
    runs: &'b [Run],
    media: &'b Media,
    clear: Color,
    /// Pictures laid down so far, which is where the next one's settings go.
    composites: usize,
}

impl Renderer {
    /// Draw a frame whose mesh has more than the root scope. The mesh, the
    /// uniforms and the pictures are already uploaded (see `render`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_composed(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: RenderTarget<'_>,
        composition: Composition<'_>,
        runs: &[Run],
        media: &Media,
        clear: Color,
    ) {
        if self.compositor.is_none() {
            self.compositor = Some(Compositor::new(self, device));
        }
        if let Some(c) = &mut self.compositor {
            c.resize(target.size);
        }
        let mut frame = Frame {
            target,
            composition,
            events: plan(composition.scopes, composition.pieces),
            runs,
            media,
            clear,
            composites: 0,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fanimate composed frame"),
        });
        self.run_scope(device, queue, &mut encoder, &mut frame, 0, Dest::Root);
        queue.submit([encoder.finish()]);
    }

    /// A pipeline that draws the mesh's triangles with `blend`.
    pub(super) fn scene_pipeline(
        &self,
        device: &wgpu::Device,
        blend: wgpu::BlendState,
        label: &str,
    ) -> wgpu::RenderPipeline {
        super::create_scene_pipeline(
            device,
            &self.shader,
            &self.pipeline_layout,
            self.format,
            self.sample_count,
            blend,
            label,
        )
    }

    /// Draw one scope into `dest`: its own pieces, and every scope opened in it
    /// drawn into a texture and laid down where it comes.
    fn run_scope(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &mut Frame<'_, '_>,
        scope: ScopeId,
        dest: Dest,
    ) {
        let events = std::mem::take(&mut frame.events[scope as usize]);
        let mut batch: Vec<Piece> = Vec::new();
        let mut cleared = false;
        // the masks opened in this scope, kept until it is done
        let mut masks: Vec<(ScopeId, usize)> = Vec::new();
        let clear = match dest {
            Dest::Root => Some(frame.clear),
            Dest::Slot(_) => Some(Color::TRANSPARENT),
        };
        for event in events {
            match event {
                Event::Draw(i) => batch.push(frame.composition.pieces[i]),
                Event::Child(child) => {
                    self.flush(encoder, frame, dest, &mut batch, &mut cleared, clear);
                    let slot = self.acquire(device, frame.target.size);
                    self.run_scope(device, queue, encoder, frame, child, Dest::Slot(slot));
                    match frame.composition.scopes[child as usize].kind {
                        ScopeKind::Layer { opacity, blend, mask } => {
                            // a mask that nothing was drawn into lets nothing through
                            let held = match mask {
                                Some(m) => masks.iter().find(|(id, _)| *id == m).map(|(_, s)| Some(*s)),
                                None => Some(None),
                            };
                            if let Some(mask_slot) = held {
                                self.lay_down(
                                    device, queue, encoder, frame, dest, slot, mask_slot, opacity, blend,
                                );
                            }
                            self.release(slot);
                        }
                        ScopeKind::Mask => masks.push((child, slot)),
                        ScopeKind::Root => self.release(slot),
                    }
                }
            }
        }
        self.flush(encoder, frame, dest, &mut batch, &mut cleared, clear);
        // a scope nothing was drawn in is still cleared
        if !cleared {
            self.flush(encoder, frame, dest, &mut Vec::new(), &mut cleared, clear);
        }
        for (_, slot) in masks {
            self.release(slot);
        }
    }

    /// Draw the pieces gathered so far in one pass, clearing the surface first
    /// if this is the first thing drawn on it.
    fn flush(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame<'_, '_>,
        dest: Dest,
        batch: &mut Vec<Piece>,
        cleared: &mut bool,
        clear: Option<Color>,
    ) {
        if batch.is_empty() && *cleared {
            return;
        }
        let load = match (*cleared, clear) {
            (false, Some(c)) => {
                let c = c.to_linear_premultiplied();
                wgpu::LoadOp::Clear(wgpu::Color {
                    r: c[0] as f64,
                    g: c[1] as f64,
                    b: c[2] as f64,
                    a: c[3] as f64,
                })
            }
            _ => wgpu::LoadOp::Load,
        };
        *cleared = true;
        let (color, resolve) = self.surface(dest, &frame.target);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fanimate scope"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if !batch.is_empty() {
            let c = self.compositor.as_ref().expect("made before drawing");
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
            for piece in batch.iter() {
                pass.set_pipeline(match piece.ink {
                    Ink::Over => &self.pipeline,
                    Ink::Atop => &c.atop,
                    Ink::Erase => &c.erase,
                });
                self.draw_range(&mut pass, frame.runs, frame.media, piece.first, piece.end);
            }
        }
        drop(pass);
        batch.clear();
    }

    /// The view to draw into for `dest` and the one the picture is resolved to.
    fn surface<'a>(
        &'a self,
        dest: Dest,
        target: &'a RenderTarget<'a>,
    ) -> (&'a wgpu::TextureView, Option<&'a wgpu::TextureView>) {
        match dest {
            Dest::Root => match &self.msaa {
                Some((msaa, _)) if self.sample_count > 1 => (msaa, Some(target.view)),
                _ => (target.view, None),
            },
            Dest::Slot(i) => {
                let slot = &self.compositor.as_ref().expect("made before drawing").slots[i];
                match &slot.msaa {
                    Some(msaa) => (msaa, Some(&slot.view)),
                    None => (&slot.view, None),
                }
            }
        }
    }

    /// Lay the picture in slot `src` over `dest`, faded by `opacity`, mixed as
    /// `blend` says and held to the mask in slot `mask` when there is one.
    #[allow(clippy::too_many_arguments)]
    fn lay_down(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &mut Frame<'_, '_>,
        dest: Dest,
        src: usize,
        mask: Option<usize>,
        opacity: f32,
        blend: Blend,
    ) {
        if frame.composites >= MAX_COMPOSITES {
            log::warn!(
                "more than {MAX_COMPOSITES} pictures laid over others in a frame; the rest are left out"
            );
            return;
        }
        let at = frame.composites;
        frame.composites += 1;
        let c = self.compositor.as_ref().expect("made before drawing");
        let settings: [f32; 4] = [
            opacity.clamp(0.0, 1.0),
            if mask.is_some() { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ];
        queue.write_buffer(
            &c.params,
            at as u64 * PARAMS_STRIDE,
            bytemuck::cast_slice(&settings),
        );
        let src_view = &c.slots[src].view;
        // without a mask the second texture is never read; any picture will do
        let mask_view = &c.slots[mask.unwrap_or(src)].view;
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fanimate composite"),
            layout: &c.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &c.params,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
            ],
        });
        let (color, resolve) = self.surface(dest, &frame.target);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fanimate lay down"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let which = Blend::ALL.iter().position(|b| *b == blend).unwrap_or(0);
        pass.set_pipeline(&c.composite[which]);
        pass.set_bind_group(0, &group, &[(at as u64 * PARAMS_STRIDE) as u32]);
        pass.draw(0..3, 0..1);
    }

    /// A texture to draw a scope into: one that was laid down already, or a new one.
    fn acquire(&mut self, device: &wgpu::Device, size: (u32, u32)) -> usize {
        let (format, samples) = (self.format, self.sample_count);
        let c = self.compositor.as_mut().expect("made before drawing");
        if let Some(i) = c.free.pop() {
            return i;
        }
        let extent = wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        };
        let resolved = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fanimate scope"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = resolved.create_view(&Default::default());
        let mut textures = vec![resolved];
        let msaa = (samples > 1).then(|| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fanimate scope msaa"),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = t.create_view(&Default::default());
            textures.push(t);
            view
        });
        c.slots.push(Slot {
            msaa,
            view,
            _textures: textures,
        });
        c.slots.len() - 1
    }

    /// Draw the mesh's triangles from index `first` to `end`, each with the
    /// picture the runs say it has.
    fn draw_range(&self, pass: &mut wgpu::RenderPass<'_>, runs: &[Run], media: &Media, first: u32, end: u32) {
        // the picture at an index is the one of the last run that starts at or before it
        let mut next = runs.partition_point(|r| r.first <= first);
        let mut at = first;
        while at < end {
            let current = next.checked_sub(1).map(|i| &runs[i]);
            let until = runs.get(next).map_or(end, |r| r.first).min(end);
            let group = match current.and_then(|r| r.media.as_deref()) {
                None => &self.white,
                Some(name) => match media.get(name) {
                    Some(_) => self.textures.get(name).map_or(&self.missing, |t| &t.1),
                    None => &self.missing,
                },
            };
            if until > at {
                pass.set_bind_group(1, group, &[]);
                pass.draw_indexed(at..until, 0, 0..1);
            }
            at = until;
            next += 1;
        }
    }

    fn release(&mut self, slot: usize) {
        if let Some(c) = &mut self.compositor {
            c.free.push(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::ROOT_SCOPE;

    fn scope(parent: ScopeId, kind: ScopeKind) -> Scope {
        Scope { parent, kind }
    }

    fn piece(scope: ScopeId, first: u32, end: u32) -> Piece {
        Piece {
            scope,
            ink: Ink::Over,
            first,
            end,
        }
    }

    fn layer() -> ScopeKind {
        ScopeKind::Layer {
            opacity: 1.0,
            blend: Blend::Normal,
            mask: None,
        }
    }

    fn shape(events: &[Vec<Event>]) -> Vec<Vec<String>> {
        events
            .iter()
            .map(|list| {
                list.iter()
                    .map(|e| match e {
                        Event::Draw(i) => format!("draw {i}"),
                        Event::Child(s) => format!("child {s}"),
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_scope_is_laid_down_where_its_first_piece_comes_and_nested_ones_inside_it() {
        let scopes = [
            Scope::ROOT,
            scope(ROOT_SCOPE, layer()),
            scope(1, layer()),
            scope(ROOT_SCOPE, layer()),
        ];
        // root, scope 1, scope 2 inside it, back to scope 1, root, scope 3
        let pieces = [
            piece(0, 0, 3),
            piece(1, 3, 6),
            piece(2, 6, 9),
            piece(1, 9, 12),
            piece(0, 12, 15),
            piece(3, 15, 18),
        ];
        assert_eq!(
            shape(&plan(&scopes, &pieces)),
            [
                vec!["draw 0", "child 1", "draw 4", "child 3"],
                vec!["draw 1", "child 2", "draw 3"],
                vec!["draw 2"],
                vec!["draw 5"],
            ]
        );
    }

    #[test]
    fn a_scope_with_nothing_in_it_is_never_drawn_and_a_deep_one_brings_its_parents() {
        let scopes = [
            Scope::ROOT,
            scope(ROOT_SCOPE, layer()),
            scope(1, layer()),
            scope(ROOT_SCOPE, ScopeKind::Mask),
        ];
        // only the innermost scope has a piece, and an empty piece counts for nothing
        let pieces = [piece(2, 0, 4), piece(3, 4, 4)];
        assert_eq!(
            shape(&plan(&scopes, &pieces)),
            [vec!["child 1"], vec!["child 2"], vec!["draw 0"], vec![]]
        );
    }

    #[test]
    fn damaged_scope_tables_are_taken_as_the_root_and_never_panic() {
        // a parent that is not an earlier scope, and a piece for a scope that does not exist
        let scopes = [Scope::ROOT, scope(7, layer())];
        let events = plan(&scopes, &[piece(1, 0, 3), piece(40, 3, 6)]);
        assert_eq!(shape(&events)[0], ["child 1"]);
        assert_eq!(shape(&events)[1], ["draw 0", "draw 1"]);
    }
}
