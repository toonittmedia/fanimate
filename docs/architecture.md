# Architecture

Fanimate is one Rust workspace. The engine knows nothing about windows or files; each layer above it adds one concern.

```
fanimate-text      text engine: shaping, bidirectional order, line breaking, glyph outlines, bundled fonts
  └─ (below core: core calls it to lay text out)
fanimate-core      scene graph, animation channels, bones, frame evaluation
  ├─ fanimate-fnm      .fnm reader, atomic writer, validator and repair, crash journal
  ├─ fanimate-import   Moho and Adobe Animate importers
  ├─ fanimate-render   tessellation (lyon) and the wgpu renderer
  │    └─ fanimate-export   video, animated image and picture sequence export; the render queue folder
  └─ fanimate-editor   editing model: selection, commands, undo, transactions, keymaps (no UI toolkit)
       ├─ fanimate-api      the scripting API as Rust: explicit-target queries and commands, all through transactions
       └─ fanimate-app      desktop app (eframe/egui): panels, canvas, dialogs, updater, crash reports
fanimate-cli       headless tool: render, export, import, validate, benchmark
fanimate-web       browser player (WebGPU): opens, plays and checks projects
fanimate-oscodec   the system's H.264 and AAC encoders (Media Foundation, AVFoundation), used by export
```

Dependencies only point down. `fanimate-core`, `fanimate-fnm`, `fanimate-editor`, `fanimate-render` and `fanimate-import` compile to WebAssembly, so the browser build and future tools share the engine.

## How a frame is drawn

1. **Evaluate** (`fanimate_core::eval`): for a frame number, sample every channel, pose the skeletons, apply Smart Bones and bind artwork to bones, and produce a flat list of shapes and pictures with their transforms.
2. **Tessellate** (`fanimate_render::Tessellator`): turn each shape's fill and stroke into triangles at a tolerance set by the zoom.
3. **Render** (`fanimate_render::Renderer`): draw the triangles with 4x multisampling into a texture.

The canvas, the CLI, the render queue and the browser build all use these three steps, so what you see is what exports.

## How an edit happens

Menus, shortcuts, right-click menus and the command palette all queue an `Action`. `Editor::run` performs it: it records the state before (`Editor::checkpoint`, one undo step per gesture), changes the document, and raises the revision. The app notices the revision change and journals the document for crash recovery.

## Where things live on disk

- A project: a single `.fnm` file, a ZIP archive of readable JSON files; a `.fnm` folder with the same files also opens ([specification](spec/fnm.md)).
- Settings, crash recovery journals, crash reports, downloaded updates and the render queue: the user data folder ([Installing](install.md)).

More detail for contributors is in [CLAUDE.md](../CLAUDE.md) (rules and key facts) and [CONTRIBUTING.md](../CONTRIBUTING.md) (workflow).
