//! The Fanimate desktop app.
//!
//! ```text
//! fanimate                       open the app (what opens first is a preference)
//! fanimate scene.fnm other.fnm   open projects, each in a tab
//! fanimate --bench [--bones N]   play the benchmark rig for 10 s, print fps
//!   --no-vsync                   present as fast as possible
//! fanimate --render-queue        the render queue window (the editor opens it)
//!   --theme <id>                 in this theme
//! ```
//!
//! On Windows this is a GUI program, so opening it shows no console window.
//! Output (the benchmark result, `--help`) still prints when it is run from
//! a terminal or with its output redirected, and appears in a message box
//! when there is nowhere to print.

// no console window on Windows, in any build
#![windows_subsystem = "windows"]

mod app;
mod audio;
mod canvas_gpu;
mod crash;
mod dialogs;
mod docs;
mod doctabs;
mod export;
mod formats;
mod home;
mod icons;
mod instance;
mod kit;
mod layout;
mod links;
mod lipsync;
mod markdown;
mod menus;
mod panels;
mod places;
mod preview;
mod recovery;
mod render_queue;
mod saveas;
mod taskbar;
mod theme;
mod thumbs;
mod titlebar;
mod update;
mod whatsnew;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};

/// Print `text`, or show it in a message box when there is no stdout (a GUI
/// program started from Explorer or a shortcut).
pub fn report(text: &str) {
    if has_stdout() {
        println!("{text}");
    } else {
        rfd::MessageDialog::new()
            .set_title("Fanimate")
            .set_description(text)
            .set_level(rfd::MessageLevel::Info)
            .show();
    }
}

#[cfg(windows)]
fn has_stdout() -> bool {
    use std::os::windows::io::AsRawHandle;
    // the standard library reports a missing handle as null
    !std::io::stdout().as_raw_handle().is_null()
}

#[cfg(not(windows))]
fn has_stdout() -> bool {
    true
}

struct Args {
    files: Vec<PathBuf>,
    bench_bones: Option<usize>,
    vsync: bool,
    render_queue: bool,
    theme: Option<theme::Theme>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut out = Args {
        files: Vec::new(),
        bench_bones: None,
        vsync: true,
        render_queue: false,
        theme: None,
    };
    let mut bench = false;
    let mut bones = 40;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bench" => bench = true,
            "--no-vsync" => out.vsync = false,
            "--render-queue" => out.render_queue = true,
            "--theme" => {
                let id = args.next().ok_or("--theme needs a theme")?;
                out.theme = Some(theme::Theme::from_id(&id).ok_or(format!("unknown theme {id}"))?);
            }
            "--bones" => {
                bones = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or("--bones needs a number")?;
            }
            "-V" | "--version" => {
                report(&format!("fanimate {}", env!("CARGO_PKG_VERSION")));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                report("usage: fanimate [scene.fnm ...] [--bench [--bones N]] [--no-vsync]");
                std::process::exit(0);
            }
            other if other.starts_with('-') => return Err(format!("unknown option {other}")),
            other => out.files.push(PathBuf::from(other)),
        }
    }
    if bench {
        out.bench_bones = Some(bones);
    }
    Ok(out)
}

/// The GPU setup for every window: the renderer's backend choice (DX12
/// first on Windows), never eframe's default, which loads Vulkan and so
/// crashes the process on some Intel drivers.
pub fn wgpu_options() -> WgpuConfiguration {
    let mut setup = WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = fanimate_render::backends();
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(setup),
        ..Default::default()
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            report(&format!("fanimate: {e}"));
            std::process::exit(2);
        }
    };
    crash::install_hook();
    if args.render_queue {
        return render_queue::run(args.theme.unwrap_or_default());
    }

    // one Fanimate: a second launch (opening a .fnm from the file manager)
    // hands its files to the running one, which opens them as tabs
    let claim = if args.bench_bones.is_some() {
        None
    } else {
        match instance::claim(&args.files) {
            instance::Role::Primary(claim) => Some(claim),
            instance::Role::Forwarded => return Ok(()),
            instance::Role::Alone => None,
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Fanimate")
        .with_app_id("fanimate")
        // fits a 1366x768 laptop; starts maximised, later runs restore the
        // last window size and position
        .with_inner_size([1280.0, 720.0])
        .with_min_inner_size([900.0, 560.0])
        .with_maximized(true);
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../brand/app-icon/fanimate-256.png"))
    {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    viewport = if titlebar::uses_custom_frame() {
        // the menus live in our own title bar
        viewport.with_decorations(false)
    } else {
        // macOS: keep the traffic lights, draw under the title bar
        viewport
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false)
    };

    let mut wgpu_options = wgpu_options();
    if !args.vsync {
        wgpu_options.surface.present_mode = wgpu::PresentMode::AutoNoVsync;
    }

    let options = eframe::NativeOptions {
        viewport,
        wgpu_options,
        persist_window: args.bench_bones.is_none(),
        ..Default::default()
    };
    let opts = app::Options {
        files: args.files,
        bench_bones: args.bench_bones,
        claim,
    };
    eframe::run_native(
        "Fanimate",
        options,
        Box::new(|cc| Ok(Box::new(app::FanimateApp::new(cc, opts)))),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// UI rule from CLAUDE.md: never use em dashes in UI text. Every UI and
    /// CLI string lives in Rust source, so scan all crates.
    #[test]
    fn no_em_dashes_in_source() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates dir");
        let mut offenders = Vec::new();
        scan(crates, &mut offenders);
        assert!(
            offenders.is_empty(),
            "em dash (U+2014) found; use a colon, comma or hyphen:\n{}",
            offenders.join("\n")
        );
    }

    fn scan(dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                for (n, line) in text.lines().enumerate() {
                    if line.contains('\u{2014}') {
                        out.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
    }
}
