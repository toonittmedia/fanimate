<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/logo/fanimate-lockup-dark.svg">
    <img src="brand/logo/fanimate-lockup-light.svg" alt="Fanimate" height="80">
  </picture>
</p>

<p align="center"><strong>Free to animate.</strong><br>
Open-source 2D character animation. Rig it. Draw it. Move it.</p>

<p align="center"><a href="https://github.com/toonittmedia/fanimate/actions/workflows/release.yml"><img src="https://github.com/toonittmedia/fanimate/actions/workflows/release.yml/badge.svg" alt="Release build"></a></p>

---

Fanimate is a free, open-source 2D character animation app. It keeps what animators love about Moho Pro (bone rigging, Smart Bones, vector puppets), fixes the problems that push people away from it, and adds the best tools from Toon Boom Harmony and Adobe Animate.

> **Status: public alpha (Phase 1, milestone M5).** You can draw and animate vector shapes, rig characters with bones, IK, Smart Bones and actions, animate frame by frame with onion skin and the X-sheet, lip sync dialogue, import Moho, Animate, Photoshop and SVG files, and export video, and you get unsaved work back after a crash. Installers update themselves. It is an alpha: expect rough edges, keep backups, and please [report what goes wrong](https://github.com/toonittmedia/fanimate/issues).

## Download

Installers for Windows, macOS and Linux, and a browser build, are on the [releases page](https://github.com/toonittmedia/fanimate/releases). See [Installing and updating](docs/install.md); the builds are not code-signed yet, so the first start needs one extra click. Progress is tracked in the [Phase 1 plan](docs/roadmap/phase-1.md#status).

## Try it

You need [rustup](https://rustup.rs); it installs the pinned Rust version from `rust-toolchain.toml` on first build. On Linux, also install `libxkbcommon-dev`, `libwayland-dev`, `libgtk-3-dev` (for file dialogs) and `libasound2-dev` (for sound).

```sh
git clone https://github.com/toonittmedia/fanimate
cd fanimate
cargo run --release -p fanimate-app            # open the app
cargo run --release -p fanimate-app -- --bench # 10-second performance benchmark
```

On first launch Fanimate asks which keyboard shortcuts you want:

- **Fanimate (standard):** shortcuts like other editing apps. Ctrl+C, Ctrl+X and Ctrl+V work on layers, Ctrl+Y or Ctrl+Shift+Z redo, Delete deletes keys, points and shapes, Ctrl+Delete deletes layers, F2 renames.
- **Moho:** Moho Pro's own shortcuts, for Moho users.

Switch any time in **Edit › Keyboard Shortcuts**. The menus live in the title bar, every panel has right-click menus, and **Space** plays and pauses. **File › Open Sample** has the hello rig, the 40-bone benchmark and a 400-bone stress test.

The headless tool renders and checks projects without a window:

```sh
cargo run --release -p fanimate-cli -- sample hello hello.fnm
cargo run --release -p fanimate-cli -- render hello.fnm -o hello.png --frame 12
cargo run --release -p fanimate-cli -- validate hello.fnm
```

## Goals

- **Moho's rigging, Harmony's drawing and pipeline, Animate's ease of use**, in one app.
- **Familiar on day one, easier than Moho:** Moho Pro's layout and tool names, a Moho keymap for Moho users, but standard editing shortcuts, right-click menus everywhere and no modal settings dialogs. Fanimate's own code, icons and styling.
- **Free on every platform:** Windows, macOS and Linux natively, plus a browser build. Tablets after 1.0.
- **Fast:** one Rust codebase with GPU rendering (wgpu), built to hold 60 fps with complex rigs.
- **Never lose work:** an open, documented `.fnm` file format with self-repair and crash-proof autosave.
- **Open everything:** source, file format, roadmap and API.

## Documents

| Document | What it covers |
|---|---|
| [Product vision](docs/product-vision.md) | The problem, principles, full feature specification, AI, business model, roadmap and risks |
| [Phase 1: Foundation](docs/roadmap/phase-1.md) | Status, branding, scope, workstreams, milestones and Gate 1 acceptance tests for version 1.0 |
| [User manual](docs/manual/README.md) | How to use Fanimate, chapter by chapter |
| [Installing and updating](docs/install.md) | Installers, updates and crash reports |
| [Importing](docs/import.md) | What comes through from Moho and Animate projects |
| [Fanimate in the browser](docs/web.md) | The web build and its limits |
| [Architecture](docs/architecture.md) | How the crates fit together |
| [`.fnm` format specification](docs/spec/fnm.md) | The open project file format |
| [Legal review scope](docs/legal/review-scope.md) | Questions for the legal review required before 1.0 |
| [Brand guidelines](brand/README.md) | Logo, colours, themes, typography and how to rebuild the assets |
| [Contributing](CONTRIBUTING.md) and [Governance](GOVERNANCE.md) | How to help and how decisions are made |
| [CLAUDE.md](CLAUDE.md) | Rules for AI assistants working in the repo; also a compact guide to the project's conventions |

## Roadmap

| Phase | Duration (estimate) | Focus | Ends at |
|---|---|---|---|
| **1. Foundation** | About 2 to 3 more months (M0 to M5 are built; the public beta remains) | Engine and renderer, drawing and frame-by-frame, rigging, lip sync, SVG, PSD, Moho and Animate import, video export | **Version 1.0** |
| 2. Production | About 6 months | Node graph, text animation, auto colour fill, raster brushes, scripting API, local AI tools, Lottie and game export | A pilot short film ships |
| 3. Studio | About 6 months | Fanimate Hub, review and approvals, cloud rendering, marketplace | Studio-ready |
| 4. Reach | Ongoing | Tablet apps, live co-editing, Harmony import, education | — |

Each phase ends at a quality gate, not a date. See the [product vision](docs/product-vision.md#roadmap-risks-and-success-metrics) for details.

## Repository layout

```
fanimate/
├── crates/
│   ├── fanimate-core/     Scene graph, animation, bones, evaluation (no UI or GPU code)
│   ├── fanimate-fnm/      .fnm reader, writer and validator
│   ├── fanimate-render/   Tessellation (lyon) and GPU drawing (wgpu)
│   ├── fanimate-editor/   Editing model: commands, undo, clipboard, keymaps (no UI code)
│   ├── fanimate-app/      Desktop app (the `fanimate` binary): title bar, panels, canvas
│   └── fanimate-cli/      Headless tool: render, validate, benchmark
├── docs/                  Vision, roadmap, file format spec, reference hardware, legal scope
├── corpus/                Import test files and their consent record
├── brand/                 Logo, icons, design tokens, fonts and build script
├── scripts/release.js     `pnpm release`: check, version, tag and push
└── .github/               Release workflow, issue and pull request templates
```

## Contributing

Contributions are welcome, from code to benchmark results on your hardware to test files for the importers. Start with the [contribution guide](CONTRIBUTING.md). Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). Report security problems privately as described in [SECURITY.md](SECURITY.md).

## Licence

Fanimate is dual-licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Fanimate by you, as defined in the Apache-2.0 licence, shall be dual-licensed as above, without any additional terms or conditions.

The Inter font in `brand/fonts/` is licensed separately under the [SIL Open Font License 1.1](brand/fonts/Inter-OFL.txt). The interface icons in `crates/fanimate-app/icons/` come from [Lucide](https://lucide.dev) under the [ISC licence](crates/fanimate-app/icons/lucide/LICENSE). The Fanimate name and logo identify this project; see the [brand guidelines](brand/README.md#licence).
