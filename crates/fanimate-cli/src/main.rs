//! Headless Fanimate tool. Runs the same engine and renderer as the app
//! with no window, for render servers, CI and batch work.

mod script;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fanimate_core::eval::Evaluator;
use fanimate_core::{Document, samples};
use fanimate_render::{Gpu, Mesh, Offscreen, Tessellator, render_to_rgba};

const USAGE: &str = "\
usage:
  fanimate-cli sample <hello|benchmark|episode|text|template> <out.fnm> [--bones N] [--seconds S] [--chars N]
                      (a template is its menu name in lower case with dashes: bouncing-ball,
                       scene-dressing-room, ...)
  fanimate-cli validate <project.fnm> [--repair]
  fanimate-cli import <file.moho|.mohoproj|.fla|.xfl> -o <out.fnm>
  fanimate-cli corpus <folder>
  fanimate-cli render <project.fnm|file.moho|.fla|.xfl> -o <out.png|out.svg> [--frame N] [--width W] [--height H]
                      (an .svg is vector and ignores --width and --height; it lists what SVG cannot carry)
  fanimate-cli export <project.fnm> -o <out.mp4|.mov|.avi|.gif|.png|.exr|...> [--format F]
                      [--width W] [--height H] [--start N] [--end N] [--ffmpeg PATH]
                      (-o out.json is a Lottie and -o out.lottie a dotLottie: the animation as vectors baked to keys,
                       with --start and --end; it lists what Lottie cannot carry)
  fanimate-cli sheet <project.fnm> -o <sheet.png> [--clips walk=1-24,idle=25-48] [--width W] [--height H]
                      [--padding N] [--max-width N] [--no-trim]
                      (a sprite sheet for games: the frames of the clips packed into one PNG, and
                       <sheet>.json beside it, a TexturePacker JSON-hash atlas with a clips list)
  fanimate-cli formats [--ffmpeg PATH]
  fanimate-cli script [<project.fnm>] [--save <out.fnm>]
                      (the scripting API over stdin and stdout: one JSON request per line,
                       one JSON answer per line; the method api.methods lists the calls)
  fanimate-cli serve [<project.fnm>] [--save <out.fnm>] [--port N]
                      (the same calls over a local socket: prints one line of JSON with the port and
                       a token; the first request on a connection must be auth with that token)
  fanimate-cli bench [--bones N] [--frames N] [--seconds S] [--text CHARS] [--actions] [--gpu [--width W] [--height H]]

Frame numbers (--frame, --start, --end) are the project's own: its first frame is 1 and
frame 0 is the rest pose. Without them, render shows the first frame and export the whole
animation.";

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("sample") => sample(&args[1..]),
        Some("validate") => validate(&args[1..]),
        Some("import") => import(&args[1..]),
        Some("corpus") => corpus(&args[1..]),
        Some("render") => render(&args[1..]),
        Some("export") => export(&args[1..]),
        Some("sheet") => sheet(&args[1..]),
        Some("formats") => formats(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("script") => script_cmd(&args[1..]),
        Some("serve") => serve_cmd(&args[1..]),
        Some("-V" | "--version") => {
            println!("fanimate-cli {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("-h" | "--help") | None => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(format!("unknown command {other}\n{USAGE}")),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("fanimate-cli: {e}");
            ExitCode::from(2)
        }
    }
}

fn serve_cmd(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--save", "--port"])?;
    let project = a.positional.first().map(String::as_str);
    let editor = script::open(project)?;
    let save_to = a.get::<String>("--save")?.or_else(|| project.map(str::to_owned));
    let port = a.get::<u16>("--port")?.unwrap_or(0);
    script::serve_socket(editor, save_to.as_deref(), port, std::io::stdout().lock())?;
    Ok(ExitCode::SUCCESS)
}

fn script_cmd(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--save"])?;
    let project = a.positional.first().map(String::as_str);
    let editor = script::open(project)?;
    let save_to = a.get::<String>("--save")?.or_else(|| project.map(str::to_owned));
    let mut editor = editor;
    script::serve(
        &mut editor,
        save_to.as_deref(),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
    .map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

type CmdResult = Result<ExitCode, String>;

/// Split arguments into positionals and `--flag value` pairs.
struct Args {
    positional: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(args: &[String], takes_value: &[&str]) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a.starts_with('-') {
                let value = if takes_value.contains(&a.as_str()) {
                    Some(it.next().ok_or(format!("{a} needs a value"))?.clone())
                } else {
                    None
                };
                flags.push((a.clone(), value));
            } else {
                positional.push(a.clone());
            }
        }
        Ok(Self { positional, flags })
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(f, _)| f == name)
    }

    fn get<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        match self.flags.iter().find(|(f, _)| f == name) {
            Some((_, Some(v))) => v
                .parse()
                .map(Some)
                .map_err(|_| format!("bad value for {name}: {v}")),
            _ => Ok(None),
        }
    }
}

fn sample(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["--bones", "--seconds", "--chars"])?;
    let [kind, out] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let doc = match kind.as_str() {
        "hello" => samples::hello(),
        "benchmark" => samples::benchmark(a.get("--bones")?.unwrap_or(40)),
        // a title of animating text, to look at in the app
        "text" => samples::text_benchmark(a.get("--chars")?.unwrap_or(120)),
        "episode" => samples::episode(
            a.get("--bones")?.unwrap_or(40),
            a.get("--seconds")?.unwrap_or(300),
        ),
        other => match fanimate_editor::Sample::ALL
            .iter()
            .find(|s| slug(s.name()) == other)
        {
            Some(template) => template.document(),
            None => {
                return Err(format!(
                    "unknown sample {other}; use hello, benchmark, episode, text or a template"
                ));
            }
        },
    };
    fanimate_fnm::save(&doc, out).map_err(|e| e.to_string())?;
    println!("wrote {out}");
    Ok(ExitCode::SUCCESS)
}

/// A template's name as typed on the command line: "Scene: Dressing room" is `scene-dressing-room`.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

fn validate(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &[])?;
    let [path] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let (doc, report) = if a.has("--repair") {
        let (doc, report) = fanimate_fnm::load_and_repair(path).map_err(|e| e.to_string())?;
        if report.repaired_count() > 0 {
            fanimate_fnm::save(&doc, path).map_err(|e| e.to_string())?;
        }
        (doc, report)
    } else {
        fanimate_fnm::load(path).map_err(|e| e.to_string())?
    };
    print!("{report}");
    if report.is_clean() {
        println!();
    }
    println!(
        "{}: {} layers, {} issue(s), {} repaired",
        doc.name,
        doc.layer_count(),
        report.issues.len(),
        report.repaired_count()
    );
    Ok(if report.unresolved_errors().next().is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn render(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["-o", "--frame", "--width", "--height"])?;
    let [path] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let out: PathBuf = a.get::<String>("-o")?.ok_or("render needs -o <out.png>")?.into();
    let doc = if fanimate_import::can_import(path.as_ref()) {
        let im = fanimate_import::import(path.as_ref())?;
        for note in &im.notes {
            eprintln!("import: {note}");
        }
        im.doc
    } else {
        let (doc, report) = fanimate_fnm::load(path).map_err(|e| e.to_string())?;
        if !report.is_clean() {
            eprint!("{report}");
        }
        doc
    };
    let frame: i32 = a.get("--frame")?.unwrap_or(doc.start);
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        let (svg, report) = fanimate_export::svg::frame(&doc, frame as f32);
        std::fs::write(&out, svg).map_err(|e| format!("{}: {e}", out.display()))?;
        println!("wrote frame {frame} as SVG to {}", out.display());
        for line in report.lines() {
            println!("  {line}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    let (width, height) = output_size(&doc, a.get("--width")?, a.get("--height")?);

    let started = Instant::now();
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let pixels = render_to_rgba(&gpu, &doc, frame as f32, width, height).map_err(|e| e.to_string())?;
    image::save_buffer(&out, &pixels, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("{}: {e}", out.display()))?;
    let info = gpu.adapter.get_info();
    println!(
        "rendered frame {frame} at {width}x{height} to {} in {:.0} ms on {} ({:?})",
        out.display(),
        started.elapsed().as_secs_f64() * 1000.0,
        info.name,
        info.backend
    );
    Ok(ExitCode::SUCCESS)
}

/// Convert a Moho or Animate project to a Fanimate project, listing what
/// did not come through exactly.
fn import(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &["-o"])?;
    let [path] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let out: PathBuf = a.get::<String>("-o")?.ok_or("import needs -o <out.fnm>")?.into();
    let mut im = fanimate_import::import(path.as_ref())?;
    let report = fanimate_fnm::repair(&mut im.doc);
    if !report.is_clean() {
        eprint!("{report}");
    }
    fanimate_fnm::save(&im.doc, &out).map_err(|e| e.to_string())?;
    println!(
        "imported {} layers, {} frames at {} fps, {}x{} to {}",
        im.doc.layer_count(),
        im.doc.frame_count(),
        im.doc.fps,
        im.doc.width,
        im.doc.height,
        out.display()
    );
    for note in &im.notes {
        println!("  {note}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Import every Moho and Animate file under a folder and report the
/// result of each, for measuring the importers against the test corpus
/// (corpus/README.md). A file "imports cleanly" when it imports with no
/// error and passes the validator with nothing left out; comparing renders
/// with the original program's is a separate, manual step.
fn corpus(args: &[String]) -> CmdResult {
    let a = Args::parse(args, &[])?;
    let [folder] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let mut files = Vec::new();
    collect_importable(std::path::Path::new(folder), &mut files);
    files.sort();
    if files.is_empty() {
        return Err(format!("no Moho or Animate files under {folder}"));
    }
    let (mut clean, mut partial, mut failed) = (0, 0, 0);
    for file in &files {
        let started = Instant::now();
        // an importer bug must not stop the run: count it as a failure
        let result = std::panic::catch_unwind(|| fanimate_import::import(file))
            .unwrap_or_else(|_| Err("the importer crashed".into()));
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(mut im) => {
                let report = fanimate_fnm::repair(&mut im.doc);
                let errors = report.unresolved_errors().count();
                if errors == 0 && im.notes.is_empty() {
                    clean += 1;
                    println!("clean    {} ({ms:.0} ms)", file.display());
                } else {
                    partial += 1;
                    println!("partial  {} ({ms:.0} ms)", file.display());
                    for note in &im.notes {
                        println!("           {note}");
                    }
                    if errors > 0 {
                        println!("           {errors} validator errors");
                    }
                }
            }
            Err(e) => {
                failed += 1;
                println!("failed   {}: {e}", file.display());
            }
        }
    }
    let total = files.len();
    let pct = |n: usize| n as f64 * 100.0 / total as f64;
    println!(
        "
{total} files: {clean} clean ({:.0}%), {partial} partial ({:.0}%), {failed} failed ({:.0}%)",
        pct(clean),
        pct(partial),
        pct(failed)
    );
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn collect_importable(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_importable(&path, out);
        } else if fanimate_import::can_import(&path) {
            out.push(path);
        }
    }
}

/// Export frames as video (through FFmpeg), an animated GIF or a PNG
/// sequence, chosen by the output's extension. The same code as the app's
/// render queue.
/// Export `path` as a Lottie (`dot` false) or a dotLottie, listing what it could not carry.
fn export_lottie(a: &Args, path: &str, out: &Path, dot: bool) -> CmdResult {
    let (doc, report) = fanimate_fnm::load(path).map_err(|e| e.to_string())?;
    if !report.is_clean() {
        eprint!("{report}");
    }
    let range = Some((
        a.get("--start")?.unwrap_or(doc.start),
        a.get("--end")?.unwrap_or(doc.end),
    ));
    let started = Instant::now();
    let (bytes, notes, keys) = if dot {
        let (bytes, notes) = fanimate_export::lottie::dotlottie(&doc, range)?;
        (bytes, notes, None)
    } else {
        let l = fanimate_export::lottie::lottie(&doc, range)?;
        let bytes = serde_json::to_vec(&l.json).map_err(|e| e.to_string())?;
        (bytes, l.report, Some(l.keys))
    };
    std::fs::write(out, &bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "wrote {} ({} KB{}) in {:.0} ms",
        out.display(),
        bytes.len().div_ceil(1024),
        keys.map_or(String::new(), |k| format!(", {k} keys")),
        started.elapsed().as_secs_f64() * 1000.0
    );
    for line in notes.lines() {
        println!("  {line}");
    }
    Ok(ExitCode::SUCCESS)
}

/// Pack the frames of some clips into a sprite sheet and its atlas.
fn sheet(args: &[String]) -> CmdResult {
    use fanimate_export::sheet::{self, Clip, Options};
    let a = Args::parse(
        args,
        &["-o", "--clips", "--width", "--height", "--padding", "--max-width"],
    )?;
    let [path] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let out: PathBuf = a.get::<String>("-o")?.ok_or("sheet needs -o <sheet.png>")?.into();
    let (doc, report) = fanimate_fnm::load(path).map_err(|e| e.to_string())?;
    if !report.is_clean() {
        eprint!("{report}");
    }
    let clips = match a.get::<String>("--clips")? {
        Some(text) => sheet::parse_clips(&text)?,
        None => vec![Clip {
            name: "all".to_owned(),
            start: doc.start,
            end: doc.end,
        }],
    };
    let defaults = Options::default();
    let (width, height) = output_size(&doc, a.get("--width")?, a.get("--height")?);
    let opts = Options {
        width,
        height,
        padding: a.get("--padding")?.unwrap_or(defaults.padding),
        max_width: a.get("--max-width")?.unwrap_or(defaults.max_width),
        trim: !a.has("--no-trim"),
    };
    let image_name = out
        .file_name()
        .map_or_else(|| "sheet.png".to_owned(), |n| n.to_string_lossy().into_owned());
    let started = Instant::now();
    let made = sheet::render(&doc, &clips, &opts, &image_name)?;
    image::save_buffer(
        &out,
        &made.rgba,
        made.width,
        made.height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| format!("{}: {e}", out.display()))?;
    let atlas = out.with_extension("json");
    let text = serde_json::to_string_pretty(&made.atlas).map_err(|e| e.to_string())?;
    std::fs::write(&atlas, text).map_err(|e| format!("{}: {e}", atlas.display()))?;
    println!(
        "wrote {} ({}x{}) and {} in {:.0} ms",
        out.display(),
        made.width,
        made.height,
        atlas.display(),
        started.elapsed().as_secs_f64() * 1000.0
    );
    for line in made.report.lines() {
        println!("  {line}");
    }
    Ok(ExitCode::SUCCESS)
}

fn export(args: &[String]) -> CmdResult {
    use fanimate_export::{Format, Settings};
    let a = Args::parse(
        args,
        &[
            "-o", "--width", "--height", "--start", "--end", "--format", "--ffmpeg",
        ],
    )?;
    let [path] = a.positional.as_slice() else {
        return Err(USAGE.into());
    };
    let out: PathBuf = a.get::<String>("-o")?.ok_or("export needs -o <out>")?.into();
    // Lottie is vectors baked to keys, not rendered frames: .json is a Lottie, .lottie a dotLottie
    let ext = out
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if a.get::<String>("--format")?.is_none() && (ext == "json" || ext == "lottie") {
        return export_lottie(&a, path, &out, ext == "lottie");
    }
    let format = match a.get::<String>("--format")? {
        Some(name) => parse_format(&name)?,
        None => {
            // the extension names the format; .mp4 is H.264, .mov ProRes 4444,
            // .png a PNG sequence unless --format says otherwise
            let ext = out
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let ext = if ext == "tiff" { "tif".to_owned() } else { ext };
            Format::ALL
                .into_iter()
                .find(|f| f.extension() == ext)
                .ok_or("unknown output type; give --format (see `fanimate-cli formats`)")?
        }
    };
    let (doc, report) = fanimate_fnm::load(path).map_err(|e| e.to_string())?;
    if !report.is_clean() {
        eprint!("{report}");
    }
    let (width, height) = output_size(&doc, a.get("--width")?, a.get("--height")?);
    let settings = Settings {
        format,
        width,
        height,
        start: a.get("--start")?.unwrap_or(doc.start),
        end: a.get("--end")?.unwrap_or(doc.end),
        ffmpeg: a.get::<String>("--ffmpeg")?.map(PathBuf::from),
        overwrite: true,
    };
    let started = Instant::now();
    let mut last = Instant::now();
    fanimate_export::export(&doc, &settings, &out, &mut |done, total| {
        if last.elapsed() > Duration::from_secs(1) {
            eprintln!("frame {done} of {total}");
            last = Instant::now();
        }
        true
    })
    .map_err(|e| e.to_string())?;
    println!(
        "exported {} frames to {} in {:.1} s",
        settings.sanitized().frame_count(),
        out.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(ExitCode::SUCCESS)
}

/// A format by its saved name (`mp4`, `prores4444`, `exr`).
fn parse_format(name: &str) -> Result<fanimate_export::Format, String> {
    serde_json::from_str(&format!("\"{}\"", name.to_ascii_lowercase()))
        .map_err(|_| format!("unknown format {name}; see `fanimate-cli formats`"))
}

/// Every export format, and what would make it on this computer.
fn formats(args: &[String]) -> CmdResult {
    use fanimate_export::ffmpeg::{Ffmpeg, find};
    use fanimate_export::{Format, Route, route};
    let a = Args::parse(args, &["--ffmpeg"])?;
    let chosen = a.get::<String>("--ffmpeg")?.map(PathBuf::from);
    let system = fanimate_oscodec::h264_available();
    let ffmpeg = find(chosen.as_deref()).and_then(|p| Ffmpeg::probe(&p).ok());
    match &ffmpeg {
        Some(f) => println!("FFmpeg {} at {}", f.version, f.path.display()),
        None => println!("FFmpeg: not found (it adds ProRes, DNxHR, H.265, WebM and more)"),
    }
    println!(
        "System H.264 encoder: {}
",
        if system { "yes" } else { "no" }
    );
    for f in Format::ALL {
        let name = serde_json::to_string(&f).unwrap_or_default().replace('"', "");
        let how = match route(f, system, ffmpeg.as_ref()) {
            Ok(Route::BuiltIn) => "Fanimate".to_owned(),
            Ok(Route::System) => "the system's encoder".to_owned(),
            Ok(Route::Ffmpeg { encoder, .. }) => format!("FFmpeg ({encoder})"),
            Err(e) => format!("not available: {e}"),
        };
        println!("  {name:<11} {:<34} {how}", f.label());
    }
    Ok(ExitCode::SUCCESS)
}

/// Document size by default; one dimension given scales the other to match.
fn output_size(doc: &Document, w: Option<u32>, h: Option<u32>) -> (u32, u32) {
    let aspect = doc.width / doc.height;
    match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, (w as f32 / aspect).round() as u32),
        (None, Some(h)) => ((h as f32 * aspect).round() as u32, h),
        (None, None) => (doc.width.round() as u32, doc.height.round() as u32),
    }
}

/// Gate 1's "no UI action over 100 ms": every editing command, run the way
/// menus, shortcuts and the command palette run it, on a fresh copy of
/// `doc` with every layer selected. Undo and Redo are timed after a real
/// edit. Commands that only open a dialog or move the view belong to the
/// app and are counted, not timed.
fn bench_actions(doc: &Document) {
    use fanimate_editor::{Action, Editor, Outcome};
    const BUDGET_MS: f64 = 100.0;
    let fresh = || {
        let mut e = Editor::new(doc.clone());
        e.select_all();
        e
    };
    let time = |e: &mut Editor, action: Action| {
        let t0 = Instant::now();
        let outcome = e.run(action);
        (t0.elapsed().as_secs_f64() * 1000.0, outcome)
    };
    let mut timed: Vec<(f64, String)> = Vec::new();
    let mut app_only = 0;
    for action in Action::all() {
        let mut e = fresh();
        if matches!(action, Action::Undo | Action::Redo | Action::UndoLayer) {
            e.run(Action::Duplicate);
            if action == Action::Redo {
                e.run(Action::Undo);
            }
        }
        match time(&mut e, action) {
            (_, Outcome::App(_)) => app_only += 1,
            (ms, _) => timed.push((ms, action.label().to_string())),
        }
    }
    timed.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = timed.iter().filter(|(ms, _)| *ms > BUDGET_MS).count();
    println!(
        "  actions      {} timed, {app_only} open a dialog or the view; {over} over {BUDGET_MS} ms",
        timed.len()
    );
    for (ms, label) in timed.iter().take(5) {
        println!("    {ms:>8.2} ms  {label}");
    }
}

/// CPU-side benchmark: evaluate and tessellate every frame, plus file
/// save and load. `--gpu` adds the full render path without a window (see
/// [`bench_gpu`]); `fanimate --bench` measures it in the app, UI included.
fn bench(args: &[String]) -> CmdResult {
    let a = Args::parse(
        args,
        &[
            "--bones",
            "--frames",
            "--width",
            "--height",
            "--seconds",
            "--text",
        ],
    )?;
    let bones: usize = a.get("--bones")?.unwrap_or(40);
    let frames: u32 = a.get("--frames")?.unwrap_or(2000);
    // --seconds 300: the Gate 1 project-open scene, a 5-minute episode
    // --text 1000: the text benchmark, a title of that many animating characters
    let text_chars: Option<usize> = a.get("--text")?;
    let doc = match (text_chars, a.get::<u32>("--seconds")?) {
        (Some(n), _) => samples::text_benchmark(n),
        (None, Some(seconds)) => samples::episode(bones, seconds),
        (None, None) => samples::benchmark(bones),
    };

    let mut evaluator = Evaluator::default();
    let mut tess = Tessellator::new();
    let mut items = Vec::new();
    let mut mesh = Mesh::new();
    let (mut eval_t, mut tess_t) = (0.0, 0.0);
    let mut triangles = 0;
    // text is laid out the first time it is drawn; time that once, apart from the frames
    let layout_ms = text_chars.map(|_| {
        let t0 = Instant::now();
        evaluator.evaluate_into(&doc, doc.start as f32, &mut items);
        t0.elapsed().as_secs_f64() * 1000.0
    });
    for i in 0..frames {
        let frame = doc.start as f32 + (i as f32 * 0.5) % doc.frame_count() as f32;
        let t0 = Instant::now();
        evaluator.evaluate_into(&doc, frame, &mut items);
        let t1 = Instant::now();
        tess.clear(&mut mesh);
        tess.canvas(&doc, &mut mesh);
        tess.draw_items(&items, &mut mesh);
        let t2 = Instant::now();
        eval_t += (t1 - t0).as_secs_f64();
        tess_t += (t2 - t1).as_secs_f64();
        triangles = mesh.indices.len() / 3;
    }
    let per = |t: f64| t * 1000.0 / frames as f64;

    let dir = std::env::temp_dir().join(format!("fanimate-bench-{}", std::process::id()));
    // a project is one file now, and saving one does not make its folder
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("bench.fnm");
    let t0 = Instant::now();
    fanimate_fnm::save(&doc, &path).map_err(|e| e.to_string())?;
    let save_t = t0.elapsed();
    let t0 = Instant::now();
    fanimate_fnm::load(&path).map_err(|e| e.to_string())?;
    let load_t = t0.elapsed();
    let _ = std::fs::remove_dir_all(&dir);

    let total = per(eval_t) + per(tess_t);
    println!(
        "{}: {} shapes, {triangles} triangles, {frames} frames",
        doc.name,
        items.len()
    );
    if let Some(ms) = layout_ms {
        println!("  text layout  {ms:>8.3} ms, once (shaping, line breaking, outlines)");
    }
    println!("  evaluate     {:>8.3} ms/frame", per(eval_t));
    println!("  tessellate   {:>8.3} ms/frame", per(tess_t));
    println!("  CPU total    {:>8.3} ms/frame  (60 fps budget: 16.7 ms)", total);
    println!("  save .fnm    {:>8.1} ms", save_t.as_secs_f64() * 1000.0);
    println!(
        "  load .fnm    {:>8.1} ms (with validation)",
        load_t.as_secs_f64() * 1000.0
    );
    if a.has("--gpu") {
        let size = (
            a.get("--width")?.unwrap_or(1280),
            a.get("--height")?.unwrap_or(800),
        );
        bench_gpu(&doc, frames, size)?;
    }
    if a.has("--actions") {
        bench_actions(&doc);
    }
    if cfg!(debug_assertions) {
        println!("  note: debug build; use --release for real numbers");
    }
    Ok(ExitCode::SUCCESS)
}

/// Longest the GPU part of the benchmark runs, so a slow software renderer
/// on a CI machine does not hold up a release for minutes.
const GPU_BENCH_LIMIT: Duration = Duration::from_secs(10);

/// Evaluate, tessellate and draw frames offscreen with 4x MSAA, as the
/// canvas does, waiting for the GPU after each one. Waiting gives up the
/// overlap a window gets between CPU and GPU work, so the frame rate is a
/// conservative figure for the same machine.
fn bench_gpu(doc: &Document, frames: u32, (width, height): (u32, u32)) -> Result<(), String> {
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut target = Offscreen::new(&gpu, width, height);
    let frame_at = |i: u32| doc.start as f32 + (i as f32 * 0.5) % doc.frame_count() as f32;

    // the first frames include pipeline and buffer setup
    for i in 0..10 {
        target.draw(&gpu, doc, frame_at(i));
    }
    target.wait(&gpu).map_err(|e| e.to_string())?;

    let mut times = Vec::with_capacity(frames as usize);
    let started = Instant::now();
    for i in 0..frames {
        let t0 = Instant::now();
        target.draw(&gpu, doc, frame_at(i));
        target.wait(&gpu).map_err(|e| e.to_string())?;
        times.push(t0.elapsed());
        if started.elapsed() > GPU_BENCH_LIMIT {
            break;
        }
    }
    let total = started.elapsed().as_secs_f64();
    times.sort_unstable();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];

    let info = gpu.adapter.get_info();
    println!(
        "  GPU render   {:>8.1} fps at {width}x{height}, 4x MSAA ({} frames, {} triangles)",
        times.len() as f64 / total,
        times.len(),
        target.triangles()
    );
    println!(
        "  frame time   {:>8.3} ms median, {:.3} ms 95th percentile, {:.3} ms slowest",
        ms(times[times.len() / 2]),
        ms(p95),
        ms(times[times.len() - 1])
    );
    println!(
        "  adapter      {} ({:?}, {:?})",
        info.name, info.backend, info.device_type
    );
    if gpu.is_software() {
        println!("  note: software renderer; these numbers say nothing about GPU speed");
    }
    Ok(())
}
