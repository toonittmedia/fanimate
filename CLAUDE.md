# CLAUDE.md

Guidance for Claude (and any AI assistant) working in this repository. Read it fully before changing anything. When a rule here conflicts with a habit, the rule wins.

## What Fanimate is

Fanimate is a free, open-source 2D character animation app. It keeps what animators love about **Moho Pro** (bone rigging, Smart Bones, vector puppets) and Moho Pro's screen layout, fixes the problems that push people away from Moho, and adds the best tools of **Toon Boom Harmony** and **Adobe Animate**. One Rust codebase, GPU-rendered with wgpu, native on Windows, macOS and Linux, with a browser build later.

**One-line pitch:** Moho's rigging, Harmony's drawing and pipeline, Animate's ease of use. Free, on every platform, with nothing locked away.

**Where we are:** Phase 1 (Foundation), public alpha. M1 (engine spike) is done on Windows, waiting only for macOS and Linux benchmark numbers on the [reference hardware](docs/reference-hardware.md). M2 (Draw), M3 (Rig), M4 (Frame-by-frame and voice) and M5 (Bring your files) are built: drawing, point and pen tools, booleans, point keyframes, timeline key editing, per-layer undo history, crash recovery, bones with IK, smooth binding, Smart Bones, actions, image layers and PSD import, frame-by-frame layers, onion skin, the X-sheet, switch and audio layers, lip sync, SVG import, video export through the render queue, Moho and Animate importers, installers with a signed self-updater, crash reports and the web build are in; their exit checks with testers remain. M6 (public beta, 1.0) is next. Status lives in the "Status" section of [docs/roadmap/phase-1.md](docs/roadmap/phase-1.md). Update that section when you finish milestone work.

### Read these before non-trivial work

| Document | Why |
|---|---|
| [docs/product-vision.md](docs/product-vision.md) | The why: pain points P1 to P24, principles, feature specs. Every feature should trace back to a pain point. |
| [docs/roadmap/phase-1.md](docs/roadmap/phase-1.md) | What is in and out of scope now, workstreams, milestones, Gate 1 tests, current status |
| [docs/spec/fnm.md](docs/spec/fnm.md) | The project file format. Must match `crates/fanimate-fnm` exactly. |
| [brand/README.md](brand/README.md) | Colours, themes, icon and wordmark rules |
| [CONTRIBUTING.md](CONTRIBUTING.md), [GOVERNANCE.md](GOVERNANCE.md) | Workflow, review standards, release process |

## Non-negotiable rules

1. **Clean room.** Never copy, trace or derive code, icons, artwork or text from Moho, Adobe Animate, Toon Boom Harmony or any closed product. Never use decompiled or leaked material. Follow Moho's *layout and workflow*, never its *look or code*. Importers are written from public documentation and from inspecting files we may use. Screenshots of other apps in `reference/` are for layout reference only and are gitignored: **never commit anything under `reference/`.**
2. **Never lose work.** Saving is atomic (write a temp folder, swap it in). Loading validates and repairs. No panics on user data: bad input becomes a validator issue or an error message. Anything touching save, load or undo needs tests, including the failure path.
3. **Fast by default.** The Gate 1 budget is 60 fps for a 40-bone rig with 10 Smart Bones on a mid-range laptop, project open under 3 s, no UI action over 100 ms. Per-frame code (evaluation, tessellation, rendering) must not allocate needlessly; give before and after numbers when you change it.
4. **Polish before features.** A smaller finished feature beats a larger half-finished one. Unfinished tools are shown but clearly marked with the milestone that brings them; they never pretend to work.
5. **The file format is a public contract.** Any change to what `.fnm` writes needs: an update to `docs/spec/fnm.md`, a version bump if older readers would misread it, a migration on load, validator coverage, and a round-trip test. Files from any released version must always open. The single-file container did not bump the version by itself; layer pivots and rectangles did (version 4), and guides, the camera, locked layers, hidden shapes and colour keys did (version 5). Version 6 added 1-based frames (the migration shifts every frame by one), per-channel loop modes, the layer order channel and the optional thumbnail entry.
6. **Ask before outward-facing actions.** Do not commit, push, tag, create releases or change GitHub settings unless the user asks. Releases go through `pnpm release` only.

## UI and UX rules

Fanimate looks like Moho Pro in **layout** (tools left of the canvas, timeline along the bottom, Style/Actions and Layers on the right, tool names and concepts like bones, Smart Bones, actions, switch layers) but must not copy Moho's **interaction problems**. Users find Moho's layer handling unusual and hard to use. So:

- **Right-click menus everywhere.** Every list row, canvas, timeline row and key area has a context menu. Right-clicking an unselected item selects it first. Open them with `menus::context_menu` (not egui's `Response::context_menu`) so they share the menu bar's style, and fill them with the same `menus::item` helpers, so labels, shortcuts and enabled states always match.
- **Feel native.** Labels are not selectable and show the arrow cursor; text shows an I-beam only where it can be edited. Do not show grab or move cursors on tabs and list rows (`native_cursors` in `app.rs` removes egui's). An edit field asks for focus once when it opens, never every frame, so clicking elsewhere commits it.
- **Standard shortcuts by default, Moho shortcuts on request.** Two keymaps ship (`crates/fanimate-editor/src/keymap.rs`):
  - **Fanimate (default):** Ctrl+C / Ctrl+X / Ctrl+V copy, cut and paste by focus (points and shapes on the canvas, keys in the timeline); **layers use Ctrl+Shift+C / Ctrl+Shift+X / Ctrl+Shift+V**; Ctrl+Z undo; **Ctrl+Y and Ctrl+Shift+Z redo**; Delete deletes keys, points, shapes and bones but **never a layer** (Ctrl+Delete is `Action::DeleteLayers`; a stray Delete or Backspace with only a layer selected just says so); F2 renames; Ctrl+D duplicates; tool letters follow Adobe Animate and Illustrator.
  - **Moho:** Moho Pro's own shortcuts (Ctrl+Shift+Z redo only, M transform layer, Z manipulate bones, T transform points, A add point or bone depending on layer type, Home resets the view).
  Users pick one in the first-run welcome dialog and can switch any time (Edit > Keyboard Shortcuts, Preferences, or the status bar). Never hard-code a shortcut in UI code: add the `Action`, bind it in **both** keymap tables (or deliberately leave it unbound in Moho), and let menus show it via `Keymap::shortcut_text`. The keymap test rejects duplicate chords.
- **Menus live in the title bar.** The window is undecorated on Windows and Linux; `titlebar.rs` draws the menus, the centred document title, window buttons, dragging, double-click to maximise and edge resizing. The menu bar behaves like a desktop one: once a menu is open, hovering another title or pressing Left/Right switches to it, and shortcuts are ignored while any menu is open. On macOS the native traffic lights stay. Do not add a separate menu row.
- **No modal dialogs for properties.** Layer and project settings are edited live in the docked Properties panel (Moho uses blocking dialogs). Reserve modals for decisions (unsaved changes, first run) and the command palette.
- **Standard selection and editing:** click, Ctrl-click, Shift-click ranges; drag and drop to reorder or nest layers; double-click or F2 to rename in place; one click on the eye to show or hide.
- **Every command is an `Action`.** Menus, shortcuts, context menus and the command palette all queue the same `Action` and run through `Editor::run`. Never put editing logic in a widget.
- **One gesture, one undo step.** Call `Editor::checkpoint` once per user action (at drag start, or via `properties::begin_edit` for value widgets), never per frame.
- **Frames are 1-based.** A document starts at frame 1 (`doc.start`, new ones run 1 to 120). Frame 0 is the rest pose: visible and selectable, and the one place the keying rule seeds. Playback, ranges and "Frame N of M" run from `start`; the transport shows seconds as well. Show frame numbers in the monospace font (JetBrains Mono, OFL).
- **Nothing is edited while the animation plays.** An edit at a moving playhead would key a different frame each time. `Editor::run` ignores every `Action` with `edits_document()` while `playing` (and `can_run` greys them), the canvas skips its tools, the editing panels are disabled, and key drags and imports refuse (`Editor::editing_blocked` says why in the status bar). A new command that changes the document must be listed in `Action::edits_document`.
- **Keying rule (Moho style):** on frame 0 an edit changes the value itself (or the key already there); on any later frame it sets a key and seeds one on frame 0 when the channel has none there, so nothing jumps (`Keying`, `Editor::keying()`, `Channel::write`, `Shape::set_path_at`). New keys take `Editor::key_interp`. The camera is the document's (`Document::camera`, three channels, never a layer) and is keyed the same way; fill and stroke colours key through `Shape::fill_keys` and `stroke_keys`. Guides live in the document too.

### Writing UI text

- **Never use em dashes (—) in any UI text**: labels, menus, tooltips, status messages, dialogs, window titles, CLI output. Use a colon, comma, full stop or parentheses instead, or a plain hyphen in window titles ("Scene - Fanimate"). A test in `fanimate-app` fails the build if an em dash appears in Rust source.
- Plain, short, sentence case. Name things the way animators do. Say what will happen ("Delete 3 layers"), not how the code works.
- Use the ellipsis character "…" only on menu items that open a dialog or need more input.
- Status messages are brief and disappear after a few seconds; errors say what went wrong and what to do.

### Brand and theming

- Colours come only from `brand/tokens/tokens.json`. `crates/fanimate-app/src/theme.rs` reads that file at compile time. Do not hard-code colours in UI code; use `shell.palette`.
- **Purple marks interaction, not content:** selection, focus, active tools, primary buttons. Animation state has its own colours: keyframes and the playhead amber (`keyframe`), bones teal (`bone`), errors coral.
- The canvas pasteboard stays neutral; never tint artwork.
- **Layer label colours** (`labels` in the tokens, `fanimate_core::Label`) mark layers in the Layers panel and timeline rows, like compositing apps' label colours. New layers take the next label in turn (`Label::for_id`). They never colour artwork, keyframes or selection.
- Studio Grey is the default theme.
- The Bone-F mark always has its see-through pivot dot. Brand assets are generated by `brand/scripts/build_brand.py`; edit the script or tokens, never the generated files.
- UI font is Inter (bundled, OFL); frame numbers and other figures that must line up use JetBrains Mono (bundled, OFL) as the monospace family. Keep text legible at 100% scaling on a 1366x768 laptop, the smallest screen we design for.
- **Interface icons are [Lucide](https://lucide.dev) SVGs** (ISC licence) in `crates/fanimate-app/icons/lucide/`, rasterised with resvg by `crates/fanimate-app/src/icons.rs`; never font glyphs or emoji (egui's fonts lack many symbols and they render as empty boxes). To add one, download the SVG from the same lucide-static version (see `icons/lucide/README.md`), add a variant to `Icon`, map it in `Icon::svg`, and list it in the icon test. Icons Lucide lacks (bone tools) go in `icons/fanimate/`, drawn on Lucide's 24 px grid with 2 px round strokes. Simple shapes that are not icons, like keyframe diamonds, are painted directly.

## Architecture

```
crates/
  fanimate-core     Scene graph, animation channels, bones, evaluation. No UI, GPU or file IO.
  fanimate-fnm      .fnm reader (single file or folder), atomic writer, validator and repair, the ZIP reader and writer.
  fanimate-render   lyon tessellation + wgpu renderer; headless render to image.
  fanimate-editor   Editing model: Editor (selection, commands, undo), Action, Tool, keymaps, clipboard. No egui.
  fanimate-export   Video, animated image and picture sequence export (built-in writers, the system encoder, or the user's FFmpeg), sound mixing, the render queue folder.
  fanimate-oscodec  The system's H.264 and AAC encoders: Media Foundation (Windows), AVFoundation (macOS). The one crate allowed `unsafe`.
  fanimate-app      The desktop app (eframe/egui + egui_dock): title bar, panels, canvas, dialogs, render queue window.
  fanimate-import   Moho (.moho/.mohoproj) and Adobe Animate (.fla/.xfl) importers (they read ZIP through fanimate-fnm).
  fanimate-cli      Headless tool: sample, validate, import, corpus, render, export, bench.
  fanimate-web      The browser player (WebGPU): opens, plays and checks projects. Empty on other targets.
packaging/          Windows installer (Inno Setup), macOS app and disk image, Linux AppImage, .deb and Flatpak.
```

Dependencies point downwards only: core ← fnm ← editor ← app; core ← render ← app/cli; fnm and render ← export ← app/cli; core ← fnm ← import ← editor/cli/web. **Keep `fanimate-core` free of UI, GPU and platform code** (it runs headless and in the browser), and **keep `fanimate-editor` free of egui** so every command is unit-tested without a window. If logic can live in `fanimate-editor`, it belongs there, with a test.

Key facts:

- **A project is one `.fnm` file** (a ZIP archive of the folder layout; `docs/spec/fnm.md`). `fanimate_fnm::save` writes a file, except where `path` is already a folder, which stays a folder (`save_folder` writes one explicitly: the crash journal and the render queue use it, as they update files in place). A file is written beside the old one, read back and checked, then renamed over it. Loading checks every entry's checksum. Anything that reads or writes projects must accept both forms; the damaged-projects test covers both.
- **Several documents are open as tabs; one is live.** The live document stays in `Shell::editor` (with `Shell::view`, the timeline zoom and the crash journal); the others are parked in `Shell::docs` (`fanimate_editor::Tabs`, tested without a window; `docs.rs` moves state in and out). So code that reads `shell.editor` always means the front document. New, Open and the samples add a tab, never replace the document; only quitting and updating ask about every unsaved document. Each document has its own journal (`recovery::Session`). With the Home tab in front there is no document to act on: `docs::works_on_home` lists the actions that still run, and `menus::item` greys the rest.
- **One Fanimate.** `instance.rs`: opening a `.fnm` from the file manager hands the file to the running Fanimate (a lock file plus an inbox folder) and exits; if nobody answers in 3 seconds it carries on as its own Fanimate, so a hung first instance cannot block a launch. The render queue process and `--bench` never claim it. When testing the app by hand, a second launch opens in the first.
- **Crash recovery depends on `Editor::revision`.** It goes up on every `checkpoint`, undo and redo, and the app journals the document once the revision changes and no gesture is in progress (`fanimate-app/src/recovery.rs`, `fanimate_fnm::journal`). An edit that changes the document without a checkpoint is only journalled by the 10-second backstop, so always checkpoint.
- A brush stroke is an ordinary shape: its path is the centreline and `Shape::widths` the width profile (one value per on-curve point). Anything that adds or removes points must keep `widths` **and `path_keys`** in step (`Shape::remove_points`, `insert_point` and `extend_from` do). Every point key has the same commands as the shape's path; edit points through `Shape::path_at` / `set_path_at` so the keying rule holds.
- **Hidden edges** (`Shape::hidden_edge_flags`, one flag per path command, Hide Edge tool) follow the same rule: code that adds or removes path commands keeps the flags in step (`remove_points`, `insert_point`, `extend_from`, `close_subpath` and Delete Edge do). `Shape::hidden_edges()` returns the flags only while they fit the path, so a missed edit shows the edges rather than hiding the wrong ones. The stroke is drawn from `Path::without_hidden_edges`; the fill uses the whole path.
- **Saved styles** are `fanimate_core::StylePreset`, managed by `fanimate_editor::StyleBook` (the Style panel's one-line control; the logic and its tests live in the editor crate, not the widget). The app keeps the list in `Shell::styles` and settings, copies it into `Document::styles` every frame (not an edit: no undo step, no dirty flag), and merges a project's styles in when it opens (`Shell::open_document`). A style holds the brush look only, never the eraser or weight brush sizes.
- **Undo history is shared per top-level layer** (`fanimate_fnm::HistoryState`): a state reuses the `Arc` of every layer unchanged since the previous one. Keep it that way (capture with `HistoryState::capture` and a previous state), or the journal rewrites every layer on every step. Code that needs a shape's index uses `DrawItem::index`, never pointer comparison: a shape with point keys is drawn from a per-frame copy.

- **Layers turn and scale about a pivot** (`transform.pivot`, an `Option`, not animated): `Transform::to_affine` keeps the pivot where `position + pivot` says. Without one (every file before version 4) a layer turns about its origin, so old projects look the same; the frame shows the pivot at the middle of the box for such a layer, and turning it sets one there without moving it. Moving a pivot always goes through `AnimTransform::set_pivot_keeping_look`, never by assigning the field. Dragging a frame handle writes transforms through `Transform::from_affine`, which refuses a shear; scale, rotate and pivot drags are `Editor::begin_handle`, `drag_handle`, `end_handle`, tested without a window.
- **A rectangle is a path that remembers it was one.** `Shape::rect` holds the box and radius; `Shape::parametric_rect` only believes it while the path still matches (and there is no width profile, point keys or skin), so any tool that edits the path ends it without having to clear anything. Never regenerate a path from `rect` without that check.
- **Bones have parents before children.** `Skeleton` assumes a parent comes earlier in the list, so reparenting goes through `Editor::set_bone_parent`, which refuses loops, keeps the bone where it is in the rest pose, moves it (and its children) after a later parent, and is one undo step. The Reparent Bone tool, the Bones panel tree (drag a row, Parent To, Make Root Bone) and the Parent menu all use it; `bone_rows` gives the tree order. Never assign `Bone::parent` directly.
- **Posing a bone always keys it.** A bone angle's `base` is the rest pose that bound artwork is measured from, so Manipulate Bones sets keys even on unkeyed bones (the one exception to the keying rule); Select Bone edits the rest pose.
- **Actions are edited in place.** `Document::enter_action` swaps an action's keys into the bone layer and `action_edit` holds the timeline's keys aside; anything written to disk goes through `Document::mainline()` (`fanimate_fnm::save` and the journal already do). Smart Bones are not applied to the layer whose action is open.
- **Pictures and sounds** live in `Document::media` by content-derived name (`MediaFile::name_for`) and on disk in the project's `media/` folder. Only names that pass `fanimate_fnm::is_media_name` are ever read or written.
- Document space has **y pointing down**, angles in **radians**, positive is clockwise on screen. Frames are integers; frame 0 is the first; negative keyframes are invalid.
- Layers are drawn first to last (last is on top). The Layers and Timeline panels list them **top of stack first** (`Editor::display_rows`).
- A channel's `base` is its value when unkeyed and, for bones, the rest pose.
- The canvas renders through `fanimate-render` into a texture registered with egui (`canvas_gpu.rs`): render through an `Rgba8UnormSrgb` view, sample as `Rgba8Unorm`. Canvas and export share the renderer, so the screen matches the output.
- **The tessellator never trusts geometry.** Shapes with points that are not finite or lie past `tess::LIMIT` are skipped, and every stroke goes through `Tessellator::stroke_checked`, which drops lyon's output if it panics (debug assertions) or writes NaN (release) and retries a brush stroke at an even width. `crates/fanimate-cli/tests/damaged_projects.rs` opens thousands of damaged projects; keep it passing when changing loading, evaluation or tessellation.
- **wgpu, winit and egui versions are coupled.** wgpu 30 / winit 0.30 / eframe and egui 0.36 / egui_dock 0.21. Upgrade them together and check `cargo tree -d` shows a single wgpu.
- **Windows uses DirectX 12 by default**, never Vulkan, and OpenGL only when DirectX 12 has no hardware adapter: the Vulkan driver for some Intel GPUs (seen on HD Graphics 520) crashes the process on load, uncatchably, and the same machine's OpenGL driver crashes in about 1 of 40 runs at process exit even when merely loaded to list adapters. `fanimate_render::backends()` is the single source of this choice; `WGPU_BACKEND` overrides it.
- **Tools follow the layer type.** The Tools panel always shows the Draw, Fill, Layer, Camera and Workspace groups, as Moho does; the Bone group shows for bone layers (and layers inside one). Drawing with no vector layer selected (nothing, a group, a bone layer) makes a new vector layer (`Editor::drawing_target`). When a layer change hides the active tool (only the bone tools can go), the app switches to Transform. The Camera group (`Tool::TrackCamera`, `ZoomCamera`, `RollCamera`) edits the document camera by the keying rule. There are no Scale Points or Rotate Points tools: with two or more points chosen the Transform tool frames the points (scale, rotate, a movable pivot kept in `Editor.points_pivot`), and the pivot picker (a toolbar and Properties button, the next canvas click sets the pivot) works for layers and points.
- **Keys are selected explicitly.** `Editor` holds a set of selected keys (layer, channel, frame). Add and Delete Key act on it, or on the playhead frame when it is empty; dragging, Alt-drag copying, scaling and the key clipboard (Ctrl+C / Ctrl+V in the timeline) all work on it, one undo step each. Loop modes (`Channel::loop_mode`) and the layer `Order` channel are sampled in the evaluator, never in widgets.
- egui turns Ctrl+C/X/V into `Event::Copy`/`Cut`/`Paste`, not key events; `app.rs` handles them directly. Shortcuts are ignored while a text field has focus, a modal or a menu is open.
- **On Windows the app is a GUI program** (`windows_subsystem = "windows"`), so no console window opens. Print user-facing output with `crate::report`, which falls back to a message box when there is no console.
- **Separate programs never show a console.** Rhubarb (lip sync) ships beside Fanimate; FFmpeg is the user's own (found by `fanimate_export::ffmpeg::find`: the path chosen in Preferences, PATH, then the usual install folders). Both are started through `fanimate_export::ffmpeg::command` or with `CREATE_NO_WINDOW` on Windows, off the UI thread; the user sees progress, never a terminal.
- **Each export format has a route** (`Format::writers`, `fanimate_export::route`): built into Fanimate (GIF, APNG, AVI, AV1, picture sequences), the system's encoder (MP4 on Windows and macOS, `fanimate-oscodec`), or the user's FFmpeg (everything else, and MP4 on Linux). Every route converts frames to BT.709 limited range and tags the file so; Windows' encoder cannot tag, so `fanimate-oscodec` rewrites the finished MP4's `avcC` and adds a `colr` box. A format the computer cannot make stays listed, saying what it needs. Never ship or download FFmpeg.
- **Exports run in the render queue process** (`fanimate --render-queue`, `render_queue.rs`), not the editor, so a render goes on after the editor closes. The editor only adds jobs (`fanimate_export::Queue::add`, a copy of `Document::mainline()`) and reads their status. Every export writes `.part` files and renames them when complete.
- **Templates come from bundled artwork.** Blank, Walk cycle, Lip sync and Frame by frame are built in `fanimate-core::samples`; the richer ones (Bouncing ball, Character rig, Talking head, Mouth styles and the four football scenes) are assembled in `fanimate-editor/src/templates/` from SVG, PNG and WAV files in `crates/fanimate-editor/assets/templates/` (see its README). The art is generated by `scripts/template-art/*.mjs` and the dialogue by `scripts/template-speech/gen-speech.mjs` (Windows voices; mouth cues come from the voice's viseme events), so edit the script and regenerate rather than hand-editing the files. Characters are layered SVGs whose layer names the rig binds to bones (`cast.rs`); mouths are switch layers named A to H and X so Lip Sync works on them. A template is a document built when it is opened, never a stored project; `Sample::ALL` lists them for the menu and Home, and a test saves and reopens each one. The scenes use made-up characters only: never draw a real player, club or sponsor.
- **Importers never panic and always report.** `fanimate_import::import` returns an ordinary `Document` plus a list of what was simplified or left out (`Notes`, counted by kind, phrased "2 masks left out"); the app shows it in the status bar. Bad input is an error string. Add a note kind for anything new an importer drops, and test damaged input. Moho is y-up with a 2-unit-high canvas and frame 0 as the rest pose; Animate edge lists are in twips and fills are rebuilt from `fillStyle0`/`fillStyle1` runs (see the module docs).
- **Updates are signed.** The release workflow signs the Windows installer, the macOS `.app.tar.gz` and the AppImage with the Ed25519 key in the `UPDATE_SIGNING_KEY` secret (`scripts/sign-update.py`); the app holds the public half (`update::PUBLIC_KEY`) and deletes any download that does not verify. Never change the key without shipping the new public key in an update signed by the old one. Downloads go through the system's `curl`; `FANIMATE_UPDATE_FEED` points the updater at a test feed. Update asset names (`Install::asset_suffix`) and `packaging/` must agree.
- **Crash reports are the user's to send.** The panic hook (`crash::install_hook`) writes a report; a locked `session-*` marker catches crashes no hook sees. Fanimate only ever opens a pre-filled GitHub issue in the browser; it never sends anything itself, and there is no telemetry.
- **A maximised borderless window must not cover the whole monitor.** winit sizes it to the work area, which is the whole monitor when the taskbar auto-hides; Windows then treats it as fullscreen (the taskbar stops appearing, Alt+Tab previews and OBS capture can go blank). `titlebar::track_window` replaces that maximise with a normal window one point short of the bottom edge. Keep it when changing window code.

## Commands

```sh
cargo run --release -p fanimate-app                 # run the app
cargo run --release -p fanimate-app -- --bench      # 10 s playback benchmark
cargo run --release -p fanimate-cli -- bench        # CPU benchmark
cargo run --release -p fanimate-cli -- bench --gpu  # plus offscreen GPU render, no window
cargo run --release -p fanimate-cli -- render scene.fnm -o out.png --frame 12
cargo run --release -p fanimate-cli -- export scene.fnm -o out.mp4   # --format F, --ffmpeg PATH
cargo run --release -p fanimate-cli -- formats                        # every format and what makes it here
cargo run --release -p fanimate-cli -- import walk.moho -o walk.fnm   # or .fla/.xfl; lists what was left out
cargo run --release -p fanimate-cli -- corpus corpus/                 # import every Moho/Animate file, report each
scripts/build-web.sh dist/web                       # the browser build (serve the folder over http)
scripts/fetch-rhubarb.sh linux rhubarb-out          # the Rhubarb that releases ship

cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check -p fanimate-core -p fanimate-fnm -p fanimate-editor -p fanimate-render -p fanimate-import --target wasm32-unknown-unknown

python brand/scripts/build_brand.py           # regenerate brand assets
python brand/scripts/build_brand.py --check   # contrast + themes.css check

pnpm release --dry-run                        # what a release would do
```

**CI does not run on every push.** `pnpm release` (scripts/release.js) runs fmt, clippy, tests and the brand check locally, bumps the version in `Cargo.toml`, commits, tags `vX.Y.Z` and pushes; the tag triggers `.github/workflows/release.yml`, which builds and tests on Windows, macOS and Linux, builds the installers and the web build, signs the update files and publishes the GitHub release (notes from `docs/releases/vX.Y.Z.md` when that file exists). So **run the checks above yourself before saying work is done.**

## Code conventions

- Rust edition 2024, `rustfmt.toml` (max width 110). `cargo clippy -D warnings` must pass. `unsafe` is denied workspace-wide; do not add it. The one exception is `fanimate-oscodec`, which calls the operating system's encoders: every `unsafe` block there has a `SAFETY` comment, and only safe functions leave the crate.
- **The Rust version is pinned in `rust-toolchain.toml`** and the release workflow installs exactly that version. Never switch CI back to "latest stable": new Rust releases add clippy lints, and an unpinned CI fails on code that passed locally. To upgrade, bump the pin, run clippy and the tests, fix, and commit in one change.
- Match the surrounding code: comment density, naming and idioms. Comments explain **why**, not what. Module docs say what the module is for.
- No placeholder code: no `let _ = SomeType::X;` to silence imports, no dead branches, no `todo!()` in shipped paths. Remove unused imports instead.
- Errors: libraries return typed errors (`thiserror`); the app shows them in the status bar or a dialog. Never `unwrap` on user data or IO.
- Tests: every editor command, file format change and bug fix gets a test. A bug fix's test must fail before the fix. GPU tests skip cleanly when no adapter exists.
- Dependencies must be MIT/Apache-compatible (no GPL/LGPL in the app or engine). MPL-2.0 is accepted for libraries used unmodified (Symphonia, which reads MP3, Ogg, FLAC and AAC); `scripts/licences.py --check` lists what is allowed. FFmpeg (GPL with x264) is never linked or shipped: Fanimate runs the user's own as a separate program (`docs/legal/video-export.md`). Keep it that way; do not add FFmpeg bindings. Check versions with `cargo search`; prefer versions at least two weeks old.
- Keep `Cargo.lock` committed. Keep generated assets in sync with their sources.

## Documentation

- Update docs in the same change as the code: the spec for format changes, the Phase 1 status for milestone work, CONTRIBUTING for workflow changes, this file for new rules.
- Docs are plain Markdown for a technical audience. Avoid marketing language. Distinguish what exists from what is planned.

## Things not to do

- Do not add features from later phases (node graph, scripting, marketplace, cloud, AI) during Phase 1 without the user asking; record ideas in the roadmap instead.
- Do not make the app depend on network access. Everything works offline.
- Do not add telemetry that is on by default.
- Do not reintroduce Moho's problems: modal settings dialogs, missing context menus, layer operations without shortcuts, or a Moho-only shortcut scheme.
