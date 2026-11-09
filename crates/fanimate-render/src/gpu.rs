//! The wgpu pipeline that draws tessellated meshes.

use std::collections::HashMap;

use fanimate_core::{Color, Media, Vec2};

mod compose;

pub use compose::Composition;

use crate::tess::{Mesh, Run, Vertex};

const SHADER: &str = r#"
struct Globals {
    scale: vec2<f32>,
    offset: vec2<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>, @location(2) uv: vec2<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(p * g.scale + g.offset, 0.0, 1.0);
    o.color = c;
    o.uv = uv;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    // flat colour samples a white pixel; pictures are straight alpha
    let t = textureSample(tex, samp, i.uv);
    return vec4<f32>(t.rgb * t.a, t.a) * i.color;
}
"#;

/// Maps document coordinates to clip space: `clip = doc * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: Vec2,
    pub offset: Vec2,
    /// Target pixels per document unit; use for tessellation tolerance.
    pub zoom: f32,
}

impl View {
    /// Fit a document of `doc_size` centred in a `target` of pixels, with
    /// `margin` pixels of pasteboard around it.
    pub fn fit(doc_size: Vec2, target: (u32, u32), margin: f32) -> Self {
        let t = Vec2::new(target.0.max(1) as f32, target.1.max(1) as f32);
        let avail = (t - Vec2::splat(2.0 * margin)).max(Vec2::ONE);
        let zoom = (avail / doc_size.max(Vec2::ONE)).min_element();
        Self::centered(doc_size, t, zoom)
    }

    /// Document centred in the target at an exact zoom.
    pub fn centered(doc_size: Vec2, target: Vec2, zoom: f32) -> Self {
        Self::looking_at(doc_size / 2.0, target, zoom)
    }

    /// The document point `center` in the middle of the target, at `zoom`
    /// target pixels per document unit. This is how the canvas pans.
    pub fn looking_at(center: Vec2, target: Vec2, zoom: f32) -> Self {
        // y points down in documents and up in clip space
        let scale = Vec2::new(2.0 * zoom / target.x.max(1.0), -2.0 * zoom / target.y.max(1.0));
        let offset = -center * scale;
        Self { scale, offset, zoom }
    }
}

/// A texture to draw into, with its size in pixels.
#[derive(Clone, Copy)]
pub struct RenderTarget<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: (u32, u32),
}

pub struct Renderer {
    format: wgpu::TextureFormat,
    sample_count: u32,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    /// What frames that use compositing need, made the first time one does.
    compositor: Option<compose::Compositor>,
    globals: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    msaa: Option<(wgpu::TextureView, (u32, u32))>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A white pixel, for flat colour.
    white: wgpu::BindGroup,
    /// A grey pixel, shown for a picture that is missing or unreadable.
    missing: wgpu::BindGroup,
    /// Uploaded pictures by media name, with the bytes they came from so a
    /// changed file is uploaded again.
    textures: HashMap<String, (std::sync::Arc<fanimate_core::MediaFile>, wgpu::BindGroup)>,
}

impl Renderer {
    /// `format` is the target texture format; `sample_count` is 1 or 4.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fanimate shapes"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fanimate globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fanimate globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fanimate globals"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fanimate picture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fanimate picture"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fanimate shapes"),
            bind_group_layouts: &[Some(&layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = create_scene_pipeline(
            device,
            &shader,
            &pipeline_layout,
            format,
            sample_count,
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
            "fanimate shapes",
        );

        let pixel = |rgba: [u8; 4]| texture_bind_group(device, queue, &texture_layout, &sampler, 1, 1, &rgba);
        Self {
            format,
            sample_count,
            shader,
            pipeline_layout,
            pipeline,
            compositor: None,
            globals,
            bind_group,
            vertices: empty_buffer(device, wgpu::BufferUsages::VERTEX),
            indices: empty_buffer(device, wgpu::BufferUsages::INDEX),
            msaa: None,
            white: pixel([255; 4]),
            missing: pixel([128, 128, 128, 160]),
            texture_layout,
            sampler,
            textures: HashMap::new(),
        }
    }

    /// The bind group for picture `name`, uploading it on first use.
    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        media: &Media,
        name: &str,
    ) -> &wgpu::BindGroup {
        let Some(file) = media.get(name) else {
            return &self.missing;
        };
        let fresh = self
            .textures
            .get(name)
            .is_some_and(|(f, _)| std::sync::Arc::ptr_eq(f, file));
        if !fresh {
            match image::load_from_memory(&file.bytes) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let group = texture_bind_group(
                        device,
                        queue,
                        &self.texture_layout,
                        &self.sampler,
                        rgba.width(),
                        rgba.height(),
                        rgba.as_raw(),
                    );
                    self.textures.insert(name.to_owned(), (file.clone(), group));
                }
                Err(e) => {
                    log::warn!("picture {name} could not be read: {e}");
                    return &self.missing;
                }
            }
        }
        &self.textures[name].1
    }

    /// Draw `mesh` into `target`, clearing it to `clear`. `runs` say which
    /// triangles show which picture from `media`.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: RenderTarget<'_>,
        view: &View,
        mesh: &Mesh,
        runs: &[Run],
        composition: Composition<'_>,
        media: &Media,
        clear: Color,
    ) {
        // a picture the project no longer holds (a painted tile that was replaced) is dropped,
        // so painting does not fill the graphics memory with old versions
        if self.textures.len() > media.len() {
            self.textures.retain(|name, _| media.contains_key(name));
        }
        // upload any new pictures before the pass borrows the cache
        for run in runs {
            if let Some(name) = &run.media {
                self.texture(device, queue, media, name);
            }
        }
        let RenderTarget { view: target, size } = target;
        let g = [view.scale.x, view.scale.y, view.offset.x, view.offset.y];
        queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&g));
        upload(
            device,
            queue,
            &mut self.vertices,
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&mesh.vertices),
        );
        upload(
            device,
            queue,
            &mut self.indices,
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&mesh.indices),
        );

        if self.sample_count > 1 && self.msaa.as_ref().is_none_or(|(_, s)| *s != size) {
            {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("fanimate msaa"),
                    size: wgpu::Extent3d {
                        width: size.0.max(1),
                        height: size.1.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                self.msaa = Some((tex.create_view(&Default::default()), size));
            }
        }
        // a frame that uses masks, clips, blend modes or flattened groups draws
        // its scopes one by one; one that does not is a single pass
        if composition.scopes.len() > 1 {
            self.render_composed(
                device,
                queue,
                RenderTarget { view: target, size },
                composition,
                runs,
                media,
                clear,
            );
            return;
        }
        let (color_view, resolve) = if self.sample_count > 1 {
            (&self.msaa.as_ref().expect("created above").0, Some(target))
        } else {
            (target, None)
        };

        let c = clear.to_linear_premultiplied();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fanimate frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fanimate shapes"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c[0] as f64,
                            g: c[1] as f64,
                            b: c[2] as f64,
                            a: c[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !mesh.indices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertices.slice(..));
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
                let total = mesh.indices.len() as u32;
                // everything before the first run, and any run, is drawn
                // with its picture or the white pixel
                let mut starts: Vec<(u32, Option<&str>)> = vec![(0, None)];
                starts.extend(runs.iter().map(|r| (r.first, r.media.as_deref())));
                for (i, &(first, name)) in starts.iter().enumerate() {
                    let end = starts.get(i + 1).map_or(total, |s| s.0).min(total);
                    if first >= end {
                        continue;
                    }
                    let group = match name {
                        None => &self.white,
                        Some(n) => match media.get(n) {
                            Some(_) => self.textures.get(n).map_or(&self.missing, |t| &t.1),
                            None => &self.missing,
                        },
                    };
                    pass.set_bind_group(1, group, &[]);
                    pass.draw_indexed(first..end, 0, 0..1);
                }
            }
        }
        queue.submit([encoder.finish()]);
    }
}

/// The pipeline that draws the mesh's triangles, laying them down with `blend`.
fn create_scene_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    blend: wgpu::BlendState,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2],
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// A texture of `width` by `height` RGBA8 sRGB pixels, ready to sample.
fn texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> wgpu::BindGroup {
    let size = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fanimate picture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let layout_info = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(4 * size.width),
        rows_per_image: Some(size.height),
    };
    let copy = wgpu::TexelCopyTextureInfo {
        texture: &texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    };
    queue.write_texture(copy, rgba, layout_info, size);
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fanimate picture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn empty_buffer(device: &wgpu::Device, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fanimate mesh"),
        size: 4096,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Write `data` into `buf`, growing it (to the next power of two) if needed.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buf: &mut wgpu::Buffer,
    usage: wgpu::BufferUsages,
    data: &[u8],
) {
    if data.is_empty() {
        return;
    }
    if (data.len() as u64) > buf.size() {
        let size = (data.len() as u64).next_power_of_two();
        *buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fanimate mesh"),
            size,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
    // write_buffer needs a multiple of 4 bytes; f32 and u32 data always is
    queue.write_buffer(buf, 0, data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centres_and_flips_y() {
        let v = View::fit(Vec2::new(100.0, 50.0), (200, 100), 0.0);
        assert_eq!(v.zoom, 2.0);
        let clip = |p: Vec2| p * v.scale + v.offset;
        assert!(clip(Vec2::ZERO).abs_diff_eq(Vec2::new(-1.0, 1.0), 1e-6));
        assert!(clip(Vec2::new(100.0, 50.0)).abs_diff_eq(Vec2::new(1.0, -1.0), 1e-6));
    }

    #[test]
    fn fit_letterboxes_tall_targets() {
        let v = View::fit(Vec2::new(100.0, 100.0), (400, 200), 0.0);
        assert_eq!(v.zoom, 2.0);
        let clip = |p: Vec2| p * v.scale + v.offset;
        // 200 px wide square in a 400 px target spans clip x -0.5..0.5
        assert!(clip(Vec2::ZERO).abs_diff_eq(Vec2::new(-0.5, 1.0), 1e-6));
    }
}
