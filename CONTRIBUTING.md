# Contributing to Fanimate

Thanks for helping build Fanimate. This guide covers how to get the code running, what we expect from a change, and how decisions are made.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Where things stand

Fanimate is in **Phase 1**. Milestone M1 (engine spike) proved the architecture: the core engine, the GPU renderer, the `.fnm` file format and the app shell with its Moho-style layout. It is done on Windows and waits for benchmark numbers from macOS and Linux. M2 (Draw), M3 (Rig) and M4 (Frame-by-frame and voice) are built and wait for their exit checks with testers. M5 (Bring your files) is built and released as the first public alpha: the Moho and Animate importers, installers, the self-updater, crash reports and the web build. The public beta and 1.0 now come after Phase 2 (decided 2026-10-30, so that films, examples and handmade templates exist before the public release; see [docs/roadmap/release-1.0.md](docs/roadmap/release-1.0.md)). Import test files are the most useful contribution right now: see [corpus/README.md](corpus/README.md). See the [Phase 1 plan](docs/roadmap/phase-1.md#status) for details.

Good ways to help right now:

- Run the benchmark on your machine, especially on macOS or Linux, and post the numbers in an issue (see [Benchmarks](#benchmarks)).
- Try the drawing tools with a pen tablet and report how pressure and smoothing feel.
- Contribute Moho, Animate, SVG or PSD files to the [import test corpus](corpus/README.md) (open an issue first; we need permission to use each file).
- Pick up an issue labelled `good first issue`.
- Review the [product vision](docs/product-vision.md) and the [`.fnm` specification](docs/spec/fnm.md) and open issues where they are unclear or wrong.

## Getting started

You need [rustup](https://rustup.rs). The Rust version is pinned in `rust-toolchain.toml` and installs automatically on the first build, so your checks match the release workflow exactly.

```sh
git clone https://github.com/toonittmedia/fanimate
cd fanimate
cargo run --release -p fanimate-app        # open the window with the hello sample
cargo test --workspace                     # run the tests
```

Linux also needs the usual windowing libraries, for example on Ubuntu:

```sh
sudo apt install libxkbcommon-dev libwayland-dev libgtk-3-dev libasound2-dev
```

Lip sync runs Rhubarb, a separate program that releases ship beside Fanimate: in a development build, put `rhubarb` on your PATH or fetch the one releases ship with `scripts/fetch-rhubarb.sh <windows|linux|macos> rhubarb-out`. MP4 export uses the system's encoder on Windows and macOS. The formats FFmpeg adds (H.265, WebM, MKV, ProRes, DNxHR, WebP, and MP4 on Linux) use any `ffmpeg` you install; their tests skip when there is none. Releases do not ship FFmpeg.

### Repository layout

| Path | What it is |
|---|---|
| `crates/fanimate-core` | Scene graph, animation, bones, evaluation. No UI or GPU code. |
| `crates/fanimate-fnm` | `.fnm` reader, writer and validator |
| `crates/fanimate-render` | Tessellation (lyon) and GPU drawing (wgpu) |
| `crates/fanimate-editor` | Editing model: selection, commands, undo, clipboard, keymaps. No UI toolkit code. |
| `crates/fanimate-export` | Video, animated image and picture sequence export, sound mixing and the render queue folder |
| `crates/fanimate-oscodec` | The system's H.264 and AAC encoders (Media Foundation, AVFoundation); the one crate allowed `unsafe` |
| `crates/fanimate-app` | The desktop app (`fanimate` binary): title bar, docked panels, canvas, dialogs, render queue window |
| `crates/fanimate-cli` | Headless tool (`fanimate-cli`): render, export, validate, benchmark |
| `docs/` | Product vision, roadmap, file format specification |
| `brand/` | Logo, icons, design tokens; see [brand/README.md](brand/README.md) |

Keep `fanimate-core` free of UI, GPU and platform code. It has to run headless on a render server and in the browser. Keep `fanimate-editor` free of egui, so every command can be unit-tested without a window.

### Useful commands

```sh
cargo run --release -p fanimate-app -- --bench            # 10 s GPU benchmark, prints fps
cargo run --release -p fanimate-cli -- bench --gpu        # CPU benchmark plus offscreen GPU render
cargo run --release -p fanimate-cli -- sample hello hello.fnm
cargo run --release -p fanimate-cli -- render hello.fnm -o hello.png --frame 12
cargo run --release -p fanimate-cli -- validate hello.fnm
```

## Making a change

1. **Open an issue first** for anything larger than a small fix, so the approach can be agreed before you spend time on it.
2. Create a branch from `main`.
3. Keep the change focused: one feature or fix per pull request.
4. Before pushing, run:

   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

5. Open a pull request using the template. CI does not run automatically on pushes or pull requests, so the checks in step 4 are what keep `main` healthy. A maintainer can run the full build on all platforms from the Actions tab (**Release → Run workflow**) when a change needs it.

## Releases

Maintainers cut releases from `main` with:

```sh
pnpm release            # patch: 0.0.1 -> 0.0.2
pnpm release minor      # 0.0.2 -> 0.1.0
pnpm release 1.0.0      # an exact version
pnpm release --dry-run  # run the checks and show the plan, change nothing
```

The script requires a clean `main` that is up to date with GitHub. It runs fmt, clippy, the tests and the brand token check, sets the version in `Cargo.toml`, commits, tags `vX.Y.Z` and pushes. The tag starts the [release workflow](.github/workflows/release.yml), which tests and builds on Windows, macOS and Linux and publishes the GitHub release with the binaries, using `docs/releases/vX.Y.Z.md` as its notes when that file exists. Write the notes before releasing.

### What we look for in review

From the product principles:

- **Never lose work.** Anything touching saving, loading or undo needs tests, including what happens on failure.
- **Fast by default.** Changes on the per-frame path (evaluation, tessellation, rendering) should include benchmark numbers from before and after.
- **Polish before features.** A smaller finished feature beats a larger half-finished one.
- **Tests** for new behaviour, and for every bug fix a test that failed before the fix.
- **Comments explain why**, not what the code already says.

### Interface conventions

Fanimate keeps Moho Pro's layout but fixes its interaction problems. When you add UI:

- **Every command is an `Action`**, run through `Editor::run`, so menus, shortcuts, right-click menus and the command palette behave the same.
- **Add a right-click menu** to anything the user can select, reusing the `menus::item` helper.
- **Bind shortcuts in both keymaps** in `crates/fanimate-editor/src/keymap.rs`: Fanimate (standard editing conventions, the default) and Moho. Never hard-code a key in UI code.
- **No modal dialogs for settings**; use the Properties panel.
- **No em dashes in UI text.** Use a colon, comma or parentheses. A test enforces this.
- Colours come from `brand/tokens/tokens.json` via the theme, never hard-coded.

[CLAUDE.md](CLAUDE.md) has the full list of rules.

### Clean-room rule

Fanimate follows Moho Pro's layout and workflow but never its code. **Do not contribute code, icons or artwork copied or derived from Moho, Adobe Animate, Toon Boom Harmony or any other closed product**, and do not use decompiled or leaked material. Importers are written from public documentation and from inspecting files you are allowed to use. If you have worked on one of those products' source code, mention it in your pull request so we can decide together whether you should work on related areas.

### AI-assisted contributions

AI tools are welcome, but you are responsible for every line you submit: you must understand it, have tested it, and be able to explain it in review. Say in the pull request if a substantial part was generated.

## Benchmarks

The Gate 1 performance budget is 60 fps for a 40-bone rig on a mid-range laptop, defined in [docs/reference-hardware.md](docs/reference-hardware.md). To help measure it:

```sh
cargo run --release -p fanimate-app -- --bench --no-vsync
cargo run --release -p fanimate-cli -- bench --gpu
```

Post both outputs with your CPU, GPU, operating system, screen resolution and scaling.

## Licence of contributions

Fanimate is dual-licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE). Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Fanimate by you, as defined in the Apache-2.0 licence, shall be dual-licensed as above, without any additional terms or conditions.

## Decisions

How decisions are made, who maintains what, and how releases work are described in [GOVERNANCE.md](GOVERNANCE.md).
