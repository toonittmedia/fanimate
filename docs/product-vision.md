# Fanimate: Product Vision for a Next-Gen 2D Character Animation App

## Executive summary

Fanimate is a free, open-source 2D character animation app that keeps what animators love about Moho Pro (bone rigging, Smart Bones, vector puppets) and fixes the 20+ problems that push people away from it. It keeps Moho Pro's interface so Moho users feel at home on day one, and adds the best features of Adobe Animate and Toon Boom Harmony.

The thesis copies the playbook of [ArtCraft](https://petapixel.com/2026/10/07/someone-rebuilt-free-open-source-versions-of-photoshop-premiere-and-lightroom-using-ai/), which in October 2026 released free, open-source Rust rebuilds of seven Adobe apps. ArtCraft funds itself by selling optional AI-generation credits, ships native Linux builds, and runs some apps in the browser via WebAssembly ([IT Pro Expert](https://itproexpert.com/goodbye-adobe-new-free-open-source-adobe-looking-alternatives-for-windows-mac-and-linux/)). Fanimate applies the same model to 2D character animation.

The timing is right. Adobe Animate went into maintenance mode in February 2026 with no new features planned, Toon Boom Harmony is subscription-only, and Moho is a small closed product with a slow release cadence. No open, modern, rig-first tool exists for the animators caught in between.

**Decisions taken for this document**

| **Decision** | **Choice** | **Why** |
|---|---|---|
| Business model | Free and open source (MIT / Apache 2.0); paid cloud rendering and AI credits | Removes the price barrier; matches the ArtCraft model |
| Relationship to Moho | Moho Pro-style interface, plus the best of Animate and Harmony | Moho users switch with no relearning; Animate and Harmony users gain their favourite tools |
| Tech foundation | Rust + GPU rendering (wgpu); native on Windows, macOS, Linux, plus a browser build via WebAssembly | Speed, safety, one codebase for every platform |
| AI approach | Hybrid: local models for assistive tools, cloud credits or your own API keys for heavy generation | Works offline and privately; heavy jobs stay optional |

**The one-line pitch:** Moho's rigging, Harmony's drawing and pipeline, Animate's ease of use. Free, on every platform, with nothing locked away.

## Why Moho Pro frustrates users

Moho's rigging is loved; almost everything around it is the problem. The table lists every pain point Fanimate must solve. IDs (P1–P24) are used throughout this document.

| **ID** | **Pain point** | **What users actually experience** |
|---|---|---|
| P1 | Not truly cross-platform | Windows and macOS only. No Linux, no tablet, no browser version. Harmony now runs on Linux; Moho does not. |
| P2 | No extension marketplace | Scripts are shared as loose Lua files on forums. No discovery, ratings, updates or safe install. |
| P3 | Weak text animation | Few text styles, no per-character animators, no library of animated title templates. |
| P4 | No pipeline (node-based) editor | Everything is layer-based. No node graph for compositing, effects or rig logic like Harmony's Node View. |
| P5 | No cloud rendering | Long renders tie up the artist's machine. No render farm or remote queue. |
| P6 | Lip sync quality | Automatic lip sync exists but is basic. Users want Rhubarb-level phoneme accuracy and more mouth shapes. |
| P7 | Rigging could be better | Smart Bones are powerful but fiddly. Rigs break silently; no rig templates, retargeting or auto-rig. |
| P8 | No AI tools | No AI inbetweening, auto-colouring, auto-rigging or pose suggestions. Harmony 25 shipped its own AI features (Ember). |
| P9 | Poor freehand and frame-by-frame drawing | Strange curves at stroke ends, weak brushes, drawing tools often locked to frame 0. Studios switch to Animate or Harmony for frame-by-frame. |
| P10 | Weak bitmap tools | Raster brushes added in Moho 13 are limited to quick sketches compared with Krita. |
| P11 | No automatic colour fill across frames | Every frame is coloured by hand. Users ask for TVPaint-style colour marks that fill later frames. |
| P12 | Slow and heavy with complex scenes | Rigs lag when moving bones; shape effects and masks slow playback; zooming in slows the canvas. |
| P13 | Hidden file-corrupting bugs | The "negative keyframe" bug and broken references silently fill channels with keyframes and make files painfully slow. |
| P14 | Crashes and unstable major releases | Veterans keep the previous version for real work; a .0 release is treated as experimental. Crashes can lose unsaved rig work. |
| P15 | Dated, unintuitive UI | Floating windows, no proper docking, poor scaling on high-resolution screens, hard-to-read labels. |
| P16 | Polish neglected for new features | Long-standing timeline and undo annoyances remain while new features ship half-finished. |
| P17 | No team collaboration | No shared asset library, no project server, no review/approval loop. Built for one artist, not a studio. |
| P18 | Limited scripting API | Lua is high-level; even creating a folder needs a system call or a compiled plugin. Hard to build studio pipelines. |
| P19 | Poor SVG import and interoperability | Complex SVG files import badly. No Lottie, web or game-engine export. Weak links to 3D tools. |
| P20 | Small industry footprint | Few studios list Moho in job ads, so learning it opens fewer doors and studios hesitate to adopt it. |
| P21 | Licensing and upgrade friction | Paid major upgrades ($349.99 from Pro 12 to Pro 14); serials bought from some resellers excluded from free updates; unreliable in-app updater. |
| P22 | Slow, closed development | Small team, long gaps between versions, roadmap decided privately. Users can't fix bugs themselves. |
| P23 | Awkward layer handling, no right-click menus | The Layers panel works unlike other editing apps: no context menus anywhere, layers copied and pasted only through Edit > Copy Layer / Paste Layer with no shortcut, settings changed in blocking modal dialogs. |
| P24 | Unconventional shortcuts | Ctrl+C / Ctrl+V do not copy and paste layers, Ctrl+Y is not redo (it selects a tracing image; redo is only Ctrl+Shift+Z), tools use single letters unlike any other app. Newcomers from Adobe, Toon Boom or Krita fight the keyboard. |

What Moho gets right, and what Fanimate must keep, is just as clear: Smart Bones, easy inverse kinematics (IK), bone physics, PSD-based puppets, a perpetual (non-subscription) feel, and a friendly, generous community.

## Product principles and positioning

Fanimate wins by being the only tool that is rig-first *and* draw-first, free, open and everywhere. Seven principles guide every design decision:

1. **Rig-first, but drawing is not second-class.** Every drawing tool must be good enough for full frame-by-frame production (fixes P9–P11).
2. **Fast by default.** 60 fps playback on a mid-range laptop for a typical TV-quality rig; heavy work runs on the GPU (P12).
3. **Never lose work.** Crash-proof autosave, file health checks, and versioned history (P13, P14).
4. **Polish before features.** A release ships only when its features are finished; bug fixes have their own release train (P16).
5. **Open everything.** Open file format, open source, open roadmap, open API (P18, P22).
6. **Plays well with others.** Clean import from SVG, PSD, Moho, Animate and Harmony; export to video, Lottie, web and game engines (P19).
7. **Looks like Moho Pro, works like a modern editor.** Fanimate keeps Moho Pro's layout (tools on the left, canvas in the centre, Layers, Style and Actions panels on the right, timeline along the bottom) with Moho's tool names, groupings and concepts (bones, Smart Bones, actions, switch layers). It does **not** keep Moho's interaction problems (P23, P24): every panel has right-click menus, layers copy and paste with Ctrl+C / Ctrl+V, Ctrl+Y redoes, and settings are edited in a docked panel instead of modal dialogs. Moho users can switch to a Moho keymap at any time. Fanimate draws its own icons and visual styling and is written from scratch, never from Moho's code.

### How Fanimate compares

|  | **Moho Pro 14** | **Toon Boom Harmony 25** | **Adobe Animate** | **Fanimate (target)** |
|---|---|---|---|---|
| Price | $399.99 one-time | Subscription only | Creative Cloud subscription | **Free; optional paid cloud and AI credits** |
| Source | Closed | Closed | Closed | **Open (MIT / Apache 2.0)** |
| Platforms | Windows, macOS | Windows, macOS, Linux | Windows, macOS | **Windows, macOS, Linux, browser; tablets later** |
| Rigging | Excellent (Smart Bones) | Strong, more technical | Basic | **Moho-class plus templates, retargeting, auto-rig** |
| Frame-by-frame | Weak | Excellent | Very good | **Excellent (vector and raster)** |
| Node editor | None | Full Node View | None | **Optional node graph beside the timeline** |
| Collaboration | None | Harmony Server | Basic | **Built-in project server and cloud sync** |
| AI | None | Ember | None new | **Hybrid local and cloud** |
| Scripting | Lua | JavaScript, Python | JavaScript (JSFL) | **Python and TypeScript, plus WASM plugins** |
| Development | Slow, private | Active, private | Maintenance mode | **Public, community-driven** |

**Target users, in order:** (1) independent animators and YouTubers priced out of Harmony; (2) Moho and Animate users looking for a modern home; (3) small studios (2–30 people) doing TV-style cut-out work; (4) schools and training centres, especially where software costs are a real barrier, such as Nigeria and other emerging markets.

## Moho Pro interface, with the best of Animate and Harmony

Fanimate keeps Moho Pro's interface and rigging, then adds the tools Animate and Harmony users would miss most. Every borrowed feature slots into a Moho-style panel, so the screen still feels like Moho.

### Default layout (Moho Pro style)

| **Area** | **As in Moho Pro** | **What Fanimate adds** |
|---|---|---|
| Tools panel (left) | Draw, Fill, Bone, Layer, Camera and Workspace tool groups | Animate-style brushes, Harmony-style curve deformer and cutter tools |
| Canvas (centre) | Single canvas with bone and point overlays | Canvas rotation for drawing, off-stage pasteboard area, light table |
| Layers panel (right) | Layer list with groups, switch, bone and image layers | Library tab and Node view tab beside the layer list |
| Style panel (right) | Fill and line colours, brushes | Linked palettes: change a colour once, it updates everywhere |
| Actions panel (right) | Smart Bone actions | On-canvas sliders and 2D control pads for actions |
| Timeline (bottom) | Keyframes, channels, sequencer | X-sheet tab, graph editor, motion and easing presets |

Panels open where Moho users expect them, but every panel can also be docked, tabbed or moved to a second monitor.

### Taken from Adobe Animate

| **Feature** | **Why Animate users love it** | **How it works in Fanimate** |
|---|---|---|
| Symbols and Library | Reuse an animated object (a blinking eye, a walking extra) many times; edit it once and every copy updates | Any group can become a symbol with its own timeline and loop options, stored in the Library tab |
| Shape tweens with hints | Morph one drawing into a completely different one automatically | Shape-tween option on vector layers; hint points match parts of the two shapes |
| Classic easing and motion presets | Apply a saved motion (bounce in, fade out) in one click | Presets panel in the timeline; shareable on the marketplace |
| Pressure brushes and paint modes | Natural variable-width lines; paint behind, inside or only on fills | Part of the brush engine (feature 1) |
| Asset Warp | Bend a drawing with a few pins, no bones needed | Warp tool in the Bone group; pins become a deformation mesh that can later get bones |
| Layer parenting | Quick rigs by parenting layers to each other | Parent dropdown in the Layers panel, alongside full bone rigs |
| Interactive publishing | Buttons and scripted behaviour for web, e-learning and banners | Interactive HTML5 and Lottie export with TypeScript actions |
| Simple to learn | Small toolset that beginners pick up fast | "Simple mode" hides advanced panels for students and beginners |

### Taken from Toon Boom Harmony

| **Feature** | **Why Harmony users love it** | **How it works in Fanimate** |
|---|---|---|
| Node view | Full control of effects, compositing and rig logic | Optional node graph, synced with the layer list (feature 7) |
| Curve and envelope deformers | Smooth, bendy limbs and props without bone-joint creases | Curve deformer tool next to bones; works on vector and raster art |
| Master controllers | One slider or handle drives a complex change, such as a full head turn | Smart Bone actions exposed as on-canvas sliders, dials and 2D pads |
| Drawing substitution | Swap hand poses or mouth shapes per frame from a visual list | Switch layers gain a thumbnail picker and per-frame swapping on the timeline |
| Art layers | Separate underlay, line art, colour art and overlay inside one drawing | Every drawing has these four sub-layers, so colour never damages line work |
| X-sheet | The traditional exposure sheet animators read like a score | X-sheet tab in the timeline (feature 2) |
| Cutters and auto-matte | Hide parts of a drawing with another shape (a mouth inside a head) | Cutter and matte nodes, also available from the Layers panel |
| Multiplane 3D space | Layers placed at real depth for parallax camera moves; 3D models in 2D scenes | Z-depth for every layer, 3D camera, glTF model import |
| Game export | Rigs exported for game engines | Spine-compatible, Godot, Unity and Unreal export |
| Studio server | Many artists on one production database | Fanimate Hub (collaboration section) |

## Platform and architecture

Fanimate is one Rust codebase that renders on the GPU and compiles to native apps for Windows, macOS and Linux and to WebAssembly (WASM) for the browser. This answers P1, P12 and P14 at the foundation rather than with patches.

**Key terms.** *Rust* is a systems programming language that is as fast as C++ but prevents most memory crashes at compile time. *wgpu* is a Rust graphics library that talks to each platform's GPU interface (Vulkan, Metal, DirectX 12, WebGPU). *WebAssembly* lets the same compiled code run inside a web browser at near-native speed.

### Layers of the system

1. **Core engine (Rust).** Scene graph, bones and deformation, animation curves, undo history. Has no user-interface code, so it can run headless on a render server.
2. **Renderer (wgpu).** Draws vectors, raster strokes and effects on the GPU. Vector shapes are tessellated (cut into triangles) on the GPU so large scenes stay smooth. The same renderer is used for the canvas and the final render, so what you see is exactly what you export.
3. **Evaluation scheduler.** Works out which bones, shapes and effects actually changed on a frame and recalculates only those, spread across all CPU cores. This is how Fanimate avoids Moho's lag with complex rigs.
4. **UI shell.** A docking interface (see the UI section) drawn by the same GPU renderer, so it scales cleanly on any screen density.
5. **Plugin host.** Runs extensions in a sandbox (see the extension section).
6. **Services (optional).** Cloud render, AI, sync and the marketplace, all switchable off; the app works fully offline.

### File format: .fnm

- **Open and documented.** A .fnm project is a folder (or a zipped single file) holding a human-readable scene description plus binary assets. The specification is published, so other tools can read and write it.
- **Version-control friendly.** Scene data is split into small text files per layer or rig, so Git diffs are readable and two artists rarely touch the same file.
- **Self-healing.** On every save, a validator checks for broken references, duplicate or out-of-range keyframes and orphaned data, and repairs or flags them. This directly targets Moho's negative-keyframe and broken-reference bugs (P13).
- **Crash-proof saving.** Autosave every 60 seconds into a separate journal; a crash recovers to the last action, not the last manual save (P14).

### Platform targets

| **Platform** | **Version 1.0** | **Notes** |
|---|---|---|
| Windows 10/11 | Yes | DirectX 12 or Vulkan |
| macOS 13+ (Apple Silicon and Intel) | Yes | Metal |
| Linux (Ubuntu, Fedora, Rocky, Flatpak) | Yes | Vulkan; first-class, not an afterthought |
| Browser (Chrome, Edge, Firefox, Safari) | Yes, with limits | WebGPU; best for learning, review and light work; projects sync with desktop |
| iPad and Android tablets | After 1.0 | Same engine, touch-first UI for drawing and posing |

**Performance budget (release blockers):** canvas playback at 60 fps for a 40-bone character with 10 Smart Bones on a mid-range laptop; project open under 3 seconds for a 5-minute episode scene; no single UI action slower than 100 ms.

## Core feature specification

These are the features that make Fanimate a complete replacement for Moho Pro and a credible alternative to Harmony and Animate. Each lists the pain points it fixes.

### 1. Drawing engine (P9, P10)

Fanimate has one brush engine that can paint **vector strokes** (editable curves) or **raster strokes** (pixels) on any layer type, on any frame, never locked to frame 0.

- **Pressure-true vector brushes.** Strokes are stored as a centreline plus a width profile driven by pen pressure, tilt and speed. Stroke ends taper naturally, fixing Moho's "weird curvature at endpoints" complaint.
- **Stroke smoothing with prediction.** Adjustable stabiliser (like Clip Studio Paint and Krita) that smooths shaky lines without lagging behind the pen.
- **Clean-up tools.** Auto-close gaps, merge overlapping strokes, and a "pencil to ink" pass that turns rough sketch strokes into clean vector lines.
- **Raster brushes on par with Krita.** Textured brushes, blending, smudge, and import of Krita and Photoshop (.abr) brush presets.
- **Point and curve tools** that Moho users know (add point, curve, transform points) remain for construction-style drawing.
- **Boolean shape tools** (union, subtract, intersect), live shape primitives, and a pen tool that behaves like Illustrator's.

### 2. Frame-by-frame workflow (P9)

A dedicated frame-by-frame mode makes Fanimate a real tool for traditional animation, not just puppets.

- **Exposure sheet (X-sheet) and timeline views** of the same data. The X-sheet is the vertical, column-per-character chart traditional animators and Harmony users rely on.
- **Onion skinning** with colour-coded past and future frames, adjustable range, and "key drawings only" filter.
- **Light table and flipping:** flip between selected drawings with a hotkey; roll back and forth with a pen gesture.
- **Rough, clean and colour layers** per drawing, so a scene can go from blue-pencil rough to final colour without new layers for every step.
- **Mix freely:** a frame-by-frame layer can sit inside a rigged character, e.g. hand-drawn hand poses on a bone-rigged body.

### 3. Colouring and automatic fill (P11)

- **Gap-tolerant paint bucket** that closes small gaps in line art automatically.
- **Colour marks:** mark each closed shape in the first frame with a small coloured dash; Fanimate tracks those regions through the following frames and fills them (the approach TVPaint users love). A review panel highlights frames where tracking was uncertain.
- **AI colour assist** (optional, see AI section) for harder cases.
- **Palettes linked to styles,** so changing a character's shirt colour updates every frame and every rig at once.

### 4. Rigging system (P7)

Fanimate keeps Moho's rigging ideas (they are the reason people use Moho) and makes them sturdier and easier.

- **Bones, IK and FK.** *Forward kinematics (FK)* moves a chain from the root outward; *inverse kinematics (IK)* lets you drag a hand and the arm follows. Fanimate supports both and lets you blend between them per limb.
- **Smart Bones.** A driver bone's rotation or position controls corrective shape changes. For example, turning a head bone morphs the face to a three-quarter view. Fanimate's Smart Bones support multiple inputs (two bones driving one shape) and visual curves showing how the shape responds.
- **Mesh deformation.** Bind any vector or raster art to a deformation mesh with automatic weight painting, so bitmap PSD characters bend smoothly instead of cracking at joints.
- **Rig templates and auto-rig.** Start from a biped, quadruped, bird or face template; drop in your artwork and Fanimate fits the bones. Optional AI auto-rig detects limbs on imported art.
- **Retargeting.** Copy animation from one rig to another with different proportions (a short character and a tall one can share a walk cycle).
- **Rig health check.** Warns about bones with no influence, conflicting drivers, keyframes on hidden channels and other issues that slow scenes down, before they become bugs.
- **Physics:** bone dynamics, hair and cloth springs, wind and gravity, with a bake-to-keyframes option.
- **Moho compatibility:** Smart Bone actions and switch layers import from .moho files (see migration section).

### 5. Timeline and animation editing (P16)

- **Dope sheet, graph editor and X-sheet** as three views of the same keys.
- **Animation layers:** stack a breathing cycle under a walk, then add a one-off head turn on top, each with its own weight.
- **Pose library and action library** shared across projects; drag a pose onto the timeline.
- **Robust undo:** unlimited, per-object undo history that survives a crash, with a visual history panel.
- **Bulk tools:** copy, mirror, offset and retime keys across many bones without the slow-down Moho shows when copy-pasting keyframes in large files.

### 6. Text animation (P3)

Text becomes a first-class animated object, similar to After Effects' text animators.

- **Per-character, per-word and per-line animators:** offset position, rotation, scale, colour, blur and opacity across characters with a range selector.
- **Text on a path,** variable fonts, OpenType features, right-to-left and complex scripts (Arabic, Hindi) shaped correctly.
- **Template gallery:** a wide set of ready-made title, lower-third, caption and kinetic-typography templates, editable and shareable through the marketplace.
- **Auto-captions:** import an SRT subtitle file or transcribe audio, then style captions with a template.

### 7. Pipeline editor: the node graph (P4)

Fanimate offers an optional **node graph** next to the familiar layer list. Each node is one operation; cables carry images or data from one node to the next. Beginners never need to open it; studios get Harmony-level control.

- **Compositing nodes:** blur, glow, colour correction, light and shadow, masks, blend modes, cut-outs (mattes).
- **Rig logic nodes:** expressions, constraints and math that drive bones or shape values (e.g. "eye blink every 3–5 seconds").
- **Render passes:** output separate line, colour, shadow and effects passes for compositing in another program.
- **Two-way sync:** a change in the layer list appears in the graph and vice versa, so no one is forced to choose.

### 8. SVG import done right (P19)

SVG is a major entry point for art made in Illustrator, Inkscape, Affinity Designer, Figma or ArtCraft's VectorCraft, so the importer is a priority, not an afterthought.

- **Full SVG 1.1 and SVG 2 coverage** for drawing content: paths, all shape elements, groups, nested transforms, clipping paths, masks, linear and radial gradients, patterns, strokes with dashes and joins, and embedded images.
- **Structure preserved:** each SVG group becomes a Fanimate group; layer names (including Illustrator and Inkscape layer labels) are kept, so the art is ready to rig.
- **Exact geometry:** curves import as-is, with no re-sampling that adds hundreds of points.
- **CSS and styles resolved:** inline styles, `<style>` blocks and classes are converted into Fanimate styles and palettes.
- **Text choice:** keep text editable, or convert to outlines when the font is missing.
- **Import report:** a short summary of anything that couldn't map exactly (e.g. an unsupported filter) with a preview so nothing is silently lost.
- **Live link:** optionally re-import when the source SVG changes, keeping rigging and animation attached.
- **Animated SVG export** (SMIL/CSS) and **Lottie export** going the other way (see export below).

### 9. Lip sync (P6)

- **Rhubarb Lip Sync built in.** [Rhubarb](https://github.com/DanielSWolf/rhubarb-lip-sync) is an open-source (MIT-licensed) tool that analyses dialogue audio and outputs mouth shapes over time. Fanimate bundles it, so automatic lip sync works offline with no setup.
- **Mouth sets:** Rhubarb's standard set of mouth shapes (based on the classic Preston Blair chart) maps onto a switch layer or a Smart Bone action; users can add extra shapes.
- **Script-assisted accuracy:** paste the dialogue text and the analysis aligns sounds more precisely.
- **Edit by hand:** results appear as editable keys on a dedicated lip-sync track with the waveform and phoneme labels shown.
- **Optional AI upgrade:** a higher-accuracy model and emotion-aware mouth shapes through the AI service.

### 10. Interface (P15)

- **Moho Pro layout by default, fully dockable** with saved workspaces (Rigging, Drawing, Animation, Compositing), plus floating panels for second monitors.
- **Menus in the title bar:** the menu bar shares the window's title bar (as in VS Code and other modern editors), so no vertical space is spent on a separate menu row. Windows and Linux get a custom title bar with its own window buttons; macOS keeps its native traffic lights.
- **Right-click menus everywhere (P23):** on layers, the canvas, timeline rows and keyframes, with the same commands and shortcuts as the main menus. Right-clicking an unselected item selects it first, as in every other editor.
- **Layers that behave like other apps (P23):** click, Ctrl-click and Shift-click selection; drag and drop to reorder or nest; double-click or F2 to rename in place; one-click visibility; cut, copy, paste and duplicate layers with the usual shortcuts.
- **Properties in a panel, not dialogs:** layer and project settings are edited live in a docked Properties panel instead of Moho's blocking Layer Settings and Project Settings dialogs.
- **Sharp on every screen:** the GPU-drawn UI scales cleanly on 4K and high-density displays; adjustable font size and contrast.
- **Command palette:** press a key, type any command or setting, with no hunting through menus.
- **Keymaps (P24):** two keymaps ship from day one, chosen on first launch and switchable any time:
  - **Fanimate (default):** the conventions of mainstream editing apps. Ctrl+C / Ctrl+X / Ctrl+V on anything selected, including layers; Ctrl+Z undo; Ctrl+Y and Ctrl+Shift+Z redo; Delete, F2 rename, Ctrl+D duplicate; tool letters from Adobe Animate and Illustrator.
  - **Moho:** Moho Pro's own shortcuts, so Moho users keep their muscle memory.
  Harmony-style keymaps and full remapping follow later; every menu shows the active keymap's shortcuts.
- **Pen and touch first:** radial menus, gestures and on-canvas controls for tablet users.
- **Built-in learning:** interactive tutorials inside the app and contextual hints the first time a tool is used.

### 11. Export (P19)

| **Target** | **Formats** |
|---|---|
| Video | MP4 (H.264, H.265), ProRes with alpha, WebM, AV1, GIF, PNG/EXR image sequences |
| Web and apps | Lottie / dotLottie, animated SVG, HTML5 canvas |
| Game engines | Spine-compatible JSON, sprite sheets, plugins for Godot, Unity and Unreal |
| Editing and compositing | Layered EXR render passes, After Effects / Natron / Blender project export |
| Interchange | Open .fnm format, PSD, SVG, OpenTimelineIO for edit decisions |

## AI tools (P8)

AI in Fanimate assists the animator; it never replaces the artist's control or takes ownership of their work. It runs in a hybrid way: small models ship with the app and run locally, while heavy generation uses cloud credits or the user's own API keys.

| **Tool** | **What it does** | **Runs** | **Default** |
|---|---|---|---|
| Inbetween assist | Suggests drawings between two key drawings (frame-by-frame) or smooths motion between two poses (rigs) | Local | Off; user enables |
| Auto-rig | Detects head, torso, limbs on imported art and places bones and weights | Local | On request |
| Colour assist | Fills frames where colour-mark tracking is unsure | Local | On request |
| Lip sync plus | Higher-accuracy phonemes and emotion-aware mouth shapes | Local or cloud | Rhubarb used by default |
| Line clean-up | Turns rough sketches into clean vector lines | Local | On request |
| Pose from reference | Poses a rig from a photo or video frame (motion reference) | Local or cloud | On request |
| Background and prop generation | Creates layered, editable art from a prompt | Cloud or own keys | Off |
| Script to animatic | Turns a script into rough boards and timing | Cloud or own keys | Off |
| Command assistant | "Make the walk heavier" → adjusts keys; explains any tool | Cloud or own keys | Off |

**Ground rules**

- **Off by default** for anything generative. Nothing is uploaded unless the user starts a cloud job.
- **Bring your own keys:** users can plug in their own model provider or local server instead of buying credits.
- **Editable output only:** every AI result becomes normal Fanimate layers, keys and bones the artist can change.
- **Labelled:** AI-generated assets are tagged in the file so studios can track what was generated.
- **Training data:** user projects are never used to train models unless the user explicitly opts in.

## Extension marketplace and scripting API (P2, P18)

Fanimate replaces Moho's scattered Lua scripts with a proper API and an in-app marketplace, like VS Code extensions or Blender add-ons.

### Scripting API

- **Python** for studio pipelines and automation (the language used by Maya, Blender and Harmony pipelines), and **TypeScript** for UI panels and lightweight tools.
- **WASM plugins** (compiled from Rust, C++ or other languages) for fast custom tools, brushes, effects and importers.
- **Full access:** the API exposes everything the UI can do (files, folders, layers, bones, keys, renders, the node graph), so no system-call workarounds are needed.
- **Headless mode:** run Fanimate from the command line to batch-render, convert files or run checks on a server.
- **Stable and versioned:** API versions follow semantic versioning, and deprecated calls keep working for at least two major releases.

### Marketplace

- **What's sold or shared:** scripts and tools, brushes, rigs and character templates, text and title templates, effect presets, node-graph setups, keymaps, and tutorials.
- **Free and paid listings.** Creators set their own price; the marketplace takes a small cut (proposed 15%) that funds hosting and security review.
- **Safety:** plugins run in a sandbox with permissions the user approves (e.g. "read files in this project", "use the network"). Listings are signed and scanned; ratings and reviews show quality.
- **Updates:** one-click install and automatic updates, with the option to pin a version for a production.
- **Self-hosting:** studios can run a private marketplace for in-house tools.

## Collaboration and cloud rendering (P5, P17)

Fanimate works for a solo artist on a laptop and for a 30-person studio, using the same files.

### Collaboration

- **Fanimate Hub (open source, self-hostable).** A project server that stores scenes, rigs and assets with version history, locks and permissions. It is the open equivalent of Harmony Server. Studios run it on their own machine or use the hosted version.
- **Shared asset library:** a character rig updated once updates in every scene that uses it, with an option to pin a scene to an older version.
- **Review and approval:** reviewers watch renders in the browser, draw notes on frames, and approve or request changes; notes appear on the animator's timeline.
- **Task tracking:** each shot carries a status (layout, animation, clean-up, colour, comp, approved) and an owner; it connects to existing trackers such as ShotGrid or Kitsu through plugins.
- **Live co-editing (later phase):** two people working in the same scene, on different characters, see each other's changes in real time.

### Cloud rendering

- **One-click cloud render:** send a scene to Fanimate's render service and keep working; get a notification and download link when it's done. Paid per render-minute with credits.
- **Bring your own farm:** because the engine runs headless, studios can render on their own machines or common render managers (Deadline, Flamenco, OpenCue).
- **Network render on a LAN:** spare office computers can join a local render pool for free.
- **Identical results:** the cloud runs the same renderer as the desktop app, so frames match exactly.

## Migration and interoperability (P19, P20)

Switching must cost almost nothing, so Fanimate opens the files people already have. This matters most for Animate users, who now face a product with no new features planned.

| **Source** | **What imports** | **Priority** |
|---|---|---|
| Moho (.moho, .mohoproj) | Layers, vector shapes, bones, Smart Bone actions, switch layers, keyframes, audio, camera | Version 1.0 |
| Adobe Animate (.fla, .xfl) | Symbols, timelines, vector drawings, classic and shape tweens, audio | Version 1.0 |
| SVG | Full structure and styles (see SVG section) | Version 1.0 |
| Photoshop (.psd) and Krita (.kra) | Layers, groups, blend modes, clipping, names; ready to rig | Version 1.0 |
| Toon Boom Harmony (scene export) | Drawings, deformers as bones, timing | Version 1.x |
| Lottie (.json, .lottie) | Shapes and animation | Version 1.x |
| OpenToonz, Synfig, Spine | Drawings, bones, keys | Community plugins |

**Migration help:** a "Coming from Moho / Animate" guide, matching keymaps, and a side-by-side glossary (e.g. Moho terms are used unchanged; Animate "movie clip" = Fanimate "symbol"; Harmony "deformer" = Fanimate "curve deformer").

**Legal note:** importers are built from public documentation and the files' own contents, never from the original programs' code. Fanimate follows Moho Pro's layout and workflow but not its code, icons, artwork or trademarks. Copying an interface's look still carries some trade-dress and patent risk, which was the main concern commentators raised about ArtCraft, so a legal review before 1.0 is required.

## Business model, licensing and governance (P21, P22)

The app is free forever; money comes from optional services, following ArtCraft, whose apps are free while it sells AI-generation credits to fund development.

### Licensing

- **App and file format:** dual MIT / Apache 2.0, the same permissive licences ArtCraft uses. Anyone can use Fanimate commercially, modify it, or build on it.
- **Your work is yours:** no watermark, no feature locks, no account needed to use the desktop app.
- **No paid upgrades:** every version is free, ending Moho's upgrade fees and serial-number restrictions.

### Revenue streams

| **Stream** | **What users pay for** | **Why it's fair** |
|---|---|---|
| Cloud render credits | Rendering on Fanimate's servers | Costs real compute; local rendering stays free |
| AI credits | Cloud AI generation and premium models | Costs real compute; local AI and own keys stay free |
| Fanimate Hub hosting | Hosted project server, storage, review links for teams | Self-hosting stays free |
| Marketplace fee | A cut of paid extension sales | Funds review and hosting |
| Studio support | Support contracts, training, custom features | For studios that need guarantees |
| Education | Free for schools; paid certification and course content | Grows the talent pool (P20) |

### Governance

- **Public roadmap and issue tracker** on GitHub; users vote on priorities.
- **Two release trains:** a feature release about every 4 months and a stability release about every month, so polish never waits for new features (P16).
- **Bug bounties** and contributor credits, as ArtCraft offers.
- **Steering group** of animators, studio technical directors and core developers, so decisions reflect working animators. This answers the old forum complaint that Moho's developers had little animation experience.

## Roadmap, risks and success metrics

Fanimate ships in four phases, each gated by a quality bar rather than a date, so it doesn't repeat Moho's unstable .0 releases. Phase lengths are estimates.

| Phase | Duration | Scope | Exit gate |
|---|---|---|---|
| **1. Foundation** | First planned at about 12 months; M0 to M5 took about a week, and the rest is the exit checks with testers (see the [Phase 1 schedule](roadmap/phase-1.md#41-schedule)) | Engine and renderer; drawing and frame-by-frame; rigging and lip sync; SVG and PSD import; Moho and FLA import; video export; Windows, macOS, Linux and web | **Phase 1 close-out**, with no public release. Exit checks of the first milestones passed with testers; closed alpha continues. *(Changed 2026-10-30: Gate 1 moved to the end of Phase 2.)* |
| **2. Production** | About 6 months, then a public beta of at least 8 weeks | Node graph; text animators; auto colour fill; raster brushes; scripting API; local AI tools; Lottie and game export; films, examples and handmade templates made before launch | **Gate 1: version 1.0 and the public launch** <br> Performance budget met <br> 99.5% crash-free public beta <br> 90% of test Moho files open <br> Scripting API frozen <br> A flagship short film and a body of films and examples released <br> Templates made by hand <br> *(The old Gate 2 criteria are merged in; see [release-1.0.md](roadmap/release-1.0.md).)* |
| **3. Studio** | About 6 months | Fanimate Hub; review and approvals; cloud render; marketplace; cloud AI credits | **Gate 3: studio-ready** <br> Studio pilots on Hub <br> Plugin sandbox audited |
| **4. Reach** | Ongoing | Tablet apps; live co-editing; Harmony import; education and certification | — |

Phase 1 alone must replace Moho for a solo rigging animator; later phases add what studios and frame-by-frame teams need.

### Risks

| **Risk** | **Impact** | **Mitigation** |
|---|---|---|
| Too broad a scope for an early team | Nothing ships well | Phase gates; rigging and drawing first, everything else later |
| AI-built code quality (ArtCraft's early users reported basic bugs) | Loss of trust | AI-assisted coding, but human review, automated tests and performance benchmarks on every change |
| Patent or trade-dress claims | Legal cost | Own code, icons and name; Moho layout reviewed by a lawyer; importers from public specs; legal review before 1.0 |
| Studios won't switch without a track record | Slow adoption | Free studio pilots; showcase short films; Moho/Animate importers |
| Funding services don't cover costs | Development slows | Several revenue streams; grants from open-source and arts foundations |
| Browser version too slow for big scenes | Poor first impression | Position web as learning and review; full power on desktop |

### Success metrics

| **Metric** | **Target 12 months after 1.0** |
|---|---|
| Monthly active users | 100,000 |
| Moho and Animate files imported | 50,000 |
| Studios using Fanimate Hub in production | 50 |
| Marketplace extensions | 500 |
| Crash-free sessions | 99.5% or higher |
| Median time for a new user to rig a character | Under 30 minutes |
| Schools teaching with Fanimate | 200 |

## Appendix: pain point to solution matrix

Every pain point has at least one owning feature and a phase.

| **ID** | **Pain point** | **Fanimate solution** | **Phase** |
|---|---|---|---|
| P1 | Not cross-platform | Rust + wgpu; Windows, macOS, Linux, browser; tablets later | 1 |
| P2 | No extension marketplace | In-app marketplace with sandboxed, signed plugins | 3 |
| P3 | Weak text animation | Per-character animators, template gallery, auto-captions | 2 |
| P4 | No pipeline editor | Optional node graph synced with the layer list | 2 |
| P5 | No cloud rendering | One-click cloud render; headless engine for own farms | 3 |
| P6 | Basic lip sync | Bundled Rhubarb Lip Sync, script-assisted, editable track | 1 |
| P7 | Rigging needs improvement | Sturdier Smart Bones, templates, auto-rig, retargeting, rig health check | 1 |
| P8 | No AI tools | Hybrid local/cloud AI suite, off by default for generation | 2–3 |
| P9 | Poor frame-by-frame drawing | Pressure-true brushes on any frame, X-sheet, onion skins | 1 |
| P10 | Weak bitmap tools | Krita-class raster brushes in the same engine | 2 |
| P11 | No auto colour fill | Colour marks tracked across frames, gap-tolerant bucket | 2 |
| P12 | Slow with complex scenes | GPU renderer, change-only evaluation, performance budget | 1 |
| P13 | Hidden corrupting bugs | File validator and self-repair on every save | 1 |
| P14 | Crashes and unstable releases | Journal autosave, phase quality gates, stability release train | 1 |
| P15 | Dated UI | Docking workspaces, menus in the title bar, HiDPI, command palette, keymaps | 1 |
| P16 | Polish neglected | Separate stability releases; public bug voting | All |
| P17 | No collaboration | Fanimate Hub, asset library, review and approvals | 3 |
| P18 | Limited scripting | Python, TypeScript and WASM API; headless mode | 2 |
| P19 | Poor SVG import and interop | Full SVG importer, Lottie/game/web export, broad importers | 1–2 |
| P20 | Small industry footprint | Free education programme, certification, studio pilots | 3–4 |
| P21 | Licensing friction | Free MIT/Apache licence; no paid upgrades | 1 |
| P22 | Slow, closed development | Open source, public roadmap, steering group | 1 |
| P23 | Awkward layer handling, no right-click menus | Context menus everywhere, standard layer selection, drag and drop, rename in place, Properties panel instead of modal dialogs | 1 |
| P24 | Unconventional shortcuts | Fanimate (standard) keymap by default, Moho keymap one click away | 1 |

### References

- [ArtCraft coverage (PetaPixel)](https://petapixel.com/2026/10/07/someone-rebuilt-free-open-source-versions-of-photoshop-premiere-and-lightroom-using-ai/)
- [ArtCraft platforms and business model (IT Pro Expert)](https://itproexpert.com/goodbye-adobe-new-free-open-source-adobe-looking-alternatives-for-windows-mac-and-linux/)
- [ArtCraft licences and parity status (Mezha)](https://mezha.ua/en/news/artcraft-adobe-photoshop-clone-315969/amp/)
- [ArtCraft early issues (Android Headlines)](https://www.androidheadlines.com/?p=1317688)
- [Adobe Animate maintenance mode (PCWorld)](https://www.pcworld.com/article/3050939/end-of-an-era-adobe-animate-is-being-put-to-rest-after-30-years.html)
- [Toon Boom Harmony 25 system requirements](https://docs.toonboom.com/help/harmony-25/premium/system-requirements/harmony-25.html)
- [Harmony 25.1 release notes (Ember AI)](https://docs.toonboom.com/help/harmony-25/essentials/release-notes/harmony/harmony-25-1-release-notes.html)
- [Moho pricing and upgrades](https://moho.lostmarble.com/upgrade)
- [Moho forum (FBF tool limitations)](https://www.lostmarble.com/forum/viewtopic.php?p=212056)
- [Moho forum (performance issues)](https://www.lostmarble.com/forum/viewtopic.php?p=201373)
- [Moho forum (negative keyframe bug)](https://lostmarble.com/forum/viewtopic.php?p=196931)
- [Moho forum (UI overhaul request)](https://www.lostmarble.com/forum/viewtopic.php?p=192523)
- [Moho forum (pipeline and Lua limits)](https://www.lostmarble.com/forum/viewtopic.php?p=213392)
- [Moho forum (SVG and Lottie)](https://www.lostmarble.com/forum/viewtopic.php?p=204470)
- [Moho forum (auto-colour request)](https://lostmarble.com/forum/viewtopic.php?p=194620)
- [Moho alternatives (FitGap)](https://us.fitgap.com/products/009152/moho/alternative)
- [Rhubarb Lip Sync](https://github.com/DanielSWolf/rhubarb-lip-sync)
