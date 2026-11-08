//! GPU setup and headless rendering to an image.

use fanimate_core::eval::Evaluator;
use fanimate_core::{Color, Document};

use crate::{Mesh, RenderError, RenderTarget, Renderer, Tessellator, View};

/// An open GPU device.
///
/// Holds on to the instance as well: some drivers crash if the instance is
/// destroyed while a device created from it is still alive.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter: wgpu::Adapter,
    pub instance: wgpu::Instance,
}

impl Gpu {
    /// Open a device on `instance`, able to present to `surface` if given.
    pub async fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, RenderError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("fanimate"),
                // WebGL2 limits keep the browser build in reach
                required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            device,
            queue,
            adapter,
            instance,
        })
    }

    /// Open a device for offscreen work, with no window.
    pub fn headless() -> Result<Self, RenderError> {
        pollster::block_on(Self::new(instance(), None))
    }

    /// Whether this is a software rasteriser (lavapipe, llvmpipe, WARP)
    /// rather than a real GPU. Its timings say nothing about GPU speed.
    pub fn is_software(&self) -> bool {
        self.adapter.get_info().device_type == wgpu::DeviceType::Cpu
    }
}

/// Create the wgpu instance with Fanimate's backend choice (see
/// [`backends`]).
pub fn instance() -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends();
    wgpu::Instance::new(desc.with_env())
}

/// The graphics backends Fanimate uses, honouring `WGPU_BACKEND` (e.g.
/// `WGPU_BACKEND=vulkan`). Also used to configure the app window's renderer.
///
/// On Windows this is DirectX 12, plus OpenGL only when DirectX 12 has no
/// hardware adapter (old GPUs). Vulkan is left out because some older Intel
/// drivers (seen on HD Graphics 520) crash the whole process while the
/// Vulkan driver loads, which cannot be caught. OpenGL is not enabled next to
/// DirectX 12 because listing its adapters loads the OpenGL driver, and
/// Intel's crashes in about 1 of 40 runs as the process exits. Elsewhere
/// wgpu's primary backends are used (Vulkan, Metal, WebGPU).
pub fn backends() -> wgpu::Backends {
    if let Some(from_env) = wgpu::Backends::from_env() {
        return from_env;
    }
    #[cfg(windows)]
    {
        static CHOICE: std::sync::OnceLock<wgpu::Backends> = std::sync::OnceLock::new();
        *CHOICE.get_or_init(|| {
            if has_hardware_adapter(wgpu::Backends::DX12) {
                wgpu::Backends::DX12
            } else {
                wgpu::Backends::DX12 | wgpu::Backends::GL
            }
        })
    }
    #[cfg(not(windows))]
    wgpu::Backends::PRIMARY
}

/// Whether `backends` offers a real GPU. DirectX 12 nearly always lists the
/// WARP software renderer, which must not win over an OpenGL GPU.
#[cfg(windows)]
fn has_hardware_adapter(backends: wgpu::Backends) -> bool {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = backends;
    let adapters = pollster::block_on(wgpu::Instance::new(desc).enumerate_adapters(backends));
    adapters
        .iter()
        .any(|a| a.get_info().device_type != wgpu::DeviceType::Cpu)
}

/// Render `doc` at `frame` to straight-alpha sRGB RGBA8 pixels, with the
/// canvas filling the image. Uses 4x MSAA, like the interactive canvas.
pub fn render_to_rgba(
    gpu: &Gpu,
    doc: &Document,
    frame: f32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let mut target = Offscreen::new(gpu, width, height);
    target.draw(gpu, doc, frame);
    target.read_rgba(gpu)
}

/// An offscreen image that documents are rendered into, frame after frame.
///
/// The pipeline, the target texture and the tessellation buffers live as
/// long as this does, so rendering a sequence (the GPU benchmark, and video
/// export later) does not rebuild them for every frame.
pub struct Offscreen {
    renderer: Renderer,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    size: (u32, u32),
    evaluator: Evaluator,
    tess: Tessellator,
    mesh: Mesh,
}

impl Offscreen {
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// A `width` by `height` pixel target (at least 1x1), rendered with 4x
    /// MSAA like the interactive canvas.
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let size = (width.max(1), height.max(1));
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fanimate offscreen"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self {
            renderer: Renderer::new(&gpu.device, &gpu.queue, Self::FORMAT, 4),
            target_view: target.create_view(&Default::default()),
            target,
            size,
            evaluator: Evaluator::for_render(),
            tess: Tessellator::new(),
            mesh: Mesh::new(),
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Triangles in the last frame drawn.
    pub fn triangles(&self) -> usize {
        self.mesh.indices.len() / 3
    }

    /// Evaluate, tessellate and draw `doc` at `frame`, with the canvas
    /// filling the image. Returns once the GPU work is submitted; use
    /// [`Offscreen::wait`] or [`Offscreen::read_rgba`] to wait for it.
    pub fn draw(&mut self, gpu: &Gpu, doc: &Document, frame: f32) {
        let view = View::fit(doc.size(), self.size, 0.0);
        let mut items = Vec::new();
        self.evaluator.evaluate_into(doc, frame, &mut items);
        // the picture the document's camera takes, if it has one
        let camera = fanimate_core::camera_at(doc, frame).unwrap_or(fanimate_core::Affine2::IDENTITY);
        let zoom = (view.zoom * camera.matrix2.x_axis.length()).max(1e-6);
        self.tess.clear(&mut self.mesh);
        self.tess.tolerance = 0.25 / zoom;
        self.tess.canvas_through(doc, &mut self.mesh, camera);
        self.tess.draw_items(&items, &mut self.mesh);
        self.renderer.render(
            &gpu.device,
            &gpu.queue,
            RenderTarget {
                view: &self.target_view,
                size: self.size,
            },
            &view,
            &self.mesh,
            &self.tess.runs,
            self.tess.composition(&self.evaluator.scopes),
            &doc.media,
            Color::TRANSPARENT,
        );
    }

    /// Block until the GPU has finished everything submitted so far.
    pub fn wait(&self, gpu: &Gpu) -> Result<(), RenderError> {
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|_| RenderError::Readback)
    }

    /// The last frame drawn, as straight-alpha sRGB RGBA8 pixels.
    pub fn read_rgba(&self, gpu: &Gpu) -> Result<Vec<u8>, RenderError> {
        let (width, height) = self.size;
        // rows in the buffer must be 256-byte aligned
        let row = width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fanimate readback"),
            size: padded as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.wait(gpu)?;
        rx.recv()
            .map_err(|_| RenderError::Readback)?
            .map_err(|_| RenderError::Readback)?;

        let data = buffer.get_mapped_range(..).map_err(|_| RenderError::Readback)?;
        let mut pixels = Vec::with_capacity((row * height) as usize);
        for y in 0..height as usize {
            let start = y * padded as usize;
            pixels.extend_from_slice(&data[start..start + row as usize]);
        }
        drop(data);
        buffer.unmap();
        unpremultiply(&mut pixels);
        Ok(pixels)
    }
}

/// The GPU blends premultiplied; image files expect straight alpha.
fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = px[3];
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::samples;

    /// GPU tests need a GPU (or a software adapter such as lavapipe or
    /// WARP) and skip, with a note, on machines with neither.
    ///
    /// Keep one shared device for the whole test process: creating and tearing
    /// down several headless devices in one run can trip flaky Windows driver
    /// faults.
    fn gpu_or_skip() -> Option<&'static Gpu> {
        static GPU: std::sync::OnceLock<Option<Gpu>> = std::sync::OnceLock::new();
        GPU.get_or_init(|| {
            Gpu::headless()
                .inspect_err(|e| eprintln!("skipping GPU test: {e}"))
                .ok()
        })
        .as_ref()
    }

    #[test]
    fn renders_hello_sample() {
        let Some(gpu) = gpu_or_skip() else { return };
        let doc = samples::hello();
        let (w, h) = (320, 180);
        let px = render_to_rgba(gpu, &doc, 0.0, w, h).unwrap();
        assert_eq!(px.len(), (w * h * 4) as usize);

        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        // corner is the violet-950 background, #1C0B3D
        let bg = at(2, 2);
        assert!(
            bg[0].abs_diff(0x1C) <= 2 && bg[1].abs_diff(0x0B) <= 2 && bg[2].abs_diff(0x3D) <= 2,
            "{bg:?}"
        );
        assert_eq!(bg[3], 255);
        // the F's stem is white: stem centre is at x = 570 of 1280
        let stem = at(570 * w / 1280, h / 2 + 30);
        assert!(stem.iter().all(|&c| c >= 250), "{stem:?}");
    }

    #[test]
    fn pictures_render_and_missing_ones_show_grey() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = fanimate_core::Document::new("p", 100.0, 100.0, 24.0);
        doc.background = fanimate_core::Color::WHITE;
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let name = fanimate_core::MediaFile::name_for(&bytes, "png");
        doc.media.insert(
            name.clone(),
            std::sync::Arc::new(fanimate_core::MediaFile { bytes }),
        );
        let blue = doc.new_layer(
            "blue",
            fanimate_core::LayerKind::Image(fanimate_core::Image::new(name, 50.0, 100.0)),
        );
        let mut grey = doc.new_layer(
            "gone",
            fanimate_core::LayerKind::Image(fanimate_core::Image::new("media/gone.png", 50.0, 100.0)),
        );
        grey.transform.position.base = fanimate_core::Vec2::new(50.0, 0.0);
        doc.layers.extend([blue, grey]);
        let px = render_to_rgba(gpu, &doc, 0.0, 100, 100).unwrap();
        let at = |x: usize, y: usize| &px[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3];
        assert_eq!(at(20, 50), [0, 0, 255]);
        let g = at(80, 50);
        assert!(g[0] == g[1] && g[1] == g[2] && g[0] > 100 && g[0] < 250, "{g:?}");
    }

    // ---- compositing: masks, clips, blend modes and flattened groups ----

    use fanimate_core::{Blend, Color, Document, Layer, LayerKind, MaskRole, Path, Shape, Style, Vec2};

    /// A 100 by 100 white canvas, so a pixel of the 100 by 100 render is a unit.
    fn canvas() -> Document {
        let mut doc = Document::new("c", 100.0, 100.0, 24.0);
        doc.background = Color::WHITE;
        doc
    }

    fn square(doc: &mut Document, name: &str, from: f32, to: f32, color: Color) -> Layer {
        let path = Path::rect(Vec2::splat(from), Vec2::splat(to - from), 0.0);
        let shape = Shape::new(name, path, Style::fill(color));
        doc.new_layer(name, LayerKind::vector(vec![shape]))
    }

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);

    /// The colour (red, green, blue) of pixel x, y of a 100 by 100 render.
    fn pixel(gpu: &Gpu, doc: &Document, x: usize, y: usize) -> [u8; 3] {
        let px = render_to_rgba(gpu, doc, 0.0, 100, 100).unwrap();
        let i = (y * 100 + x) * 4;
        [px[i], px[i + 1], px[i + 2]]
    }

    fn near(got: [u8; 3], want: [u8; 3]) -> bool {
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 4)
    }

    const WHITE_PX: [u8; 3] = [255, 255, 255];

    #[test]
    fn a_flattened_group_fades_as_one_picture_and_a_plain_one_fades_each_shape() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = canvas();
        let (a, b) = (
            square(&mut doc, "a", 0.0, 60.0, RED),
            square(&mut doc, "b", 40.0, 100.0, RED),
        );
        let mut group = doc.new_layer("g", LayerKind::Group { children: vec![a, b] });
        group.opacity.base = 0.5;
        doc.layers.push(group);
        // each shape is half there: where they overlap, three quarters
        // (blended in linear light, so half is 188 of 255 and three quarters 137)
        assert!(
            near(pixel(gpu, &doc, 20, 20), [255, 188, 188]),
            "{:?}",
            pixel(gpu, &doc, 20, 20)
        );
        assert!(
            near(pixel(gpu, &doc, 50, 50), [255, 137, 137]),
            "{:?}",
            pixel(gpu, &doc, 50, 50)
        );
        // flattened, the overlap is no darker than the rest
        doc.layers[0].composite.flatten = true;
        assert!(near(pixel(gpu, &doc, 20, 20), [255, 188, 188]));
        assert!(
            near(pixel(gpu, &doc, 50, 50), [255, 188, 188]),
            "{:?}",
            pixel(gpu, &doc, 50, 50)
        );
        assert!(near(pixel(gpu, &doc, 90, 10), WHITE_PX));
    }

    #[test]
    fn what_is_masked_shows_only_inside_the_mask_layer() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = canvas();
        let mut mask = square(&mut doc, "mask", 20.0, 80.0, Color::BLACK);
        mask.composite.mask = MaskRole::Add;
        let art = square(&mut doc, "art", 0.0, 100.0, BLUE);
        doc.layers.extend([mask, art]);
        assert!(near(pixel(gpu, &doc, 50, 50), [0, 0, 255]));
        assert!(near(pixel(gpu, &doc, 10, 10), WHITE_PX), "outside the mask");
        assert!(near(pixel(gpu, &doc, 90, 50), WHITE_PX));
        // the mask layer is not drawn itself: black does not show
        assert!(near(pixel(gpu, &doc, 25, 25), [0, 0, 255]));
        // hiding the mask turns it off
        doc.layers[0].visible = false;
        assert!(near(pixel(gpu, &doc, 10, 10), [0, 0, 255]));
    }

    #[test]
    fn a_mask_layer_can_take_away_and_an_unmasked_layer_ignores_it() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = canvas();
        let mut add = square(&mut doc, "add", 10.0, 90.0, Color::BLACK);
        add.composite.mask = MaskRole::Add;
        let mut take = square(&mut doc, "take", 40.0, 60.0, Color::BLACK);
        take.composite.mask = MaskRole::Subtract;
        let art = square(&mut doc, "art", 0.0, 100.0, BLUE);
        let mut free = square(&mut doc, "free", 0.0, 8.0, GREEN);
        free.composite.unmasked = true;
        doc.layers.extend([add, take, art, free]);
        assert!(near(pixel(gpu, &doc, 20, 20), [0, 0, 255]), "let through");
        assert!(
            near(pixel(gpu, &doc, 50, 50), WHITE_PX),
            "taken away: {:?}",
            pixel(gpu, &doc, 50, 50)
        );
        assert!(
            near(pixel(gpu, &doc, 3, 3), [0, 255, 0]),
            "the unmasked layer is outside the mask and shows"
        );
        assert!(near(pixel(gpu, &doc, 95, 95), WHITE_PX));
    }

    #[test]
    fn a_clipped_layer_shows_only_where_its_base_is() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = canvas();
        let base = square(&mut doc, "base", 20.0, 60.0, RED);
        let mut paint = square(&mut doc, "paint", 0.0, 100.0, GREEN);
        paint.composite.clip = true;
        doc.layers.extend([base, paint]);
        assert!(
            near(pixel(gpu, &doc, 30, 30), [0, 255, 0]),
            "{:?}",
            pixel(gpu, &doc, 30, 30)
        );
        assert!(near(pixel(gpu, &doc, 80, 80), WHITE_PX), "outside the base");
        assert!(near(pixel(gpu, &doc, 10, 10), WHITE_PX));
        // hiding the base hides what is clipped to it
        doc.layers[0].visible = false;
        assert!(near(pixel(gpu, &doc, 30, 30), WHITE_PX));
    }

    #[test]
    fn blend_modes_mix_with_what_is_under_them() {
        let Some(gpu) = gpu_or_skip() else { return };
        let mut doc = canvas();
        let under = square(&mut doc, "yellow", 0.0, 60.0, Color::rgb(1.0, 1.0, 0.0));
        let mut over = square(&mut doc, "cyan", 40.0, 100.0, Color::rgb(0.0, 1.0, 1.0));
        over.composite.blend = Blend::Multiply;
        doc.layers.extend([under, over]);
        // yellow times cyan is green; cyan alone over white is cyan; the rest stays white
        assert!(
            near(pixel(gpu, &doc, 50, 50), [0, 255, 0]),
            "{:?}",
            pixel(gpu, &doc, 50, 50)
        );
        assert!(near(pixel(gpu, &doc, 90, 90), [0, 255, 255]));
        assert!(near(pixel(gpu, &doc, 10, 10), [255, 255, 0]));
        // screen lightens: dark red and dark blue make a brighter purple than either
        doc.layers[1].composite.blend = Blend::Screen;
        doc.layers[0] = square(&mut doc, "a", 0.0, 60.0, Color::rgb(0.5, 0.0, 0.0));
        doc.layers[1] = {
            let mut l = square(&mut doc, "b", 40.0, 100.0, Color::rgb(0.0, 0.0, 0.5));
            l.composite.blend = Blend::Screen;
            l
        };
        let both = pixel(gpu, &doc, 50, 50);
        assert!(both[0] > 100 && both[2] > 100, "{both:?}");
        // add adds the lights
        doc.layers[1].composite.blend = Blend::Add;
        let added = pixel(gpu, &doc, 50, 50);
        assert!(added[0] > 100 && added[2] > 100, "{added:?}");
    }

    #[test]
    fn a_plain_document_renders_the_same_with_the_scopes_in_place() {
        let Some(gpu) = gpu_or_skip() else { return };
        // nothing here uses compositing, so the frame is one pass as it always was
        let doc = samples::hello();
        let mut target = Offscreen::new(gpu, 160, 90);
        target.draw(gpu, &doc, 0.0);
        let first = target.read_rgba(gpu).unwrap();
        // and a document that does use it still draws every plain layer the same
        let mut mixed = samples::hello();
        let n = mixed.layers.len();
        let mut extra = mixed.new_layer("extra", LayerKind::vector(vec![]));
        extra.composite.flatten = true;
        let group = mixed.new_layer(
            "g",
            LayerKind::Group {
                children: vec![extra],
            },
        );
        mixed.layers.push(group);
        mixed.layers.last_mut().unwrap().composite.flatten = true;
        assert!(mixed.layers.len() > n);
        target.draw(gpu, &mixed, 0.0);
        assert_eq!(
            target.read_rgba(gpu).unwrap(),
            first,
            "an empty flattened group changes nothing"
        );
    }

    /// Headless rendering is deterministic: a frame gives the same pixels
    /// drawn fresh or on a target that has already drawn other frames.
    #[test]
    fn same_frame_renders_identically() {
        let Some(gpu) = gpu_or_skip() else { return };
        let doc = samples::benchmark(40);
        let (w, h) = (320, 180);
        let fresh = render_to_rgba(gpu, &doc, 12.0, w, h).unwrap();

        let mut target = Offscreen::new(gpu, w, h);
        for frame in [0.0, 60.0, 12.0] {
            target.draw(gpu, &doc, frame);
        }
        assert_eq!(target.read_rgba(gpu).unwrap(), fresh);

        // and the benchmark really is animated, so the check means something
        target.draw(gpu, &doc, 60.0);
        assert_ne!(target.read_rgba(gpu).unwrap(), fresh);
    }

    /// The OpenGL driver is only loaded when DirectX 12 has no GPU, because
    /// Intel's crashes intermittently at process exit.
    #[cfg(windows)]
    #[test]
    fn windows_leaves_opengl_out_when_directx_has_a_gpu() {
        if std::env::var_os("WGPU_BACKEND").is_some() {
            return;
        }
        let expected = if has_hardware_adapter(wgpu::Backends::DX12) {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::DX12 | wgpu::Backends::GL
        };
        assert_eq!(backends(), expected);
    }

    #[test]
    fn unpremultiply_restores_straight_alpha() {
        let mut px = [64, 0, 128, 128];
        unpremultiply(&mut px);
        assert_eq!(px, [128, 0, 255, 128]);
    }
}
