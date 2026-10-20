//! FFmpeg: the program a user can add for more video formats.
//!
//! Fanimate does not ship FFmpeg. Formats Fanimate cannot write itself or
//! through the system's encoder (ProRes, DNxHR, H.265, WebM and others,
//! see [`crate::Format::writers`]) use the FFmpeg the user has: the one
//! chosen in Preferences, else one on PATH or in the usual install places.
//! Fanimate talks to it only through its command line and standard input,
//! so FFmpeg's licence never reaches Fanimate's code, and runs it with no
//! console window.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::job::{Format, Settings, Writer};

fn exe_name() -> &'static str {
    if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }
}

/// Where FFmpeg usually lands when installed the standard ways: next to
/// Fanimate, winget, Chocolatey, Scoop, Homebrew, MacPorts, the system.
fn usual_places() -> Vec<PathBuf> {
    let exe = exe_name();
    let mut places = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        places.push(dir.join(exe));
        places.push(dir.join("ffmpeg").join(exe));
        places.push(dir.join("ffmpeg").join("bin").join(exe));
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = env("LOCALAPPDATA") {
            places.push(local.join("Microsoft").join("WinGet").join("Links").join(exe));
        }
        if let Some(data) = env("ProgramData") {
            places.push(data.join("chocolatey").join("bin").join(exe));
        }
        if let Some(home) = env("USERPROFILE") {
            places.push(home.join("scoop").join("shims").join(exe));
        }
        places.push(PathBuf::from(r"C:\ffmpeg\bin").join(exe));
    } else {
        for dir in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/opt/local/bin",
            "/usr/bin",
            "/snap/bin",
        ] {
            places.push(Path::new(dir).join(exe));
        }
    }
    places
}

/// The FFmpeg to use: `chosen` (from Preferences) if it exists, else the
/// first on PATH, else one in the usual places.
pub fn find(chosen: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = chosen {
        return p.is_file().then(|| p.to_path_buf());
    }
    let on_path = std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(exe_name()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    on_path.into_iter().chain(usual_places()).find(|p| p.is_file())
}

/// An FFmpeg program and what it can write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ffmpeg {
    pub path: PathBuf,
    /// As FFmpeg reports it, such as "7.1".
    pub version: String,
    pub encoders: BTreeSet<String>,
}

impl Ffmpeg {
    /// Ask the program at `path` its version and encoders.
    pub fn probe(path: &Path) -> Result<Self, String> {
        let run = |arg: &str| {
            command(path)
                .args(["-hide_banner", arg])
                .output()
                .map_err(|e| format!("{} could not start: {e}", path.display()))
        };
        let version = run("-version")?;
        let first = String::from_utf8_lossy(&version.stdout);
        let first = first.lines().next().unwrap_or_default();
        if !first.starts_with("ffmpeg version") {
            return Err(format!("{} is not FFmpeg", path.display()));
        }
        let encoders = run("-encoders")?;
        Ok(Self {
            path: path.to_path_buf(),
            version: parse_version(first),
            encoders: parse_encoders(&String::from_utf8_lossy(&encoders.stdout)),
        })
    }

    /// The encoder this FFmpeg would use for `format`, if it has one.
    pub fn encoder_for(&self, format: Format) -> Option<&'static str> {
        format.writers().iter().find_map(|w| match w {
            Writer::Ffmpeg(names) => names.iter().copied().find(|n| self.encoders.contains(*n)),
            _ => None,
        })
    }

    /// The formats only FFmpeg writes that this one can.
    pub fn formats(&self) -> Vec<Format> {
        Format::ALL
            .into_iter()
            .filter(|f| f.needs_ffmpeg() && self.encoder_for(*f).is_some())
            .collect()
    }
}

/// "ffmpeg version 7.1-full_build-www.gyan.dev Copyright ..." gives "7.1".
fn parse_version(line: &str) -> String {
    let v = line.split_whitespace().nth(2).unwrap_or("unknown");
    // builds append their name after a dash; keep the release number
    let short: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if short.is_empty() { v.to_owned() } else { short }
}

/// The encoder names in `ffmpeg -encoders` output: the second column of
/// the lines after the dashed rule.
fn parse_encoders(text: &str) -> BTreeSet<String> {
    text.lines()
        .skip_while(|l| !l.trim_start().starts_with("------"))
        .skip(1)
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

/// The H.264 and similar bit rate for encoders without a quality setting:
/// generous, as the animator will often edit the file further.
pub fn bit_rate(settings: &Settings, fps: f32) -> u32 {
    let bits = f64::from(settings.width) * f64::from(settings.height) * f64::from(fps.max(1.0)) * 0.2;
    bits.clamp(1.0e6, 80.0e6) as u32
}

/// FFmpeg's arguments to encode raw RGBA frames from standard input with
/// `encoder`, with the sound in `audio` if any, into `out` (written as
/// `settings.format` whatever its extension).
pub fn video_args(
    settings: &Settings,
    fps: f32,
    encoder: &str,
    audio: Option<&Path>,
    out: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    push(&["-hide_banner", "-loglevel", "error", "-nostdin", "-y"]);
    let size = format!("{}x{}", settings.width, settings.height);
    let rate = format!("{fps}");
    push(&[
        "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size, "-r", &rate, "-i", "-",
    ]);
    let sound = audio.filter(|_| settings.format.has_sound());
    if let Some(audio) = sound {
        push(&["-i"]);
        args.push(audio.as_os_str().to_owned());
    }
    let mut push = |items: &[&str]| args.extend(items.iter().map(OsString::from));
    let bits = format!("{}", bit_rate(settings, fps));
    // hardware and system encoders take NV12; the rest the common 4:2:0
    let hardware = ["_mf", "_qsv", "_videotoolbox"]
        .iter()
        .any(|s| encoder.ends_with(s));
    let four_two_zero = if hardware { "nv12" } else { "yuv420p" };
    match settings.format {
        Format::Mp4 | Format::Mkv | Format::Hevc => {
            push(&["-c:v", encoder]);
            match encoder {
                "libx264" => push(&["-preset", "medium", "-crf", "17"]),
                "libx265" => push(&["-preset", "medium", "-crf", "20"]),
                _ => push(&["-b:v", &bits]),
            }
            push(&["-pix_fmt", four_two_zero]);
            if settings.format == Format::Hevc {
                // the tag Apple's players need to open H.265
                push(&["-tag:v", "hvc1"]);
            }
        }
        Format::Webm => push(&[
            "-c:v", encoder, "-crf", "24", "-b:v", "0", "-row-mt", "1", "-pix_fmt", "yuv420p",
        ]),
        Format::ProRes4444 => push(&[
            "-c:v",
            encoder,
            "-profile:v",
            "4",
            "-vendor",
            "apl0",
            "-pix_fmt",
            "yuva444p10le",
        ]),
        Format::ProRes422 => push(&[
            "-c:v",
            encoder,
            "-profile:v",
            "3",
            "-vendor",
            "apl0",
            "-pix_fmt",
            "yuv422p10le",
        ]),
        Format::Dnxhr => push(&["-c:v", encoder, "-profile:v", "dnxhr_hq", "-pix_fmt", "yuv422p"]),
        Format::Webp => push(&["-c:v", encoder, "-lossless", "1", "-loop", "0"]),
        // written by Fanimate itself; never reaches FFmpeg
        Format::Av1 | Format::Avi | Format::Gif | Format::Apng | Format::Png | Format::Exr | Format::Tiff => {
        }
    }
    if settings.format != Format::Webp {
        // convert and tag as BT.709 like Fanimate's own encoders, not
        // FFmpeg's untagged BT.601 default, so every format looks the same
        push(&[
            "-vf",
            concat!(
                "scale=out_color_matrix=bt709:out_range=tv,",
                "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv"
            ),
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ]);
    }
    if sound.is_some() {
        match settings.format {
            Format::Webm => push(&["-c:a", "libopus", "-b:a", "160k"]),
            // editing formats keep the sound exact
            Format::ProRes4444 | Format::ProRes422 | Format::Dnxhr => push(&["-c:a", "pcm_s16le"]),
            _ => push(&["-c:a", "aac", "-b:a", "192k"]),
        }
    }
    let seconds = format!("{:.3}", settings.frame_count() as f32 / fps.max(1.0));
    push(&["-t", &seconds]);
    match settings.format {
        Format::Mkv => push(&["-f", "matroska"]),
        Format::Webm => push(&["-f", "webm"]),
        Format::ProRes4444 | Format::ProRes422 | Format::Dnxhr => push(&["-f", "mov"]),
        Format::Webp => push(&["-f", "webp"]),
        // start playing before the whole file has arrived
        _ => push(&["-movflags", "+faststart", "-f", "mp4"]),
    }
    args.push(out.as_os_str().to_owned());
    args
}

/// A command for `program` that opens no console window on Windows.
pub fn command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    fn settings(format: Format) -> Settings {
        Settings {
            format,
            width: 640,
            height: 360,
            start: 0,
            end: 47,
            ffmpeg: None,
            overwrite: true,
        }
    }

    #[test]
    fn versions_and_encoders_are_read_from_ffmpegs_output() {
        assert_eq!(
            parse_version("ffmpeg version 7.1-full_build-www.gyan.dev Copyright (c) 2000-2024"),
            "7.1"
        );
        assert_eq!(parse_version("ffmpeg version n6.0 Copyright"), "n6.0");
        let listing = "Encoders:\n V..... = Video\n ------\n V....D libx264              H.264\n V....D prores_ks            Apple ProRes\n A....D aac                  AAC\n";
        let encoders = parse_encoders(listing);
        assert_eq!(
            encoders.into_iter().collect::<Vec<_>>(),
            ["aac", "libx264", "prores_ks"]
        );
    }

    #[test]
    fn each_format_gets_its_encoder_when_ffmpeg_has_one() {
        let ffmpeg = Ffmpeg {
            path: PathBuf::from("ffmpeg"),
            version: "7.1".into(),
            encoders: ["h264_mf", "prores_ks", "aac"].map(String::from).into(),
        };
        assert_eq!(ffmpeg.encoder_for(Format::Mkv), Some("h264_mf"));
        assert_eq!(ffmpeg.encoder_for(Format::ProRes4444), Some("prores_ks"));
        assert_eq!(ffmpeg.encoder_for(Format::Hevc), None);
        assert_eq!(ffmpeg.encoder_for(Format::Gif), None);
        let formats = ffmpeg.formats();
        assert!(formats.contains(&Format::Mkv) && formats.contains(&Format::ProRes422));
        assert!(!formats.contains(&Format::Webm));
    }

    #[test]
    fn arguments_describe_the_frames_codec_and_file() {
        let a = strings(&video_args(
            &settings(Format::Mkv),
            24.0,
            "libx264",
            Some(Path::new("/t/sound.wav")),
            Path::new("/o/x.part"),
        ))
        .join(" ");
        assert!(a.contains("-s 640x360 -r 24 -i -"), "{a}");
        assert!(a.contains("-i /t/sound.wav"), "{a}");
        assert!(
            a.contains("-c:v libx264 -preset medium -crf 17 -pix_fmt yuv420p"),
            "{a}"
        );
        assert!(a.contains("-c:a aac"));
        assert!(
            a.contains("out_color_matrix=bt709") && a.contains("-colorspace bt709"),
            "{a}"
        );
        assert!(a.contains("-t 2.000"));
        assert!(a.ends_with("-f matroska /o/x.part"), "{a}");

        let prores = strings(&video_args(
            &settings(Format::ProRes4444),
            24.0,
            "prores_ks",
            Some(Path::new("s.wav")),
            Path::new("x.mov"),
        ))
        .join(" ");
        assert!(
            prores.contains("-profile:v 4") && prores.contains("yuva444p10le"),
            "{prores}"
        );
        assert!(
            prores.contains("-c:a pcm_s16le") && prores.ends_with("-f mov x.mov"),
            "{prores}"
        );

        // a system encoder takes NV12 and a bit rate; no sound, no audio input
        let mf = strings(&video_args(
            &settings(Format::Mp4),
            24.0,
            "h264_mf",
            None,
            Path::new("x.mp4"),
        ))
        .join(" ");
        assert!(
            mf.contains("-c:v h264_mf -b:v") && mf.contains("-pix_fmt nv12"),
            "{mf}"
        );
        assert!(!mf.contains("-c:a") && mf.contains("+faststart"), "{mf}");

        // a format without sound never takes the sound file
        let webp = strings(&video_args(
            &settings(Format::Webp),
            24.0,
            "libwebp_anim",
            Some(Path::new("s.wav")),
            Path::new("x.webp"),
        ))
        .join(" ");
        assert!(
            !webp.contains("s.wav") && webp.ends_with("-f webp x.webp"),
            "{webp}"
        );
        assert!(!webp.contains("bt709"), "WebP stays RGB: {webp}");
    }

    #[test]
    fn a_chosen_path_that_is_gone_finds_nothing() {
        assert_eq!(find(Some(Path::new("/no/such/ffmpeg"))), None);
    }
}
