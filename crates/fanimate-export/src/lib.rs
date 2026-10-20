//! Exporting animation: rendering a project to a video file, an animated
//! GIF or a PNG sequence, and the render queue that does it in a separate
//! process.
//!
//! Frames are rendered headless with `fanimate-render`, the same renderer
//! as the canvas, then written by one of three (see [`Format::writers`]):
//!
//! - Fanimate itself: picture sequences (PNG, OpenEXR, TIFF), GIF,
//!   animated PNG, uncompressed AVI, and AV1 video.
//! - The operating system's H.264 and AAC encoders (`fanimate-oscodec`)
//!   for MP4 on Windows and macOS.
//! - The FFmpeg the user has, for ProRes, DNxHR, H.265, WebM and more, run
//!   as a separate program and fed raw frames; Fanimate does not include
//!   it, and nothing of it is linked in.
//!
//! The queue lives in a folder: one subfolder per job holding a copy of the
//! project as it was when queued, its settings and its progress, plus the
//! order the jobs run in. The editor adds jobs; the queue window (`fanimate
//! --render-queue`) runs them, so a render goes on after the editor closes.

mod apng;
mod av1;
mod avi;
pub mod ffmpeg;
pub mod job;
pub mod queue;
pub mod run;
mod sequence;
pub mod sound;
mod yuv;

pub use job::{Format, Settings, Writer};
pub use queue::{Entry, Job, Queue, State, Status};
pub use run::{ExportError, Route, existing_output, export, route};
