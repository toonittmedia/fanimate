//! Fanimate GPU renderer.
//!
//! Vector shapes are tessellated into triangles with lyon ([`tess`]) and
//! drawn with wgpu ([`Renderer`]). The canvas and final export share this
//! code, so what the artist sees is exactly what renders.
//!
//! This is the milestone M1 spike: tessellation runs on the CPU every frame.
//! Caching static layers and moving tessellation to the GPU are the next
//! performance steps once the benchmark shows where the time goes.

mod gpu;
mod offscreen;
pub mod tess;

pub use gpu::{Composition, RenderTarget, Renderer, View};
pub use offscreen::{Gpu, Offscreen, backends, instance, render_to_rgba};
pub use tess::{Mesh, Piece, Tessellator, Vertex};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no suitable GPU adapter found: {0}")]
    NoAdapter(#[from] wgpu::RequestAdapterError),
    #[error("could not open the GPU device: {0}")]
    NoDevice(#[from] wgpu::RequestDeviceError),
    #[error("reading the rendered image back failed")]
    Readback,
}
