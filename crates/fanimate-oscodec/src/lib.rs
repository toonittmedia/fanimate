//! The operating system's own video encoders, for MP4 export with nothing
//! else installed.
//!
//! Windows (Media Foundation) and macOS (AVFoundation) ship an H.264 video
//! and AAC sound encoder, licensed by Microsoft and Apple for the programs
//! that use them, and write MP4 files themselves. Fanimate hands them NV12
//! frames (converted by `fanimate-export`, so the colour matches every
//! other format) and stereo sound.
//!
//! This is the one crate in the workspace allowed `unsafe` code: both
//! frameworks are only reachable through foreign function calls. Every
//! call is wrapped here, behind the safe [`H264Writer`], and nothing
//! unsafe leaves the crate.

#![allow(unsafe_code)]

use std::path::Path;

#[cfg(any(windows, test))]
mod colour;
#[cfg(target_os = "macos")]
mod mac;
mod taskbar;
#[cfg(windows)]
mod win;

#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(windows)]
use win as imp;

pub use taskbar::{TaskbarProgress, set_taskbar_progress};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this system has no H.264 encoder Fanimate can use")]
    Unsupported,
    #[error("the system's video encoder failed: {0}")]
    System(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to write.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Even, and at most what the system encoder takes (4096x2304 on
    /// Windows).
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    /// Video bits per second.
    pub bit_rate: u32,
    /// Stereo sound at this rate (48 kHz) goes in the file too.
    pub sound_rate: Option<u32>,
}

/// Whether this computer has an H.264 encoder the system offers.
pub fn h264_available() -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        imp::h264_available()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// An MP4 file being written by the system: H.264 video, AAC sound.
pub struct H264Writer {
    #[cfg(any(windows, target_os = "macos"))]
    inner: imp::Writer,
}

impl H264Writer {
    /// Start writing `path` (any extension: it is always MP4).
    pub fn create(path: &Path, options: Options) -> Result<Self> {
        if options.width % 2 == 1 || options.height % 2 == 1 || options.width == 0 || options.height == 0 {
            return Err(Error::System("the picture size must be even".into()));
        }
        #[cfg(any(windows, target_os = "macos"))]
        {
            Ok(Self {
                inner: imp::Writer::create(path, options)?,
            })
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = path;
            Err(Error::Unsupported)
        }
    }

    /// Add frame `index` (NV12: the luma plane, then interleaved chroma),
    /// and the stereo samples that play during it.
    pub fn frame(&mut self, index: u32, nv12: &[u8], sound: &[f32]) -> Result<()> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.inner.frame(index, nv12, sound)
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = (index, nv12, sound);
            Err(Error::Unsupported)
        }
    }

    /// Finish the file. Dropping the writer without finishing leaves an
    /// unplayable file, which the caller deletes.
    pub fn finish(self) -> Result<()> {
        #[cfg(any(windows, target_os = "macos"))]
        {
            self.inner.finish()
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            Err(Error::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nv12(width: u32, height: u32, luma: u8) -> Vec<u8> {
        let mut v = vec![luma; (width * height) as usize];
        v.extend(std::iter::repeat_n(128, (width * height / 2) as usize));
        v
    }

    #[test]
    fn odd_sizes_are_refused_before_the_system_is_asked() {
        let o = Options {
            width: 63,
            height: 36,
            fps: 24.0,
            bit_rate: 1_000_000,
            sound_rate: None,
        };
        assert!(H264Writer::create(Path::new("x.mp4"), o).is_err());
    }

    /// Writes a real file where the system has an encoder; elsewhere checks
    /// that it says so.
    #[test]
    fn writes_an_mp4_with_video_and_sound() {
        let dir = std::env::temp_dir().join(format!("fanimate-oscodec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("tone.mp4.part");
        let o = Options {
            width: 64,
            height: 36,
            fps: 24.0,
            bit_rate: 1_000_000,
            sound_rate: Some(48_000),
        };
        if !h264_available() {
            assert!(H264Writer::create(&out, o).is_err());
            eprintln!("no system H.264 encoder; skipping");
            return;
        }
        let mut w = H264Writer::create(&out, o).unwrap();
        let sound: Vec<f32> = (0..2000 * 2)
            .map(|i| ((i / 2) as f32 * 0.05).sin() * 0.3)
            .collect();
        for i in 0..12 {
            w.frame(i, &nv12(64, 36, (i * 20) as u8), &sound).unwrap();
        }
        w.finish().unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let has = |what: &[u8]| bytes.windows(what.len()).any(|x| x == what);
        assert!(has(b"ftyp"), "an MP4 file");
        assert!(has(b"avc1"), "with H.264 video");
        assert!(has(b"mp4a"), "with AAC sound");
        if cfg!(windows) {
            assert!(has(b"colrnclx"), "tagged BT.709");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
