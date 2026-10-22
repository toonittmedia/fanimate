# UX pass: tabs, Home, handles, drawing tools, snapping

Companion to [Phase 1](phase-1.md). This is a planned set of changes, agreed with the project owner on 2026-10-18. Phases A to F are built and ship in 0.2.2; the items still marked open below are not. A second pass shipped in 0.2.3 and is summarised in [UX pass 2](#ux-pass-2-0-2-3) at the end. Move finished items into the Phase 1 status when they ship.

Reference material: screenshots of Vector Studio (ves.puter.com) kept in `reference/` for layout and interaction only. Clean room rules apply: we follow the interaction model, never code or artwork.

## Decisions

| Topic | Decision |
|---|---|
| Visual style | Flat, dense, hairline dividers, no panel shadows. Keep the Fanimate purple accent from `brand/tokens/tokens.json`; do not adopt the reference's blue. |
| Shadows and tooltips | No shadows on panels, tooltips or menus. 1 px border instead. Floating popups over the canvas may keep one very subtle shadow so they stay readable. Tooltips are compact, 4 px radius, label plus shortcut. |
| Keyframe types | Smooth (circle), Linear (diamond), Step (square). Maps to the existing `Interpolation`; no format change. |
| Documents | Several open at once, as tabs. Each tab is a full `Editor` (undo, selection, frame, view). Tool, keymap and panel layout are shared. |
| File container | `.fnm` becomes one file (a zip of the current folder layout). Folder projects still open. OS file icon and double-click open on Windows, macOS and Linux. |
| Startup | Preferences choose: Home tab, reopen last session (all tabs), new blank document, or open the most recent file. |
| Home | A pinned first tab, not a modal. Left: New file, Open, Home, Recent, Your files, Templates. Right: preset cards (sizes and frame rates). |
| Handle colour | Selection boxes, handles and the rotation button use the primary (last clicked) layer's label colour. |
| Pivot | Per-layer, draggable on the canvas, snaps to nine box anchors, editable in Properties, keyable. |
| Corner radius | Rectangles are parametric: radius handles, Properties field, Absolute and Relative modes, Expand Shape converts to a path. Polygons and stars get the same. |
| Rotation | A Canva-style round rotate button below the box, plus rotation by dragging just outside a corner (rotate cursor). Live angle readout, Shift snaps to 15 degrees. |
| Selecting | Click inside a shape or on its stroke selects the whole shape. Double-click or Alt+click enters point editing. Marquee selects shapes it touches. |
| Curvature | A new tool beside Pen: click adds points along a smooth curve, Alt+click makes a corner, Esc finishes, a live ghost segment shows the next piece. Status bar shows the hints. |
| Drawing tools | See [Tools](#tools). |
| Snapping | Rulers with guides, grid, snap to grid, guides, canvas bounds, pixels and angles (snapping to shape edges and centres comes later). |
| Timeline range | The playing range (start to end) is marked by a clear boundary line on its left edge at the start frame, and the range is shaded. The ruler and rows keep going past the end into the future, dimmed, as in Moho. The end frame is editable from the transport bar (the "of 47" number). |
| Timeline zoom | Zoom in, zoom out and "reset zoom" buttons in the timeline header, plus a fit-all button, besides Ctrl+scroll. |
| Keyboard zoom | Ctrl+Plus, Ctrl+Minus and Ctrl+0 never scale the UI. They zoom the canvas (or the timeline when it has focus). |

## What the reference screenshots teach

Behaviour worth matching, beyond what was asked for:

- **Two kinds of marker.** The selection box uses hollow squares (corners and edge midpoints). Path anchors are filled dots, on-curve points square and filled, handles round. Never use one shape for both, so the user can tell "move the shape" from "edit the path" at a glance.
- **Multi-select** draws one union box with handles, plus a thin outline around each selected object. The union box takes the primary layer's label colour; each object's own outline uses that object's label colour.
- **Unselected shapes** show a thin neutral grey outline on hover, so the user sees what a click would pick.
- **Floating contextual toolbar** below the selection, draggable by its grip, with actions that depend on what is selected: a single path shows Edit Path and Offset Path; several objects show Group, Align and Unite; both show fill, stroke, lock and a more menu. Every button is an `Action`, with a context menu entry and shortcut as usual. It can be hidden from View.
- **Status bar hints** that change with the tool and modifier keys ("Click to add points along a smooth curve | Alt+Click for a corner | Esc to finish").
- **Properties panel** edits Transform (X, Y, W, H, rotation, 3x3 anchor grid, link ratio), shape properties (radius, rounding mode, Expand Shape) and Appearance (fill, stroke, opacity, blending), live.
- **Tool groups with a corner triangle** for variants (Shape tools, Pen variants), shortcut in the tooltip.
- **Rulers** with units, a zero-origin marker, and a cursor position line.

## File format changes (one bump, version 4, with the first new field)

All additive, so one version bump covers the new fields of every phase below. The first change to what the files hold (the pivot, in Phase D) bumps the version to 4; the container alone does not. Spec in [`docs/spec/fnm.md`](../spec/fnm.md) first, then migration, validator and round-trip tests.

| Change | Phase |
|---|---|
| Single-file container (zip) with the folder layout inside; atomic write via temp file and rename; folder projects still read. **No version bump**: the files inside are unchanged, and an older reader refuses a single file as "not a project" rather than misreading it. | C |
| Layer `pivot` (animatable point channel, defaults to the layer centre) | D |
| Parametric shape kinds: rectangle, polygon, star, line, triangle, with corner radii | D, E |
| `guides` in `document.json`, and grid settings (spacing, subdivisions) | F |

Readers refuse versions newer than they know. Files from every released version must still open.

## Phases

Each phase ends with fmt, clippy `-D warnings`, the workspace tests and a check of the running app, then a check-in before the next starts. No commits unless the owner asks.

### Phase A: Look and feel

- [x] Theme pass: no window shadow, 4 px corners, dock separators are 1 px hairlines in the border colour (`theme.rs`, `dock_style`). Tighter row heights are left to the later panel work, to be judged in the running app.
- [x] Tooltips: 4 px radius, 1 px border, one faint shadow instead of egui's heavy one. The transport and zoom buttons already show label plus shortcut.
- [x] Menus and popups: 1 px border, minimal shadow (same popup style)
- [x] Keyframe shapes on the timeline: circle, diamond, square. Already in the app; set from the key's context menu.
- [ ] Floating contextual toolbar: moved to Phase D. A toolbar with no actions would be placeholder UI, so it ships with its first real buttons.
- [x] Keyboard zoom: egui's UI scaling on Ctrl+Plus, Ctrl+Minus and Ctrl+0 is off (`zoom_with_keyboard = false`). Those chords zoom the canvas, or the timeline while the pointer is over it. Checked in the running app: the UI keeps its size, the canvas and timeline zoom.
- [x] Timeline range: boundary line on the left edge of the start frame; the end line stays visible but no longer sits under the playhead; shade the range; dim the ruler and rows beyond the end (`panels/timeline.rs` currently stops the ruler at `end`, and rows and scrolling are clamped to it)
- [x] Timeline continues into the future: the ruler and scrolling run past the end frame, dimmed, and the transport shows "Frame 47 of 47" with the total as an editable field that sets `Document::end` (one undo step, never before `start`).
- [x] The playhead goes past the end (0.2.3): scrubbing is free, playback still loops the playing range.
- [x] Timeline zoom controls: zoom in, zoom out, reset (back to 12 px per frame) and fit-all buttons in the header, with tooltips showing the shortcut; Ctrl+scroll stays; zoom keeps the playhead position fixed on screen; ruler labels thin out at low zoom and show every frame, with sub-ticks, at high zoom (second reference image)
- [x] Timeline zoom and scroll are per tab (Phase B)

### Phase B: Tabs, Home, startup

- [x] The live document stays in `Shell::editor`; the others are parked in `Shell::docs` (`fanimate_editor::Tabs`, with tests). Each tab keeps its own undo, selection, frame, canvas view, timeline zoom and scroll, and crash journal. The tool and brush are shared.
- [x] Recovery journals every tab. After a hard kill with two unsaved documents in two tabs, both were offered and both came back as tabs; a recovered file replaces the clean copy of itself.
- [x] Tab strip under the title bar: open (New, Open, samples, presets, double-click in the file manager), close (x, Ctrl+W), reorder by dragging, Ctrl+Tab and Ctrl+Shift+Tab, a dot for unsaved changes, a context menu (Close, Close Other Tabs, Save, Save As), a new-file button. Closing asks about the one document; quitting or updating asks once about all of them ("Save All").
- [x] Home tab: New file, Open, Import, Home (six presets and recent files), Recent (with a context menu), Templates (the three samples). It is a page in front of the documents, not a dialog.
- [x] Startup choice (Home, reopen last session, new blank file, most recent file) in Preferences and on the Home tab.
- [x] Opening files from the command line or the file manager opens tabs in the running Fanimate (`instance.rs`: lock file and inbox; a launch nobody answers carries on by itself after 3 seconds).
- [x] Timeline zoom and scroll are per tab.
- [x] Recent and Templates show previews (0.2.3).
- [ ] Not built: "Your files" on Home (a list of the folders you keep projects in); templates beyond the three built-in samples.
- [ ] Not checked by hand: middle-click close and Close Other Tabs (the logic is tested without a window).

### Phase C: File container and OS handlers

- [x] `.fnm` is one file (ZIP, deflated, a checksum for every entry, the same bytes for the same project). The reader and writer live in `fanimate_fnm::zip` (the importers use them too). Folder projects still open, and stay folders when saved; `save_folder` writes a folder (the journal and the render queue use it).
- [x] Atomic save: written beside the old file, read back and checked, flushed, then renamed over it. Tests: a failed save leaves the old file untouched and no stray files; damaged files (cut short, changed anywhere) never panic; the damaged-projects test now also damages 1500 single files; a project folder zipped by hand opens; newer versions and non-projects are refused.
- [x] Spec updated (`docs/spec/fnm.md`: layout, the single file, the version note); the web player opens `.fnm` files.
- [x] Windows installer: `.fnm` association, file icon and open verb (current user, or all users when run as administrator). Not run here: no Inno Setup on this machine.
- [x] macOS `Info.plist`: document type, the `app.fanimate.project` type and the file icon; `package.sh` copies the icon. Not built here.
- [x] Linux: MIME type `application/x-fanimate-project` and icon, `MimeType` in the desktop file, `%F` so several files open, the metainfo `provides`; AppImage, `.deb` and Flatpak all install them. Not built here.
- [x] Save As Folder is in the File menu (0.2.3).
- [ ] Not checked: on macOS, Finder hands a file to the running app as an Apple event rather than starting a second process; whether this version of winit passes that on has not been checked, so opening a file from Finder into a running Fanimate may need more work there.

### Phase D: Selection, handles, pivot

- [x] Whole-shape selection with Select Points: click a shape's fill or stroke to select all of its points (a shape in another layer selects that layer first); click a point for just that point; Shift or Ctrl adds. Pressing on a shape's body and dragging moves it, choosing it first if needed. A selection box picks every shape it touches (Alt: only the points inside). The shapes chosen are outlined in the layer's label colour, and the shape under the pointer gets a thin grey outline.
- [x] Selection frame for the Transform tool: the box in the layer's label colour, hollow square handles (corners; edges too where the sides can stretch on their own), per-layer outlines under one union box for several layers. Corner and edge drags scale about the opposite side (Alt: from the centre, Shift: proportions) with the new size shown beside the pointer. Several layers scale and turn together; a stretch a turned layer cannot take (it would shear) becomes an even scale.
- [x] Rotation: a round button below the box on a short stem, and the zone just outside each corner; angle shown beside the pointer; Shift steps by 15 degrees. A layer with no pivot turns about the middle of its box, without moving.
- [x] Pivot: a ring on the canvas that can be dragged (it snaps to the box's nine anchors), a 3 x 3 anchor grid and X and Y fields in Properties. It is stored per layer (`transform.pivot`, format version 4) and used by rotate and scale. **Not animated**: moving it keeps the artwork where it is on every frame unless the layer's rotation or scale is animated, when it only holds on the current frame (the status bar says so).
- [x] Rectangle: the Rectangle tool makes a rectangle that remembers it is one (`rect`, version 4); round dots inside the corners change the radius, Properties has a Corner radius field with Absolute and Relative, and Expand Shape (an action, in menus, the palette and the toolbar) turns it into a plain path. Editing a point by hand does the same quietly. One radius for all four corners.
- [x] Floating toolbar below the selection (above when there is no room): Edit Points or Transform, Group (several layers), Unite (when points on two shapes are chosen), Expand, fill and stroke colours, Duplicate, Delete, and a menu with the layer commands.
- [x] 0.2.3: the toolbar drags by a grip; Align and Lock exist as Actions.
- [ ] Not built: Offset Path; Alt+click or double-click to "drop into point editing" (Select Points already works on points, and the Transform tool on layers, so there is nothing to drop into); separate radii per corner; the rotate cursor (egui has none, so the crosshair shows); a pivot that is keyed (these three need format fields and stay open).
- [ ] Not measured: the frame needs one evaluation per frame with the Transform tool and a selection (the old outline already needed one, and is gone for that tool), and hovering is cached until the pointer, the document or the view changes. No before and after numbers were taken.

### Phase E: Keys and tools

- [x] Curvature tool: click adds Catmull-Rom smooth points, Alt+click adds a corner, Esc finishes. Live ghost segment in the canvas preview. Status-bar hint. `Tool::Curvature`, bound to `C` in both keymaps.
- [x] Shape tools: Line (`L`), Polygon (`Shift+L`), Star (`Shift+O`), Triangle (`Shift+R`). Drag to draw; scroll wheel adjusts sides / points while dragging; Alt adjusts star inner radius. Path builders in `fanimate-core` (`Path::line`, `Path::polygon`, `Path::star`, `Path::triangle`). All tested.
- [x] Editing tools: Delete Edge (click a segment, splits into two open paths), Magnet (attract / Alt=repel points in radius), Scale Points (drag from centroid), Rotate Points (drag angle from centroid, Shift snaps 15°). All bound in both keymaps and working via canvas.
- [x] Fill and stroke tools: Draw Shape (click copies style), Freehand with Width (variable pressure via drag-Y), Line Width (drag adjusts widths profile). All bound. Stroke Exposure works since 0.2.3.
- [x] Shape Builder (`Shift+M` Fanimate and Moho): drag across overlapping regions to unite them; Alt+drag to subtract. Uses the existing `combine` / `BoolOp` backend. Tested.
- [x] Each tool: `Tool` variant, `label`, `group`, `hint`, `is_available`, keymap bindings in both presets, an SVG icon in `icons/fanimate/`, wired in `icons.rs`. 13 new tests in `tools_e.rs` (290 total, all pass).
- [x] Line Width and Draw Shape were rebuilt in 0.2.3 (drag up or down on a point; freehand closed shape).
- [x] Stroke Exposure (0.2.3): `trim` and `trim_keys` on a shape (format version 5), drag along a stroke or use Properties; keyed like any property.

### Phase F: Rulers, grid, snapping

- [x] Rulers along the top and left edges of the canvas, with px tick marks, sub-ticks, a cursor-position line, and units at every major tick. Shown/hidden from View > Rulers.
- [x] Draggable guides: drag from the horizontal ruler for a vertical guide, drag from the vertical ruler for a horizontal guide. Right-click a guide on the canvas to delete or lock it (locked guides cannot be accidentally dragged). Guides are stored in the document (`Document::guides`, format version 5).
- [x] Grid with configurable spacing and subdivisions, drawn under the artwork in the canvas (View > Grid). Major lines use the border colour; minor lines are dimmed. The grid hides when zoom would make it too dense (< 4 px between minor lines).
- [x] Snap module (`fanimate-editor/src/snap.rs`): pure functions, 6 unit tests. Targets: pixel grid, document grid, guides, canvas bounds (edges and centre), with `SnapResult` carrying a hint line. Angle snapping helper `snap_angle`. `SnapSettings` and `GridSettings` are serialised and restored between sessions.
- [x] View > Snapping sub-menu: per-target toggles (pixels, grid, guides, bounds), the angle step used while Shift is held, grid spacing and subdivisions, all live-editable without closing the menu.
- [x] Snapping to other shapes' points, edges and centres (0.2.3), on both axes at once. Guides are saved in the project and can be dragged.
- [ ] Not built: snapping for Freehand, Eraser and the selection handles.
- [x] Snap hint line on the canvas while dragging (drawn in a semi-transparent accent colour after the GPU texture; cleared at the start of each frame so it vanishes when no drag is active).
- [x] Tool integration: `do_snap()` is called by every tool that drags a position: Move Layer, Select/Translate Points, Add Point, Pen, bone drag (AddBone/SelectBone/Pose), Rectangle, Ellipse, Line, Polygon, Star, Triangle. Shape drag `to` endpoints are snapped on each frame.
- [x] Pixel snapping: the pixel-snap candidate is produced by the snap module and the canvas tools call it via `do_snap()`.
- [x] Angle snapping for bones: Shift+AddBone snaps the bone angle to multiples of `snap.angle_step` degrees after position snapping. The `snap_angle` helper is used.
- [x] Units other than px (pt, mm, in) in the ruler: the corner button cycles through `RulerUnit` variants (px → pt → mm → in → px); tick labels convert via `px_to_unit()` at the configured DPI.

## Open items for the owner

- Handle and outline colours in multi-select are inferred from the screenshots (union box in the primary colour, per-object outlines in their own). Say if you want one colour only.
- Moho's shortcuts for the new tools are not decided. Moho's own bindings will be used where the tool exists in Moho.
- Timeline boundary: the owner's note was that the border should be on the left, not the right. The plan draws the start boundary on the left edge of frame 0 and keeps a quieter end line on the right where the dimmed future begins. Say if the right-hand line should go entirely.
- Hit order when shapes overlap (topmost first, Alt+click to cycle through those underneath) is assumed.
- Templates on the Home tab need at least a few built-in templates; none exist today.

## Risks

- **Tabs** touch every place that reads `shell.editor`. Do it early, behind one accessor, so later phases build on it.
- **Parametric shapes** touch tessellation, point editing, boolean operations and the importers. Expand Shape is the escape hatch; every consumer must handle both forms.
- **Zip container** must keep the "never lose work" guarantee: write a temp file, verify it reads back, then swap it in.
- **Phase E is large.** If it slips, ship Curvature, the shape tools and Shape Builder first; mark the rest "coming in a later release" per the polish rule.

## UX pass 2 (0.2.3)

Built: Stroke Exposure (trim with keys), a radius for each corner, snapping for Freehand and Draw Shape starts and scaling handles, the macOS update install on a worker thread; What's New in the app; Preferences, Export, Shortcuts, About and Update as modal dialogs with a shared layout; links that open the browser; non-blocking update with progress; the canvas outside the dock with three fixed regions and History in the Layers area; the tools panel at three per row; interpolation colours and a dropdown for new keys; expandable property rows with their own keys; key buttons in Properties; Moho-style keying; a document-level camera with a timeline row, Properties fields, look-through and a frame overlay; colour animation for fill and stroke; the shapes list in the Style panel; layer lock, Align and the draggable toolbar; layer type conversion; guides in the document; rotated ruler labels; remembered Export and Save As folders; Render Preview Frame (Ctrl+R) and Import on Ctrl+I; audio import for MP3, OGG, FLAC and AAC; taskbar render progress on Windows. Format version 5.

Not built, still open (the status of each after [UX pass 3](#ux-pass-3) is on the right):

- [x] Recent thumbnails embedded in the `.fnm` (0.3.0: `thumbnail.png`, written on save, read before the app-data cache).
- [x] "Your files" on Home; more templates (0.3.0: folders added on the Home tab; Blank, Walk cycle rig, Lip sync, Frame by frame).
- [ ] A keyed pivot (a format field). Still open, see UX pass 3.
- [ ] Offset Path. Still open, see UX pass 3. Before and after numbers for the selection frame: not taken in 0.3.0 (no release build fits on the maintainer's disk), still open.
- [ ] Snapping for the Eraser (it would erase away from the pointer): still open, see UX pass 3. Snapping for rotating and the pivot: done in 0.3.0 (guides and shape points).
- [x] Camera drag on the canvas (0.3.0: the Track, Zoom and Roll camera tools).
- [ ] Moho keymap bindings for the new actions: only the layer clipboard actions are bound. Moho's manual does not list keys for the camera tools or Hide Edge (it lists only Translate Points on T), so they stay unbound rather than guessed.
- [x] Real MP3, OGG, FLAC and AAC fixtures in the tests: generated tones in `crates/fanimate-editor/tests/fixtures/audio/` (MP3, Ogg Vorbis, FLAC, AAC in M4A and in ADTS) with decode tests in `src/editor/sound.rs`. They pass.
- [ ] Video import: [design note](video-import.md) only. Still open.

## UX pass 3

Planned with the owner after 0.2.3; the plan is in the maintainer's planning notes and is summarised here. Everything below shipped in 0.3.0 and was checked in the running Windows build, except where it says otherwise (macOS and Linux were not tried; the pivot picker's click, Zoom and Roll camera, Alt+click to show a hidden edge, the Loop menu and the Order rows were covered by tests but not clicked through by hand). It ships as one format bump, **version 6**, so the new fields need only one migration. Files from every earlier version keep opening and look the same.

### Format version 6

| Change | Detail |
|---|---|
| 1-based frames | A document starts at frame 1 (a new one runs 1 to 120). Frame 0 is the rest pose: visible, selectable, and the place the keying rule seeds. Migration adds 1 to `start`, `end` and every key frame; it does not invent a frame 0 key for old files. |
| Loop | Each animation channel can be set to Off, Absolute or Relative. Absolute repeats the keyed cycle; Relative adds the cycle's net change on each repeat. Only meaningful with two or more keys. |
| Layer order | A stepped integer `Order` channel on layers inside groups, bone layers and switch layers. Siblings are drawn in the order of their sampled slot, so a layer can move in front of or behind its siblings over time. The Layers panel stays in storage order. |
| Thumbnail | An optional small PNG entry in the `.fnm` container, written on save and ignored by older readers. It is covered by the checksum list. |

The byte-level description belongs to [the format spec](../spec/fnm.md).

### Timeline

- Frame 0 shows in the ruler and the transport has a Rest Pose button (key `1` in the Fanimate keymap, unbound in Moho). First Frame and Home still go to frame 1. The transport reads "Frame 41 of 120".
- Seconds: a label at each second on the ruler, and "seconds:frames" in the transport, the status bar, Properties and the X-sheet.
- A monospace font (JetBrains Mono, OFL) for every frame number.
- The names column has a drag divider, per tab, with a double-click to reset; long names get an ellipsis and a tooltip, in the Layers panel too.
- The interpolation dropdown: each item is a full-width row, so hover reaches the panel edges and the icon is clickable.
- Preferences > Timeline: hide layers with no keys (off by default). Groups stay when a child is animated and selected layers always show. The Camera row hides when the camera has no keys.
- Key selection: click, Shift or Ctrl to add, a box drag across layers and channels, Ctrl+A, Esc. Add and Delete Key act on the selected keys, or on the playhead frame when none are selected. Dragging moves them all; Alt-drag copies.
- Scale: hover grips at the ends of the selection stretch the keys about the opposite end (Alt: about the playhead), in whole frames, as one undo step.
- Key copy and paste: Ctrl+C and Ctrl+V in the timeline keep layer and channel and put the earliest key at the playhead. In another tab, keys go to the selected layer's matching channels.
- Loop: right-click a channel or key and choose Off, Absolute or Relative. A tinted bar with ghost keys runs from the last key to the end.
- Order rows for layers in groups.

### Tools and canvas

- Tool groups follow the layer type: Transform, Camera and Workspace always show; Draw and Fill for vector layers; Bone for bone layers. If a layer change hides the active tool, the app switches to Transform.
- A Camera group with Track, Zoom and Roll Camera. Dragging on the canvas keys the camera by the keying rule, in one undo step.
- Point transform in the Moho style: with two or more points chosen, the Transform tool shows a frame with scale, rotate and a movable pivot, and edits go through the keying rule. The Scale Points and Rotate Points tools are removed.
- A pivot picker: a target button on the floating toolbar and in Properties; the next canvas click sets the pivot, snapping to the nine anchors (Shift for free). Esc cancels. With points chosen, it sets the points' pivot.
- Snapping for the rotate handle and the pivot, through guides and shape points.
- Freehand and Freehand with Width are one tool. **Pen pressure** is a checkbox in the Style panel (on by default): on, a pen's pressure sets the width; off, the stroke has an even width. A mouse has no pressure, so it draws at full width either way. The drag-up-or-down width trick is gone.
- **Hide Edge** (Shift+H): click a segment to leave its stroke undrawn, click it again or Alt+click to show it. The points and the fill stay. It is a flag per path command on the shape (`hidden_edge_flags`, format version 6); point insertion and removal, extending, closing and Delete Edge keep the flags in step, and flags that stop fitting the path are ignored (the edges show).
- **Saved styles**, one line under "Brushes and new shapes" in the Style panel: a drop-down of styles, a save button (updates the chosen style when the look was edited, otherwise saves a new one) and a menu with Save as new, Update, Rename, Delete, Set as default and Reset to default. A style holds line colour, width, fill, outline, smoothing, taper and pen pressure (not the eraser or weight brush sizes). A built-in Default style cannot be changed or deleted. Styles are kept by the app and saved in the project (`styles`, format version 6); opening a project adds the styles it has that the app does not. A default other than the built-in one is what a session starts with; without one the brush stays as the last session left it.
- Clipboard by focus: Ctrl+C, Ctrl+X and Ctrl+V copy points or shapes on the canvas and keys in the timeline. Layers use **Ctrl+Shift+C, Ctrl+Shift+X and Ctrl+Shift+V**; Ctrl+D still duplicates layers. A user who presses Ctrl+C on a selected layer gets a status hint. Shapes paste across tabs.

### Home, keymaps, tests

- "Your files" on Home: folders added on the Home tab, scanned for `.fnm` files and shown with their thumbnails. More built-in templates: blank, walk-cycle rig, lip sync, frame-by-frame.
- Moho keymap: the layer clipboard actions (Ctrl+Shift+C, X, V) are bound in both keymaps. The camera tools, Hide Edge, Go to Rest Pose and Pick Pivot are left unbound in the Moho keymap, because Moho's own keys for them are not documented anywhere we may take them from. The keymap test rejects duplicate chords.
- Audio fixtures: done (see the 0.2.3 list above).
- Before and after numbers (CLAUDE.md rule 3) for the selection frame with points, the timeline with a key selection and the hide filter, and evaluation with Loop and Order, from `fanimate-cli bench`. Pending.

### Not built in this pass

- A keyed pivot. The pivot is not animated; keying it needs a format field and a rule for what a keyed pivot does to the layer's look.
- Offset Path.
- Snapping for the Eraser. It would erase away from the pointer.
- Video import: [design note](video-import.md) only.

### Consequences to know about

- Ctrl+C on a selected layer no longer copies the layer; use Ctrl+Shift+C.
- Frame numbers shown to the user, in the CLI `render --frame` and in the web player, move by one.
