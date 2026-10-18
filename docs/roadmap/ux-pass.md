# UX pass: tabs, Home, handles, drawing tools, snapping

Companion to [Phase 1](phase-1.md). This is a planned set of changes, agreed with the project owner on 2026-10-18. Nothing here is built yet. Update the status table as each phase lands, and move finished items into the Phase 1 status when they ship.

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
| Snapping | Rulers with guides, grid, snap to grid, guides, points, shape edges and centres, canvas bounds, pixels and angles. |
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
- [ ] Still open: the playhead is still clamped to the playing range, so keys cannot yet be added past the end by moving the playhead there (dragging an existing key past the end works). Changing that touches playback and `Editor::go_to`; decide with the owner.
- [x] Timeline zoom controls: zoom in, zoom out, reset (back to 12 px per frame) and fit-all buttons in the header, with tooltips showing the shortcut; Ctrl+scroll stays; zoom keeps the playhead position fixed on screen; ruler labels thin out at low zoom and show every frame, with sub-ticks, at high zoom (second reference image)
- [ ] Timeline zoom and scroll are per tab once Phase B lands

### Phase B: Tabs, Home, startup

- [x] The live document stays in `Shell::editor`; the others are parked in `Shell::docs` (`fanimate_editor::Tabs`, with tests). Each tab keeps its own undo, selection, frame, canvas view, timeline zoom and scroll, and crash journal. The tool and brush are shared.
- [x] Recovery journals every tab. After a hard kill with two unsaved documents in two tabs, both were offered and both came back as tabs; a recovered file replaces the clean copy of itself.
- [x] Tab strip under the title bar: open (New, Open, samples, presets, double-click in the file manager), close (x, Ctrl+W), reorder by dragging, Ctrl+Tab and Ctrl+Shift+Tab, a dot for unsaved changes, a context menu (Close, Close Other Tabs, Save, Save As), a new-file button. Closing asks about the one document; quitting or updating asks once about all of them ("Save All").
- [x] Home tab: New file, Open, Import, Home (six presets and recent files), Recent (with a context menu), Templates (the three samples). It is a page in front of the documents, not a dialog.
- [x] Startup choice (Home, reopen last session, new blank file, most recent file) in Preferences and on the Home tab.
- [x] Opening files from the command line or the file manager opens tabs in the running Fanimate (`instance.rs`: lock file and inbox; a launch nobody answers carries on by itself after 3 seconds).
- [x] Timeline zoom and scroll are per tab.
- [ ] Not built: "Your files" on Home (a list of the folders you keep projects in); templates beyond the three built-in samples.
- [ ] Not checked by hand: middle-click close and Close Other Tabs (the logic is tested without a window).

### Phase C: File container and OS handlers

- [x] `.fnm` is one file (ZIP, deflated, a checksum for every entry, the same bytes for the same project). The reader and writer live in `fanimate_fnm::zip` (the importers use them too). Folder projects still open, and stay folders when saved; `save_folder` writes a folder (the journal and the render queue use it).
- [x] Atomic save: written beside the old file, read back and checked, flushed, then renamed over it. Tests: a failed save leaves the old file untouched and no stray files; damaged files (cut short, changed anywhere) never panic; the damaged-projects test now also damages 1500 single files; a project folder zipped by hand opens; newer versions and non-projects are refused.
- [x] Spec updated (`docs/spec/fnm.md`: layout, the single file, the version note); the web player opens `.fnm` files.
- [x] Windows installer: `.fnm` association, file icon and open verb (current user, or all users when run as administrator). Not run here: no Inno Setup on this machine.
- [x] macOS `Info.plist`: document type, the `app.fanimate.project` type and the file icon; `package.sh` copies the icon. Not built here.
- [x] Linux: MIME type `application/x-fanimate-project` and icon, `MimeType` in the desktop file, `%F` so several files open, the metainfo `provides`; AppImage, `.deb` and Flatpak all install them. Not built here.
- [ ] Not built: a Save As Folder command (only older folder projects stay folders). On macOS, Finder hands a file to the running app as an Apple event rather than starting a second process; whether this version of winit passes that on has not been checked, so opening a file from Finder into a running Fanimate may need more work there.

### Phase D: Selection, handles, pivot

- [x] Whole-shape selection with Select Points: click a shape's fill or stroke to select all of its points (a shape in another layer selects that layer first); click a point for just that point; Shift or Ctrl adds. Pressing on a shape's body and dragging moves it, choosing it first if needed. A selection box picks every shape it touches (Alt: only the points inside). The shapes chosen are outlined in the layer's label colour, and the shape under the pointer gets a thin grey outline.
- [x] Selection frame for the Transform tool: the box in the layer's label colour, hollow square handles (corners; edges too where the sides can stretch on their own), per-layer outlines under one union box for several layers. Corner and edge drags scale about the opposite side (Alt: from the centre, Shift: proportions) with the new size shown beside the pointer. Several layers scale and turn together; a stretch a turned layer cannot take (it would shear) becomes an even scale.
- [x] Rotation: a round button below the box on a short stem, and the zone just outside each corner; angle shown beside the pointer; Shift steps by 15 degrees. A layer with no pivot turns about the middle of its box, without moving.
- [x] Pivot: a ring on the canvas that can be dragged (it snaps to the box's nine anchors), a 3 x 3 anchor grid and X and Y fields in Properties. It is stored per layer (`transform.pivot`, format version 4) and used by rotate and scale. **Not animated**: moving it keeps the artwork where it is on every frame unless the layer's rotation or scale is animated, when it only holds on the current frame (the status bar says so).
- [x] Rectangle: the Rectangle tool makes a rectangle that remembers it is one (`rect`, version 4); round dots inside the corners change the radius, Properties has a Corner radius field with Absolute and Relative, and Expand Shape (an action, in menus, the palette and the toolbar) turns it into a plain path. Editing a point by hand does the same quietly. One radius for all four corners.
- [x] Floating toolbar below the selection (above when there is no room): Edit Points or Transform, Group (several layers), Unite (when points on two shapes are chosen), Expand, fill and stroke colours, Duplicate, Delete, and a menu with the layer commands.
- [ ] Not built: the toolbar cannot be dragged by a grip; Align, Offset Path and Lock (there are no such commands yet); Alt+click or double-click to "drop into point editing" (Select Points already works on points, and the Transform tool on layers, so there is nothing to drop into); separate radii per corner; the rotate cursor (egui has none, so the crosshair shows); a pivot that is keyed.
- [ ] Not measured: the frame needs one evaluation per frame with the Transform tool and a selection (the old outline already needed one, and is gone for that tool), and hovering is cached until the pointer, the document or the view changes. No before and after numbers were taken.

### Phase E: Keys and tools

- [ ] Curvature tool, with ghost segment and status hints
- [ ] Shape tools: Line, Polygon, Star, Triangle (radius handles where they apply)
- [ ] Editing tools: Delete Edge, Magnet, Scale Points, Rotate Points
- [ ] Fill and stroke tools: Draw Shape, Freehand with width, Line Width, Stroke Exposure
- [ ] Shape Builder (Shift+M in the default keymap): drag across regions to merge or subtract
- [ ] Each tool: an `Action`, bindings in both keymaps (or deliberately unbound in Moho), an icon (Lucide, or `icons/fanimate/`), a tooltip and a test

### Phase F: Rulers, grid, snapping

- [ ] Rulers in px, pt, mm and in, with draggable guides; right-click a guide to delete or lock it
- [ ] Grid with spacing and subdivisions, drawn under artwork, never exported
- [ ] Snapping to grid, guides, points, shape edges and centres, and canvas bounds, with a hint line while dragging; hold a modifier to bypass; per-target toggles in View > Snapping
- [ ] Pixel snapping, and angle snapping (Shift = 15 degrees) for lines, bones and rotation
- [ ] Snapping lives in `fanimate-editor` (pure functions, unit tested) so every tool shares it

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
