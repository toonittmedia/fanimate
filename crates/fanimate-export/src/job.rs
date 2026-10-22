//! What to export: the format, picture size and frames, and what each
//! format needs to be written.

use std::path::{Path, PathBuf};

use fanimate_core::{Document, Frame};
use serde::{Deserialize, Serialize};

/// An export format. Saved by name (`"mp4"`, `"exr"`), so jobs and settings
/// from earlier versions keep their meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// H.264 video in an MP4 file, with AAC sound. Plays almost anywhere.
    #[default]
    Mp4,
    /// H.264 video in a Matroska file, with AAC sound.
    Mkv,
    /// H.265 (HEVC) video in an MP4 file, with AAC sound.
    Hevc,
    /// VP9 video in a WebM file, with Opus sound.
    Webm,
    /// AV1 video in an MP4 file, without sound. Royalty-free.
    Av1,
    /// Apple ProRes 4444 in a QuickTime file: keeps transparency.
    #[serde(rename = "prores4444")]
    ProRes4444,
    /// Apple ProRes 422 HQ in a QuickTime file.
    #[serde(rename = "prores422")]
    ProRes422,
    /// Avid DNxHR HQ in a QuickTime file.
    Dnxhr,
    /// Uncompressed video in an AVI file, with sound and transparency.
    Avi,
    /// An animated GIF, looping, without sound.
    Gif,
    /// An animated PNG, looping, with transparency.
    Apng,
    /// An animated WebP, looping, lossless, with transparency.
    Webp,
    /// One PNG file per frame, with transparency.
    Png,
    /// One OpenEXR file per frame: linear light, transparency, for
    /// compositing.
    Exr,
    /// One TIFF file per frame, with transparency.
    Tiff,
}

/// What writes a format's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// Fanimate itself.
    BuiltIn,
    /// The operating system's video encoder (Windows, macOS).
    System,
    /// The FFmpeg program the user has, with one of these encoders.
    Ffmpeg(&'static [&'static str]),
}

impl Format {
    pub const ALL: [Format; 15] = [
        Format::Mp4,
        Format::Mkv,
        Format::Hevc,
        Format::Webm,
        Format::Av1,
        Format::ProRes4444,
        Format::ProRes422,
        Format::Dnxhr,
        Format::Avi,
        Format::Gif,
        Format::Apng,
        Format::Webp,
        Format::Png,
        Format::Exr,
        Format::Tiff,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp4 => "MP4 video (H.264)",
            Format::Mkv => "MKV video (H.264)",
            Format::Hevc => "MP4 video (H.265)",
            Format::Webm => "WebM video (VP9)",
            Format::Av1 => "MP4 video (AV1, no sound)",
            Format::ProRes4444 => "ProRes 4444 (MOV, transparency)",
            Format::ProRes422 => "ProRes 422 HQ (MOV)",
            Format::Dnxhr => "DNxHR HQ (MOV)",
            Format::Avi => "AVI, uncompressed",
            Format::Gif => "Animated GIF",
            Format::Apng => "Animated PNG",
            Format::Webp => "Animated WebP",
            Format::Png => "PNG sequence",
            Format::Exr => "OpenEXR sequence",
            Format::Tiff => "TIFF sequence",
        }
    }

    /// One line on what the format is for, for the Export Video window.
    pub fn hint(self) -> &'static str {
        match self {
            Format::Mp4 => "Plays almost anywhere: the web, phones, editors.",
            Format::Mkv => "H.264 in a Matroska file, for players and editors that take MKV.",
            Format::Hevc => "Smaller files than H.264 at the same quality; newer players.",
            Format::Webm => "For the web; plays in every browser.",
            Format::Av1 => "Royalty-free and small; browsers and recent players.",
            Format::ProRes4444 => "For editing and compositing, with transparency.",
            Format::ProRes422 => "For editing in Final Cut, Premiere and Resolve.",
            Format::Dnxhr => "For editing in Avid, Premiere and Resolve.",
            Format::Avi => "No compression: large files any editor opens.",
            Format::Gif => "Loops on any web page; 256 colours, no sound.",
            Format::Apng => "Loops on web pages with full colour and transparency.",
            Format::Webp => "Smaller than GIF and APNG, with transparency.",
            Format::Png => "A picture per frame, with transparency.",
            Format::Exr => "A linear-light picture per frame, for compositing.",
            Format::Tiff => "A picture per frame, for print and older tools.",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Mp4 | Format::Hevc | Format::Av1 => "mp4",
            Format::Mkv => "mkv",
            Format::Webm => "webm",
            Format::ProRes4444 | Format::ProRes422 | Format::Dnxhr => "mov",
            Format::Avi => "avi",
            Format::Gif => "gif",
            Format::Apng | Format::Png => "png",
            Format::Webp => "webp",
            Format::Exr => "exr",
            Format::Tiff => "tif",
        }
    }

    /// The ways the format can be written, best first.
    pub fn writers(self) -> &'static [Writer] {
        use Writer::*;
        match self {
            Format::Mp4 => &[
                System,
                Ffmpeg(&[
                    "libx264",
                    "libopenh264",
                    "h264_videotoolbox",
                    "h264_mf",
                    "h264_nvenc",
                    "h264_qsv",
                    "h264_amf",
                ]),
            ],
            Format::Mkv => &[Ffmpeg(&[
                "libx264",
                "libopenh264",
                "h264_videotoolbox",
                "h264_mf",
                "h264_nvenc",
                "h264_qsv",
                "h264_amf",
            ])],
            Format::Hevc => &[Ffmpeg(&[
                "libx265",
                "hevc_videotoolbox",
                "hevc_mf",
                "hevc_nvenc",
                "hevc_qsv",
                "hevc_amf",
            ])],
            Format::Webm => &[Ffmpeg(&["libvpx-vp9"])],
            Format::ProRes4444 | Format::ProRes422 => &[Ffmpeg(&["prores_ks"])],
            Format::Dnxhr => &[Ffmpeg(&["dnxhd"])],
            Format::Webp => &[Ffmpeg(&["libwebp_anim"])],
            Format::Av1
            | Format::Avi
            | Format::Gif
            | Format::Apng
            | Format::Png
            | Format::Exr
            | Format::Tiff => &[BuiltIn],
        }
    }

    /// Whether the format can only be written by FFmpeg.
    pub fn needs_ffmpeg(self) -> bool {
        self.writers().iter().all(|w| matches!(w, Writer::Ffmpeg(_)))
    }

    /// Whether the format carries the project's sound.
    pub fn has_sound(self) -> bool {
        !matches!(
            self,
            Format::Av1
                | Format::Gif
                | Format::Apng
                | Format::Webp
                | Format::Png
                | Format::Exr
                | Format::Tiff
        )
    }

    /// Whether the format keeps transparency outside the canvas.
    pub fn has_alpha(self) -> bool {
        matches!(
            self,
            Format::ProRes4444
                | Format::Avi
                | Format::Apng
                | Format::Webp
                | Format::Png
                | Format::Exr
                | Format::Tiff
        )
    }

    /// The smallest picture the format's encoder takes: DNxHR's is
    /// 256 x 120.
    pub fn min_size(self) -> (u32, u32) {
        if self == Format::Dnxhr { (256, 120) } else { (2, 2) }
    }

    /// A picture per frame rather than one file.
    pub fn is_sequence(self) -> bool {
        matches!(self, Format::Png | Format::Exr | Format::Tiff)
    }

    /// Video codecs work on 2x2 blocks of colour, so sizes must be even.
    fn needs_even_size(self) -> bool {
        (self.has_sound() && self != Format::Avi) || self == Format::Av1
    }

    /// `path` with this format's extension.
    pub fn output(self, path: &Path) -> PathBuf {
        path.with_extension(self.extension())
    }
}

/// The settings of one export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub format: Format,
    /// Picture size in pixels; the canvas is fitted to it.
    pub width: u32,
    pub height: u32,
    /// First and last frame, inclusive, in the document's own numbering
    /// (its first frame is `doc.start`, 1 for a new project; 0 is the rest pose).
    pub start: Frame,
    pub end: Frame,
    /// The FFmpeg program chosen in Preferences, for formats that need it;
    /// `None` looks for one (see [`crate::ffmpeg::find`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ffmpeg: Option<PathBuf>,
    /// Replace a file that is already there. Off, an export that would
    /// replace one stops before rendering. Jobs queued before this setting
    /// existed replaced files, so a missing value means yes.
    #[serde(default = "replace_by_default", skip_serializing_if = "is_replace")]
    pub overwrite: bool,
}

fn replace_by_default() -> bool {
    true
}

fn is_replace(overwrite: &bool) -> bool {
    *overwrite
}

impl Settings {
    /// The whole animation at the project's own size.
    pub fn for_doc(doc: &Document, format: Format) -> Self {
        Self {
            format,
            width: doc.width.round().max(1.0) as u32,
            height: doc.height.round().max(1.0) as u32,
            start: doc.start,
            end: doc.end,
            ffmpeg: None,
            overwrite: true,
        }
    }

    pub fn frame_count(&self) -> u32 {
        (self.end - self.start + 1).max(0) as u32
    }

    /// Size and range made usable: video encoders need even sizes, and an
    /// empty or reversed range exports one frame.
    pub fn sanitized(&self) -> Self {
        let even = |v: u32| {
            let v = v.clamp(2, 16384);
            if self.format.needs_even_size() {
                v + v % 2
            } else {
                v
            }
        };
        Self {
            format: self.format,
            width: even(self.width),
            height: even(self.height),
            start: self.start.max(0),
            end: self.end.max(self.start.max(0)),
            ffmpeg: self.ffmpeg.clone(),
            overwrite: self.overwrite,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_sizes_become_even_and_ranges_usable() {
        let s = Settings {
            format: Format::Mp4,
            width: 641,
            height: 0,
            start: 5,
            end: 2,
            ffmpeg: None,
            overwrite: true,
        }
        .sanitized();
        assert_eq!((s.width, s.height, s.start, s.end), (642, 2, 5, 5));
        assert_eq!(s.frame_count(), 1);
        for format in [Format::Png, Format::Avi, Format::Exr] {
            let odd = Settings {
                format,
                width: 641,
                ..s.clone()
            };
            assert_eq!(odd.sanitized().width, 641, "{format:?}");
        }
    }

    #[test]
    fn formats_name_their_files() {
        assert_eq!(
            Format::Mkv.output(Path::new("a/scene.mp4")),
            Path::new("a/scene.mkv")
        );
        assert_eq!(Format::Tiff.extension(), "tif");
        assert!(Format::ProRes4444.needs_ffmpeg() && !Format::Gif.needs_ffmpeg());
        // MP4 has a writer of its own before FFmpeg
        assert!(!Format::Mp4.needs_ffmpeg());
        assert_eq!(serde_json::to_string(&Format::Gif).unwrap(), "\"gif\"");
        assert_eq!(
            serde_json::to_string(&Format::ProRes4444).unwrap(),
            "\"prores4444\""
        );
        // every format round-trips by name, and has words for the window
        for f in Format::ALL {
            let json = serde_json::to_string(&f).unwrap();
            assert_eq!(serde_json::from_str::<Format>(&json).unwrap(), f);
            assert!(!f.label().is_empty() && !f.hint().is_empty());
        }
    }

    #[test]
    fn settings_from_before_the_ffmpeg_choice_still_load() {
        let old = r#"{"format":"mkv","width":640,"height":360,"start":0,"end":9}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!((s.format, s.ffmpeg), (Format::Mkv, None));
    }
}
