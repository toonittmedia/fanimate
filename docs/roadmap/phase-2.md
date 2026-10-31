# Fanimate Phase 2: Production

Companion to the [product vision](../product-vision.md) and the [Phase 1 plan](phase-1.md). Phase 1 makes Fanimate a replacement for Moho for a solo rigging animator. Phase 2 makes it a tool a small team can finish a production in: text, paint, a node graph, automation, local AI tools, and output to the web and to games. **It ends at Gate 1: version 1.0 and the public launch.** On 2026-10-30 the owner moved that gate here from the end of Phase 1, so that a large body of films and examples, and templates made by hand, exist before anyone outside the circle of testers sees Fanimate. The old Gate 2 (a pilot film ships; the scripting API is frozen) is merged into it. The reasoning, the showcase plan and the handmade-template plan are in [release-1.0.md](release-1.0.md).

> **Status: building has started (M7). Everything after M7 is a plan.** The [Status section](#status) below is the working plan, with one subsection per milestone, and says what is built. The rest of the document is revised after the animators' sessions are written up (section 1.4) and after the first milestones show how long features and films really take. What comes from the vision and what is new is marked in each section. Treat every number in a test or a schedule as a proposal to challenge, in the same way as the Phase 1 plan.

> **What comes from the vision and what is new.** The Phase 2 scope (node graph, text animators, automatic colour fill, raster brushes, scripting API, local AI tools, Lottie and game export) and the two old Gate 2 criteria (a pilot short film ships; the scripting API is frozen) are taken from the vision's roadmap table. The carry-over list, the milestone order, the designs, the acceptance tests and the decisions are new proposals. The move of Gate 1 to the end of this phase, and the showcase and handmade templates that come with it, are the owner's decision of 2026-10-30.

## Status

*Phase 2 building started on 2026-10-30, when the owner asked to begin. M7 is under way; M8 to M15 are plans. Checklists below use `[x]` for work that is built and checked by the tests named beside it, and `[ ]` for work that is not. Section 4 describes each workstream, section 5 gives the milestone table and the schedule; this section is the working plan, one subsection per milestone, and is updated whenever milestone work is finished.*

| Milestone | Theme | Status |
|---|---|---|
| [M7: Foundations and carry-over](#m7-foundations-and-carry-over) | Plumbing | **Built, except what only the owner or the corpus can finish.** Format version 7, transactions, the API crate, bone ids, the stateful cache, the policy and the benchmark baseline are in and checked; still open are the owner's decisions (D1 and D2), the animators' notes, and Moho actions on import, which needs corpus files |
| [M8: Depth](#m8-depth) | What animators asked for | **Mostly started.** Built: the easing engine, spring physics with wind and gravity and Bake to keys, the pose library (save, mirror, insert), master controllers (sliders and pads, rename and link in the Actions panel), the graph editor and Simple mode. Not built: canvas rotation, symbols and the Library, rig templates and retargeting, the conditional items, a keyed pivot and the other UX leftovers |
| [M9: Text](#m9-text) | P3 | **Mostly built, out of order.** The text engine, the text layer, animators, presets, text on a copied path, captions (SRT and WebVTT), the Properties panel, the API and a benchmark are in; fonts for more scripts, the Text tool, a live reference for text on a path, templates and text import are not |
| [M10: Paint](#m10-paint) | P10, P11 | Not started |
| [M11: Pipeline](#m11-pipeline) | P4 | Not started |
| [M12: Automation](#m12-automation) | P18 | Not started; its API grows from M7 on, each milestone adding what it builds |
| [M13: Intelligence](#m13-intelligence) | P8 | Not started |
| [M14: Output](#m14-output) | P19 | **Started.** Built: one frame as SVG, Lottie and dotLottie (baked, checked in the lottie-web player), sprite sheets with a TexturePacker-style atlas, each with a report of what it could not carry, in the app (File > Export for Web and Games) and on the command line. Not built: animated SVG, the open runtime format and its runtimes, OpenTimelineIO, a second Lottie player check |
| [M15: Showcase and 1.0](#m15-showcase-and-10) | Quality gate and launch | Not started; the showcase and handmade-template work runs alongside from the first month ([release-1.0.md](release-1.0.md)) |

**To look at what is built so far** (the Properties panel and the text effects have been checked by tests but not yet by a person looking at them), run a debug build of the app from the repository root: `cargo run -p fanimate-app`. Then Layer > New Text Layer, type in the Properties panel, and pick "Add animator…" or Layer > Text animator. `cargo run -p fanimate-cli -- sample hello out.fnm` and `bench --text 1000` need no window.

**Format versions, planned.** Each milestone that changes what `.fnm` writes ends with one version bump, following the format rules (spec, migration, validator rows, round-trip and damaged-file tests). The numbers can move; the rule of at most one bump per milestone does not.

| Version | Milestone | What it adds | Feature in `requires` |
|---|---|---|---|
| 7 | M7 (built), then easing and text were added before its first release | The manifest's `requires` list; a layer's `provenance` record; bone ids; eased and curved key interpolation; text layers with animators | `provenance@1`, `text@1` |
| 8 | M8 | Symbols and instances; palettes' groundwork if needed; controllers; animation and pose libraries | `symbols@1` |
| 9 | M9 (the rest) | Caption layers and template parameters; the text layer itself is in version 7 | `captions@1` |
| 10 | M10 | Raster layers and drawings, palettes and colour references, colour marks | `raster@1`, `palette@1`, `marks@1` |
| 11 | M11 | The node graph | `graph@1` |
| none | M12 to M14 | Plugin lock file (`plugins.lock`) is a separate file, not a format change; exports and models change no project data | |
| none | M15 | No format change after the feature freeze | |

**The showcase runs beside the milestones.** From [release-1.0.md](release-1.0.md): the handmade-template briefs and the licence and consent forms start in month 1, each film starts when the features it needs reach the alpha channel, and every milestone below names what the showcase can start once it ships.

### M7: Foundations and carry-over

Goal: make the engine ready to take new layer kinds, graphs, plugins and models without each inventing its own plumbing, and clear the Phase 1 leftovers that block them. Covers WS1. It changes almost nothing a tester sees: files are written as version 7, and Preferences has a "Work offline" checkbox.

Exit check (revised when M7 was built, because the graph and the models do not exist yet to stub): a stub tool changes a document through the API as one undo step and the file keeps its provenance (done: `a_tool_marks_a_layer_through_the_api_and_the_file_keeps_it`); a stateful stepper is scrubbed in any order and equals a straight run (done: the `stateful` tests); the policy gates the one network feature there is (done); the Phase 1 benchmark is no slower (done: see the baseline below); a file from any earlier version opens (done: the version 0 to 6 fixture and migration tests).

Depends on: nothing. Everything else depends on it.

**Slice 1: the format, transactions and the API crate.**

- [x] **Format version 7.** The manifest has an optional `requires` list and a reader refuses a project that needs a feature the build does not have, naming it (`FnmError::MissingFeature`). Layers have an optional `provenance` record (tool, model, version, date, in `fanimate-core::provenance`); a layer with one makes the file require `provenance@1`. The validator reports a record that is incomplete or too long and repairs it without dropping it. The journal rewrites its manifest when the needed features change. Checked by: the `fanimate-fnm` tests (82 passing), among them round trips of folder and single file, a version 6 project opening unchanged, an unknown feature refused with its name, a damaged record repaired, and the journal following the features.
- [x] **Transactions.** `Editor::transaction(label, f)`: one undo step however many edits `f` makes, nested checkpoints folded in, nothing recorded when `f` changes nothing, the document and selection restored when `f` fails with the undo and redo history untouched, and refused while the animation plays. Checked by five `fanimate-editor` tests.
- [x] **`fanimate-api`** (new crate above `fanimate-editor`, no egui). `API_VERSION` (`0.1.0-unstable`), stable error codes (`not_found`, `invalid_argument`, `playing`), queries (`document`, `layers`, `layer`) and the first explicit-target commands (`rename_layer`, `set_layer_visible`, `set_layer_provenance`) through transactions, with JSON-shaped data types. `api-surface.txt` snapshots the public surface and a test fails when it changes. Checked by 12 tests, among them a stub tool that marks a layer, saves, reloads and undoes.
- [x] **Docs:** `fnm.md` (version 7, required features, provenance, validation rows, version history), `architecture.md`, `CLAUDE.md` (transactions, required features, API-first).
- [x] `LayerKind::type_name`, the file format's layer `"type"` as a function, with a test (needed by the API and by exporters).
- [x] **Whole-workspace check for slice 1:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (every suite passes, the damaged-projects tests with the provenance fixture among them), and `cargo check` of the engine crates and `fanimate-api` for `wasm32-unknown-unknown`, all clean on 2026-10-30.

**Slice 2: ids, evaluation and the leftovers.**

- [x] **Stable ids audit, and ids where indexes move.** The audit: layers already have ids; image and sound media are named by their content; actions are addressed by name, which the editor keeps unique. **Bones** were addressed by list index, which changes when a bone is reparented (`Editor::set_bone_parent` reorders the list) or removed, so a script or a driver holding an index could silently find another bone. They now have ids (`Bone::id`, `Skeleton::next_bone_id`, in version 7, which is unreleased, so no further bump): never reused, assigned on every load to files that have none (silently, the same on every load), repaired by the validator, handed out by `Skeleton::add`, and given to importers' bones in the Moho importer. The API names bones by id (`Api::bones`, `Api::rename_bone`). Checked by tests in core, the format crate, the editor (a reparent that reorders the list keeps every id) and the API. **Shapes** stay addressed by (layer id, index) with the editor's revision as the guard: nothing in M8 needs more, and giving every shape an id touches every drawing tool. Revisit if the API or symbols need it; it would go into version 8 with M8's change.
- [x] **The stateful-node cache.** `fanimate_core::stateful`: `Stepper` (state after a frame is a pure function of the settings and the frames before it) and `StateCache`, which remembers states at intervals, starts from the nearest one so scrubbing is cheap and playing forward costs one step per frame, throws everything away when the caller's input hash changes, caps the frames it will step and the states it keeps. Six tests, among them a random-order scrub that must equal a straight run bit for bit.
- [x] **Change tracking: decided to build it in M11, with the effect pipeline.** Confirmed: evaluation reuses its buffers but recalculates every frame. The Phase 1 measurement is 0.73 ms of CPU a frame to evaluate and tessellate the 40-bone, 10-Smart-Bone benchmark rig against a 16.7 ms budget, so tracking changes would save a few percent today and cost real complexity. It pays when effects make frames expensive, and it is the same machinery as the node graph's cache of unchanged subgraphs, so it is built there (M11, item 11b) instead of twice.
- [x] **Benchmark scenes: decided to add each with its feature, not before.** A scene for a feature that does not exist is placeholder code, which the project forbids. The milestone that builds a feature adds its scene to `fanimate-cli bench` in the same change (text in M9, raster in M10, effects in M11, plugins in M12).
- [x] **Benchmark baseline, 2026-10-30,** release build on the Windows development laptop (`fanimate-cli bench`): the 40-bone, 10-Smart-Bone rig is 0.53 to 0.65 ms of CPU a frame over four runs (evaluate 0.05 ms, tessellate the rest; the first run after a build read 0.93 ms because the machine was still busy), against 0.37 to 0.73 ms recorded in Phase 1, so nothing slowed; the 400-bone stress scene is 6.2 ms a frame; the 5-minute episode is 0.78 ms a frame and loads in 162 ms. These are the figures every later "before and after" is measured from. Found and fixed on the way: `fanimate-cli bench` had been failing since projects became single files (it never made its temporary folder), with a test that fails without the fix.
- [ ] **Moho actions and Smart Bones on import once the corpus has files that show where their poses are stored (blocked on the corpus).
- Moved out of M7: **the video layer** ([video-import.md](video-import.md)). It is a user-visible layer kind with a decoder, proxies and a format bump, not plumbing. It now sits in M10, beside the media and tile storage it shares work with.
- [x] **Media provenance: not needed.** A picture or sound is only ever used through a layer (an image or audio layer), and a layer carries the record, so generated media is already labelled wherever it is used. If media is later shared between projects (the Hub's linked assets in Phase 3), the asset carries its own record then.
- [x] **The offline switch.** `fanimate_core::Policy` (offline, ai, cloud; the rules, the strictest-wins combination, the locked policy), read by `fanimate-app/src/policy.rs` from `fanimate-policy.json` beside the program and `policy.json` in the user folder (a damaged or unreadable file locks everything and names itself), plus a "Work offline" setting in Preferences. `Shell::policy()` gates the update check today and is the gate every later network, AI and cloud feature must ask. Documented in `docs/install.md`. Checked by 4 core and 8 app tests.
- [ ] **Decisions:** answer D2-1 to D2-11 and D1-1 to D1-10 (the owner), and add the legal questions to the review scope. D2-1 is already answered in practice: Phase 2 building has started, and features go to the closed alpha as they are built.
- [ ] **The animators' notes** (section 1.4), so the order of M8 to M14 can be revised with them.

**Showcase and templates, in parallel.** Write the licence, consent and provenance forms and create `docs/legal/template-assets.md`; recruit the review panel and the first makers; write the briefs for Walk cycle, Lip sync, Frame by frame and Character rig.

Not in M7, and why:

- **No new layer kind and no user-visible feature.** Those start in M8. M7 is the part that makes them cheap.
- **A `phase2` build feature.** Not needed: M7 adds no behaviour a tester can reach. If a later milestone needs an unfinished feature hidden from the alpha, the flag is introduced there.
- **A scripting host.** The API crate is the Rust surface; the hosts that speak it arrive in M12.

### M8: Depth

Goal: the committed animation and rig items from WS9, plus Simple mode and canvas rotation from WS10: the things Animate and Harmony users miss, and that the later milestones' showcase needs (a symbol library for a crowd, a graph editor for timing).

Exit check: an animator builds a reusable walking extra as a symbol, retargets a walk cycle between a tall and a short rig, adds a spring tail that bakes to keys, and moves a head from a 2D pad.

Depends on: M7 (ids, the node cache for physics). Format version 8 (`symbols@1`).

Planned order, easiest and least dependent first:

- [x] **Easing, the engine half of "easing presets"** (WS9). `fanimate_core::easing`: six named easings (ease in, ease out, ease in and out, overshoot, bounce, elastic) and `Bezier`, a cubic curve in thousandths so interpolation stays `Eq` and `Hash`; `Interpolation::Ease` and `Interpolation::Bezier`, saved as `"smooth"`, `{"ease":"bounce"}` and `{"bezier":[250,100,750,1000]}`; colours are held in range where an overshoot pushes them out; the validator reports and repairs a curve with a control point out of range; every interpolation menu and the command palette list the eased choices (`Interpolation::ALL`, labels by a macro that must cover each), and eased keys draw as curved-motion marks. Checked by 8 easing tests, 3 animation tests and a file round trip. Not yet seen in the running app.
- [x] **Graph editor panel (built, driven in the running app):** the Graph tab beside the Timeline (Window > Graph; in the default bottom dock of a new layout). It shows one property of the selected layer at a time (position, rotation, scale, opacity, layer order, a keyed bone's angle: the units differ, so they do not share an axis), one curve per number (x and y of a pair), with a grid in degrees or percent, the playhead and the keys. Drag a key to move it in time and in value (it stays between its neighbours; the picked key follows it; the value range is held during the drag so the key does not slide away); right-click a key for the interpolation choices, including **Custom curve**, whose two handles then show on the key and can be dragged to reshape the segment (`Bezier`); every drag is one undo step (`Editor::checkpoint` once, then `change_key` and `change_key_curve`). Core: `fanimate_core::graph` (`graph_channels`, `curve_keys`, `curve_value`, `set_curve_value`, `key_room`). API: `curves`, `move_key`, `set_key_interpolation` (also in the JSON-RPC table and `docs/scripting.md`). Checked by 5 core tests, 4 editor tests, 2 API tests, and by dragging a key, undoing, making a custom curve and dragging its handle in the real app. **Open:** tangent modes (broken, mirrored), selecting and dragging several keys at once, a normalise view that fits one curve, zoom and pan of the plot, colours, camera and shape properties, user presets of curves, and showing several properties together.
- [ ] **Pose and action library** (WS9): user-level and project-level; drag a pose onto the timeline; mirror a pose left to right by a naming convention.
- [x] **Simple mode (built):** Window > Workspace > Simple (`Workspace::Simple`, so the command palette and the saved layout know it): the Tools panel keeps the Draw, Fill and Layer groups (no Combine, Camera, Bone or Workspace tools) and the panels are Layers, Properties, Style and the timeline, with no Bones, Actions, X-sheet or History; leaving for any other workspace restores everything. A layout saved before the switch reads as not simple. Checked by 2 layout tests. **Open:** menus and the Properties panel's advanced sections are not trimmed, and a status-bar toggle.
- [ ] **Canvas rotation** (WS10): rotate the view with a gesture or key, with rulers, guides and snapping staying right.
- [ ] **UX pass 3 leftovers** (WS9): a keyed pivot (format field and the rule for what it does to the look), Offset Path, Eraser and Freehand snapping.
- [ ] **Symbols and the Library** (WS9): group to symbol with its own timeline and loop modes, instances with overrides, edit in place, cycle refusal, the Library panel; the Animate importer upgrades to real symbols. Format bump.
- [x] **Retargeting, the action half (built):** Copy to Rig in an action's right-click menu in the Actions panel (`Action::RetargetAction`, `Editor::retarget_action_to`, `fanimate_core::retarget`, API `retarget_action`): bones are paired by name (case, spaces, dots and underscores ignored), each bone's turn from its own rest angle is carried onto the other rig's rest angle (so proportions need no scaling), easing and loops are kept, a Smart Bone action needs its driver matched, and bones with no match and shapes' point keys are counted in the status message. One undo step. Checked by 5 core tests, 2 editor tests and an API test. **Open:** foot lock (needs IK targets), position and scale keys of non-bone layers, and **rig templates** (biped, quadruped, bird, face) with handmade art, which wait on the showcase track.
- [x] **Master controllers, the engine, editor and API (built; the panel's slider, link and menus were driven in the running app, but the effect on bound artwork was not seen there):** a slider or a pad on a bone layer (`skeleton.controllers`, spec section Master controllers, `controllers@1` in `requires`) whose keyed values play regular actions: each link maps a value range onto an action's frames (held at the ends, backwards when written backwards), and the action's bone angles and point keys apply through the same path as Smart Bones. Commands `add_controller`, `remove_controller`, `link_controller`, `unlink_controller` and `set_controller_value` (keying rule, one undo step, a drag is one step) in the editor and the API and JSON (`controllers`, `add_controller`, `remove_controller`, `link_controller`, `unlink_controller`, `set_controller_value`); deleting an action renumbers the links. Checked by 5 core tests (value range, held and NaN values, pad axes, keyed values, links that do nothing, controllers off while editing, saving), 2 format tests (round trip and required feature, repair), 4 editor tests and 2 API tests. The Actions panel lists a rig's controllers under its actions: New Slider and New Pad, a slider or a square pad that sets the value by the keying rule (a drag is one undo step), and a right-click menu with Play Action (links the whole 0 to 1 range), Stop Playing and Delete Controller (the commands are the Actions `NewController`, `LinkController`, `UnlinkController` and `DeleteController`, tested in the editor). **Open:** renaming a controller and editing a link's range in the panel (the API sets them), dial controllers (a slider drawn round is only a look), and several controllers combining on one action by adding instead of the last winning.
- [x] **Physics, the spring half (built, checked in the running app):** `fanimate_core::physics`: a bone follows its keyed angle on a damped spring (stiffness, damping, inertia, a fixed timestep with up to 256 substeps so a stiff spring stays stable, the bone's angle limits respected), and is thrown about by the turning of the bones above it, so a tail, ear or scarf swings and settles. It is a `Stepper` on the M7 `StateCache`, so its state is the same whatever order frames are asked for. **Bake Spring Motion** (Animation menu, and a Secondary motion section in the Bones panel) runs it on the selected bone and those below it, in order from the top so the whip runs down the chain, and writes one key a frame, replacing the keys in the range: plain keys, so no format change and nothing here is needed to play the file. One undo step; a bad request changes nothing. Through the API: `add_bone_layer`, `add_bone`, `set_bone_angle`, `bake_spring`. Checked by 8 physics tests (a still key stays still, a step overshoots and settles, critical damping does not overshoot, scrub-order equality, a parent's turn throws the child and no inertia leaves it still, chain order, limits, hostile settings, determinism), 3 editor tests, 4 API and JSON tests, and the real app: a three-bone rig was baked from the Bones panel and the tail curled behind the body at frame 16. **Wind and gravity are in too:** `gravity` (a turning acceleration pulling bones down the canvas) and `wind` (a steady push to the right, negative to the left) act across each bone's world direction, so a sideways tail sags by about gravity over stiffness and a hanging one is swung out by the wind; checked by 3 more physics tests (the sag, the unmoved hanging bone, both wind directions, scrub-order equality with them, hostile values), in the Bones panel and in `bake_spring`. **Open:** a ground plane and collisions (they need bone positions, not just angles), a live preview before baking (only the bake exists), springs on position and on shape points.
- [ ] API entries and tests for every item above (rule 13), both keymaps, context menus, and a manual chapter each.

Showcase and templates: the first four handmade templates reach review; the first walk-cycle film can start.

Not in M8, and why: the four conditional items (shape tweens with hints, animation layers, curve and envelope deformers, Asset Warp) are built only if physics and the AI tools are on time. Multiplane depth is Phase 4.

### M9: Text

Goal: text is a first-class animated object (P3): typed in the Properties panel, shaped correctly, animated per character, word and line, and available as templates and captions. Covers WS2. It is split into three steps, each ending with the workspace green. **M9a and most of M9b are built** (in the order the work went, ahead of M8's remaining items, because text needs only M7); M9c and the parts marked open below are not. Nothing below has been looked at in the running app by a person yet: it is checked by tests, by rendering through the evaluator, and by compiling the panel. Use the debug preview command in the status notes to look at it.

Exit check: a tester types a title in Arabic and in Devanagari, animates it per word with a ramp selector, puts a line of it on a curve, saves, reopens on another platform, exports to MP4 and gets the same result. **Not met yet:** there are no Arabic or Devanagari fonts bundled, text on a curve is not built, and cross-platform output has not been compared. Format version 7 (`text@1`).

Depends on: M7. (The original plan also named M8's graph editor and symbols; they are only needed for editing animator curves in a graph and for template groups, so M9a and M9b did not wait.)

**M9a: static text and fonts.**

- [x] **The engine, `fanimate-text`** (new crate, no IO, no UI, builds for WebAssembly): shaping with HarfBuzz's algorithm (`rustybuzz`), bidirectional order (`unicode-bidi`, with the Unicode reordering rule written out and tested), line breaking (`unicode-linebreak`) with a hard break for a word wider than the box, characters and words by `unicode-segmentation`, alignment that follows the paragraph's direction, tracking, line height, OpenType feature switches, a weight axis for variable fonts, and glyph outlines (quadratic curves raised exactly to cubic). Never trusts its input: sizes, widths and lengths are held to sane ranges, text is cut at 100,000 characters, and a missing font or a language tag that is nonsense still lays out. 24 tests, among them kerning, tracking arithmetic, wrapping, alignment, right-to-left runs inside English, and awkward strings (emoji sequences, control characters, reversed-text marks).
- [x] **The text layer in core** (`fanimate_core::text`): a `Text` holding its look (`Look`) and its animators, evaluated into ordinary shapes so the tessellator, exporters and the browser player needed no text code. The layout is made once per look and shared by clones (a look edit goes through `Text::look_mut`, which forgets it, so a stale layout cannot be drawn); the glyph shapes are borrowed by the draw items, so a frame of plain text allocates nothing.
- [x] **Fonts:** Inter (variable, with a weight axis) and JetBrains Mono, both already bundled and OFL. A family Fanimate does not have is set in Inter, and the validator says so without losing the name.
- [x] **Fonts for Arabic, Hebrew, Devanagari and Thai (built, checked by looking at renders):** Noto Sans Regular for each (SIL OFL 1.1, from the Noto project's built files; licence in `brand/fonts/NotoSans-OFL.txt`, listed in `docs/legal/licences.md` and checked by `scripts/licences.py`; about 540 KB together). Fallback is per character: the font asked for is used when it has the glyph, otherwise the first bundled font that suits the character's script and has it; spaces, digits, marks and joiners stay with the font before them, so words are not cut. Glyphs carry their own font through layout and outlining. A text with a Latin, an Arabic, a Devanagari, a Thai and a Hebrew word lays out in the right fonts in the right directions, joined Arabic forms, Devanagari conjuncts and Thai stacked marks come from the fonts' own rules, and a character no font has is the missing-glyph box and nothing else changes. Checked by 8 engine tests and a rendered title in all five scripts. **Open:** a CJK font (too large to bundle: a download), bold weights for the script fonts (they have one weight), **embedding** a font in a project when its licence allows, and **native speakers checking the shaping** (decision D2-10): the output looks right to a non-reader and the Unicode rules are followed, but that is not a review.
- [x] **Editing in the Properties panel:** the words (a multi-line box), font, size, weight, letter spacing, line height, alignment, direction, wrapping, colour and outline, with one undo step per drag or burst of typing; "Convert Text to Shapes". New Text Layer is in the Layer menu and the layer context menu, and a Text submenu shows with a text layer selected.
- [x] **The Text tool and typing on the canvas (built, checked in the running app):** T in the Fanimate keymap (unbound in the Moho keymap, whose T is Transform Points), in the Draw group. A click on empty canvas starts new text, a click on text edits it where it stands (`Editor::text_at` finds it, margin included, topmost shown layer first). An egui text box sits over the rendered words with see-through letters, so the caret, selection, copy and paste and the system's input methods work while the words themselves are the real glyph shapes. Every keystroke goes through `change_text` (one undo step for a burst of typing); a layer is only made when the first character is typed, and one this edit made that ends empty is removed again. Escape or a click away ends the edit. Checked by an editor test of the hit test and by driving the real app through `orca computer`: T, a click, typing made a layer, a click on it and more typing changed it. **Open:** a caret that follows right-to-left text and complex scripts exactly (the box uses the UI font's metrics, close but not identical to the layout engine's), and drag-to-make-a-wrapped-text-box.
- [x] **Hostile input:** damaged files with a text layer open in 3,000 seeded trials without a crash (`damaged_projects.rs`), and the validator repairs out-of-range settings, too many animators and bad animator values and keys.
- [x] **Format:** the `text` layer kind, `requires: ["text@1"]`, spec, validator rows and round trips of folder and single file.
- [x] **API:** `add_text_layer`, `text`, `set_text_content`, `set_text_style`, `add_text_animator` through transactions, with tests, including a scripted title saved, reloaded and drawn the same.
- [x] **Performance** (release build, the 2015 laptop): a 1,000-character title with two animators lays out in 7.9 ms once (the target is under 16 ms), evaluating its animators costs 0.09 ms a frame, and tessellating its 791 glyph outlines costs 9.2 ms a frame: 9.3 ms of CPU against the 16.7 ms budget. `fanimate-cli bench --text 1000` is the benchmark scene. **Follow-up:** tessellation dominates and is repeated every frame for glyphs that have not changed; caching glyph meshes in the renderer would make animated text cheap and help every static shape too.
- [ ] **Open: shaping goldens for each script, and rendering the same pixels on Windows, macOS and Linux** (only Windows has been run).

**M9b: animators, paths and variable fonts.**

- [x] **Animators:** a list per text layer, each with a unit (character, word, line), a selector (range start, end and offset in percent, all keyable; square, ramp up, ramp down, triangle, round and smooth shapes; a soft edge on the square shape; a seeded random order that is a true shuffle and the same on every machine) and properties (position, rotation and scale about the middle of the unit, opacity, fill colour), all ordinary channels with keys, easing (including the new bounce, overshoot and custom curves), loops and the keying rule. Several animators combine in order. 22 evaluation tests, among them a frame being the same whatever was drawn before it.
- [x] **Presets:** fade in by character, typewriter, rise in by word, pop in by character, and a looping wave; ordinary animators the user can edit afterwards. Menu and command palette entries ("Text Animator: …") and the Properties panel's "Add animator…".
- [x] **Editing animators:** the Properties panel lists them with every control, a keyframe diamond beside each animated value (add or remove a key on the playhead frame), and Remove. The editor's commands (`add_text_animator`, `edit_text_animator`, `set_animator_value`, `toggle_animator_key`, with `change_*` variants for drags) are tested, including that a drag is one undo step.
- [x] **Frame cost:** reused buffers (no allocation per frame for unchanged glyphs); glyphs faded to nothing are not drawn.
- [ ] **Open: animator rows in the timeline** (their keys count as the layer's keys and move with the timeline, but have no rows of their own yet) and editing their curves in a graph editor (M8).
- [x] **Text on a path (built):** a text layer follows the first shape of another vector layer (Layer › Text > Set Text Path from Shape, or `Api::set_text_path`); the path is copied into the layer's space, can be read from its end, a closed path carries the text round, an open one cuts off what does not fit, and the start along the path is a keyed percentage. It is a copy: the text does not follow later edits of the shape (live reference by id is open). Spec, validator, save round trip, editor and API tests are in.
- [x] **Animator properties tracking, anchor, outline width and outline colour (built):** `tracking` adds space after each reached character and pushes the rest of the line along (per line, mirrored for right-to-left text, ignored when it is not a number); `anchor` moves the point rotation and scale turn about; `stroke_width` and `stroke_color` give glyphs an outline or change theirs (capped at 1000). All are channels with keys, in the Properties panel with diamonds, in the spec and validator, and reachable through `Api::set_text_animator_value`. Checked by 7 evaluation tests, 1 editor test and 3 API tests.
- [ ] **Open: live reference to the shape**, **variable-font axes as keyable values** (weight is a static setting today), and **several selectors per animator with add / subtract / intersect modes**.
- [ ] Per-character blur is left to M11 and is dropped if M11 cannot take it.

**M9c: templates, captions and import.** Not started.

- [ ] Exposed parameters and a local template folder; handmade bundled text templates (titles, lower thirds, captions, kinetic type) under the rules in [release-1.0.md](release-1.0.md#5-handmade-templates).
- [x] **Captions, built without a new layer kind:** SRT and WebVTT import and export (`fanimate_core::captions`, tolerant of damaged files, capped at 20 MB and 50,000 captions, 2,000 characters each; tags and positions are dropped). An import makes a switch layer with one ordinary text layer per caption, white with a dark outline and centred low on the canvas, shown by the switch keys from its start frame to its end frame (a caption that overlaps the next ends where the next begins; one shorter than a frame shows for one frame; the animation is lengthened to the last caption). Export reads the same layer back. In the app: File > Import Captions… and Export Captions…, and dropping an `.srt` or `.vtt` file on the window; through the API: `import_captions`, `export_captions`. Checked by 11 parser tests (including 2,000 random hostile inputs), 8 editor tests (times, overlaps, save and reopen) and API and JSON tests. **Open:** a caption layer styled by a template (the captions are ordinary text layers and can be restyled, but there is no "restyle all" command), the format-9 `captions@1` layer kind (not needed so far, since the switch layer does the work), speech to text (M13), and SRT styling tags.
- [x] **SVG text import (built, checked by rendering a poster-style SVG):** `<text>` elements come in as text layers above the file's shapes (`editor/svg_text.rs`; usvg is built without fonts and used to drop them): the words, `<tspan>` lines (a new `y` or a non-zero `dy` in px or em), font family (first non-generic name, if Fanimate has it), size (px, pt, pc, mm, cm, in), weight, fill (hex, `rgb()`, a few names, inherited and from `style=`), `text-anchor` start, middle and end, and the transforms of the element and its parents (position, scale, rotation, the view box), so the baseline start lands where the SVG put it even when the text is turned. Text on a path, in `defs`, hidden or without a fill is left out and only the first is reported. Capped at 2,000 elements. Reachable through `Api::import_svg`. Checked by 7 parser tests (including hostile numbers and the cap), 3 importer tests and an API test. **Open:** Moho text layers and Animate text fields (they need corpus files: decision about the corpus is the owner's), letter and word spacing, `<use>`d text, the drawing order between text and shapes.
- [ ] A manual chapter and a first interactive tutorial.

Showcase and templates: the kinetic-type film and the captioned dialogue film can start now that animated text works in the alpha; native speakers still need to test Arabic, Hebrew, Devanagari and Thai once fonts are in; Yoruba and Igbo diacritics are a named test (decision D2-10) and can be tried today with Inter.

Not in M9, and why:

- **Speech-to-text for captions** is a local AI tool and arrives in M13, on the same caption layer.
- **Vertical text, ruby and text effects such as extrusion** are out of scope for Phase 2.
- **Text export to Lottie as editable text** is not planned: text goes out as outlines (M14).

### M10: Paint

Goal: one brush engine that paints vector strokes or pixels on any layer type and any frame (P10), and colour that carries through a cycle (P11). Covers WS3 and WS4.

Exit check: an artist paints a textured background and a frame-by-frame raster cycle, imports their Krita brushes, saves, recovers after a forced kill and exports the same pixels the canvas showed; an animator colours the first frame of a 48-frame walk with marks, tracking fills the rest, the review panel flags the frames where the hand crosses the face, they fix two marks, and changing the shirt in the palette updates every frame. Format version 10 (`raster@1`, `palette@1`, `marks@1`).

Depends on: M7; M8's symbols for palette variants in reusable objects (not blocking).

- [ ] **10a, raster layers:** sparse 256 by 256 tiles (RGBA8, premultiplied, sRGB), one set per drawing so frame-by-frame raster works and the rough, colour, line and overlay sub-layers can each be vector or raster; copy-on-write tiles that fit the per-layer history; storage as a PNG per drawing up to 2048 by 2048 and a tile bundle beyond; one resident frame; size and tile-count caps on load.
- [ ] **10a, the brush engine:** GPU stamps with float blending, and a CPU reference implementation of the same maths for tests and headless runs; dynamics (size, flow, opacity, rotation, scatter driven by pressure, tilt, speed, direction, random); procedural and image tips; textures; blend modes (normal, multiply, screen, overlay, add, erase, behind, inside); smudge and mixing; the existing stabiliser applies.
- [ ] **10b, tools and formats:** Brush, Pencil, Eraser, Smudge, Lasso and Transform Raster; rasterise and trace; own brush format (JSON plus a tip image, documented); Krita preset and Photoshop `.abr` import with an import report; OpenRaster `.ora` import and export; Krita `.kra` import. Legal review of `.abr` first.
- [ ] **10c, regions and palettes:** planar-map region extraction from line art with gap closing, the gap-tolerant bucket with a preview of the gaps it bridges, the raster version with a distance transform and a fill that grows under the line; palette entries and colour references (literal colours stay valid), "Extract palette", palette variants as per-scene overrides.
- [ ] **10d, colour marks:** marks on key drawings; tracking by region overlap, centroid after global motion, adjacency and colour history, solved as an assignment; splits and merges flagged uncertain; marks are the source of truth and fills are derived, so Re-track is non-destructive; the review panel with next, previous and accept.
- [ ] Tests, from WS3 and WS4: CPU and GPU stroke agreement; tile-diff undo; missing tile becomes transparent plus a validator issue; stroke latency under 30 ms on a 2048 by 2048 layer; 300 frames within the memory budget; the labelled colouring set (at least 30 sequences, 90% of region-frames right with no fix and 90% of wrong ones flagged); region extraction on 10,000 strokes under 100 ms or off the UI thread.
- [ ] **Video layers** (carried over from Phase 1, moved here from M7): reference clips to animate over, as a layer kind with a proxy made by the user's own FFmpeg, following [video-import.md](video-import.md) and settling its open questions first (proxy codec and size, the format bump, frame accuracy, off-thread proxy building, the web build's limit).
- [ ] API entries and tests; benchmark scenes filled in; a manual chapter each.

Showcase and templates: the painted-background film and the colour-mark short start; the colouring dataset and its consent record are collected here.

Not in M10, and why: filters (blur, glow, colour correction) are nodes in M11; the live link for SVG is a stretch goal; keying a palette entry is not planned.

### M11: Pipeline

Goal: an optional node graph beside the layer list (P4) for effects, compositing, rig logic and render passes, with the layer list staying the structure. Covers WS5.

Exit check: a tester adds a glow and a shadow to a character, mattes a mouth inside a head, drives a blink from a random-interval node, exports a layered EXR with line, colour and shadow passes, and sees the same frame in the canvas and in the export. Format version 11 (`graph@1`).

Depends on: M7 (node cache); M9 (text blur, optional); M10 (raster masks).

- [ ] **11a, the graph core:** the stored graph holds explicit nodes, edges, parameters and positions, referring to layers by id; implicit nodes come from the layer list; overrides show a "routed through graph" badge on the layer; cycles are refused when a wire is made; typed ports; `type@version` nodes.
- [ ] **11b, image nodes:** blur, glow, colour correction, shadow and light, mask and matte, cutter, blend modes, transform, crop, merge, switch, grain, vignette, each a WGSL pass in document units; pooled render targets sized to the region of interest; cached static subgraphs; draft mode and frame cache.
- [ ] **11c, data nodes and drivers:** constants, time, math, seeded noise, vector ops, the seeded random-interval node, sample channel, the expression node (small language, no loops, bounded, type-checked), constraints; drivers on bone angles and lengths, layer properties, switch choices, Smart Bone values and palette entries; "Bake driver to keys".
- [ ] **11d, render passes:** pass output nodes and sub-layer passes; layered OpenEXR export.
- [ ] **11e, the editor:** the Node view tab beside Layers and a docked node canvas; search-to-add, right-click menus on nodes, ports and wires, subgraphs, comment frames, auto-layout, keyboard navigation; properties in the Properties panel.
- [ ] Tests: data graph deterministic under any scrub order; effect goldens across platforms; cycle and type errors; round trip; hostile graphs (dangling edges, 100,000 nodes, NaN parameters) repaired or refused; the list and the graph agree after random edit sequences; the effects benchmark published.
- [ ] API entries and tests; manual chapter.

Showcase and templates: the effects film starts.

Not in M11, and why: an ID or cryptomatte pass is a stretch; Harmony's node view import is Phase 4.

### M12: Automation

Goal: a stable scripting API (P18) that studios can build pipelines on, usable headless, in beta by the end of this milestone and frozen at 1.0. Covers WS6. Work on the API itself starts in M7 and grows with every milestone (rule 13); M12 builds the hosts and the surrounding tools.

Exit check: a studio technical director writes a Python script that opens 20 scenes headless, lip syncs each, runs the health check and exports MP4s overnight; a TypeScript panel and a WASM brush are installed from a folder; the API is published in beta for outside authors. (The freeze comes at 1.0, after at least 8 weeks of public beta use.)

Depends on: M7 (the API crate); each of M8 to M11 as their features land.

- [x] **12a, stdio transport and headless (built; the socket is open):** JSON-RPC 2.0, one request per line, over standard input and output (`fanimate-cli script`, documented in [scripting.md](../scripting.md)); `fanimate_api::rpc` holds the method table and dispatcher, so every host speaks the same calls; notifications, the spec's error codes and the API's own code in `error.data.code`; `undo`, `redo`, `save` and `quit` are in. Checked by 8 dispatcher tests (hostile and malformed input, playing, the method list matching the dispatcher), 4 host tests and one test that runs the real binary as a child process. Scene building is reachable too: `add_layer`, `delete_layer`, `set_document`, `set_layer_transform` (keyed by the keying rule, in degrees) and `add_shape` (rectangle and ellipse with colours), checked by tests and by building and rendering a small animated scene from a Python script. **The local socket is built too:** `fanimate-cli serve` listens on `127.0.0.1` only, prints a random 128-bit token, makes the first request `auth` (a wrong token or a call before it is answered `unauthorized`, a new stable error code, and the connection is closed), serves one connection at a time on one shared project, has `quit` and `shutdown`, times out a client that says nothing at the door, and refuses lines over 8 MB. Checked by two process-level tests (token refusal, shared work across connections, shutdown, random tokens and ports) and Python client tests. **Open:** the app serving the same calls from a running window (off by default, switched on in Preferences), `run` for a script file, permissions for plugins (12b), and calls for freehand paths, Smart Bones, timeline keys beyond transforms and rendering.
- [ ] **12b, permissions:** the manifest and the permission list enforced by the host.
- [x] **12c, the Python client (built; not on PyPI yet):** `clients/python/` (`fanimate.Host`): starts `fanimate-cli script`, every API call is a method, failures raise `fanimate.Error` with the API's stable code, standard library only, Python 3.9 or later, `pyproject.toml` ready. Checked by 6 tests against the real CLI (non-ASCII text, error codes, undo and redo, save and reopen, closing twice, a missing CLI). **Open:** publishing to PyPI (a decision for the owner: the name `fanimate` must be claimed), type stubs generated from `api.methods`, a worked gallery of studio examples, and talking to a running app rather than a headless host (needs the local socket of 12a).
- [ ] **12d, the TypeScript host:** an embedded JavaScript engine with limits, TypeScript stripped at install; type definitions.
- [ ] **12e, the WASM host:** the component model, fuel and memory limits.
- [ ] **12f, UI contributions:** declarative panels, commands that go through the keymap and the command palette, context-menu items; the Plugins and Console panels.
- [ ] **12g, developer experience and documentation:** `fanimate-cli plugin new | build | test | pack`, hot reload, a reference generated from the schema, a gallery of worked studio examples.
- [ ] Tests: the conformance suite in Python, TypeScript and WASM; permission denial; malformed JSON-RPC fuzzing; resource limits; a crashing plugin leaves the app intact; headless and GUI give identical documents; the API-surface snapshot.

Showcase and templates: the scripted or procedural film starts; outside plugin authors are recruited.

Not in M12, and why: a bundled Python interpreter (never: Python is a client); sandboxing Python (Phase 3, decision D3-3); the marketplace (Phase 3).

### M13: Intelligence

Goal: local, off-by-default assistive tools (P8) that always produce editable, labelled layers. Covers WS7.

Exit check: on a machine with no network, a tester cleans up a rough walk cycle, gets suggested inbetweens they edit by hand, and auto-rigs a layered character; the file says what was generated.

Depends on: M8 (rig templates), M9 (captions), M10 (colour marks), M12 (the API and the permission model). The model and data policy (D2-6) and the dataset collection must be settled before any model is chosen.

- [ ] **13a, runtime and models:** the `fanimate-ml` crate behind a trait; an optional ONNX Runtime download; signed, checksummed models with a licence file each, `docs/legal/models.md` and a release check; a model manager in Preferences; a "Turn off all AI" switch and a policy file.
- [ ] **13b, classical baselines first:** stroke fitting for line clean-up, stroke correspondence and arc interpolation for inbetweens, layer-name heuristics for auto-rig. They need no model and set the bar a model must beat.
- [ ] **13c, models, only where the licence and provenance policy allows:** raster centreline extraction, a keypoint model for flat-art auto-rig, region embeddings for colour assist.
- [ ] **13d, the rest:** speech to text for captions, a forced-alignment model for lip sync plus (Rhubarb stays the default), pose from a photo (video waits for video layers).
- [ ] **13e, the interface:** the AI panel; results as ghosts with Accept and Reject; one `Action` and one undo step to accept; progress and cancel off the UI thread; first-use consent saying what runs locally; `provenance` and a badge on everything generated.
- [ ] Tests: signature and checksum failures refuse a model; every tool works with the network blocked; with AI off no model loads and no tool shows; a damaged model never panics; results on the test sets are published.

Showcase and templates: one example shows line clean-up and inbetween assist, labelled; no film depends on a tool being on.

Not in M13, and why: cloud AI and credits (Phase 3); any model whose licence or training data is unclear; the command assistant (it needs the API and the cloud gateway).

### M14: Output

Goal: work leaves Fanimate as web animation, game assets and files other tools read (P19), with an honest report of what each route could not carry. Covers WS8.

Exit check: the character-rig template exports to a dotLottie that plays in two players with a size estimate that matched, to a sprite sheet with named clips, and to the open runtime format that the TypeScript and Godot runtimes play.

Depends on: M8 (baking from physics and deformers), M9 (text outlines), M11 (graph baking, passes).

- [ ] **14a, the exporter base:** the report type shared by every exporter, baking of rigs and drivers into keys with a tolerance and a size estimate, the fidelity ladder in the export window.
- [x] **14b, Lottie and dotLottie (built, baked):** `fanimate_export::lottie`: every frame is evaluated like a render and written as shape layers with a path, a fill or a stroke per shape, each property keyed only where it changes (a still pose has no keys; a path whose vertex count changes holds instead of interpolating); a track missing on some frames is hidden there; text is outlines already; the dotLottie is a ZIP with a manifest. Limits of 20,000 frames and 3,000,000 keys fail with a message. Reported, never silent: pictures left out, width profiles drawn evenly, see-through shapes blended in sRGB, shapes with unusable points. `fanimate-cli export x.fnm -o x.lottie` (or `.json`), and File > Export for Web and Games > Export as Lottie in the app (background thread). Checked by 10 unit tests and by playing the Character rig in the open-source lottie-web player (under jsdom): three frames matched the GPU render with a mean error of 0.37 of 255. **Open:** a second independent player, the Lottie JSON schema validator, easing and trim paths kept as such instead of baked (smaller files), pictures, precompositions for symbols, a size estimate before writing.
- [x] **14c, SVG, one frame (built):** `fanimate_export::svg::frame`: the evaluated frame as paths with fills, strokes, opacity and the camera; hidden edges and stroke exposure cut the stroke path; the same report as Lottie. Checked by 4 unit tests and a test that rasterises the SVG with resvg and compares it with the GPU render (opaque shapes match exactly; see-through ones differ by colour space, which is reported). `fanimate-cli render x.fnm -o x.svg --frame N` and File > Export for Web and Games. **Open:** animated SVG, pictures as `<image>`.
- [x] **14d, sprite sheets (built):** `fanimate_export::sheet`: frames rendered like an export, trimmed to their ink (the atlas keeps the offset), frames equal to an earlier one share its picture, shelf packing with padding up to a chosen width, and a TexturePacker JSON-hash atlas (read by Phaser, PixiJS, Godot and others) with a `clips` list. Clips are named by frame ranges (`--clips walk=1-24,idle=25-48`); the app exports the whole animation as one clip, 256 pixels on its longest side. Checked by 7 tests (parsing, trim, packing without overlap, atlas, sharing, size refusal, a rendered clip). **Open:** clips from actions, rotated packing, other engine layouts (Unity, Aseprite), a dialog for sizes and clips.
- [ ] **14e, the open runtime format** (bones, slots, meshes with weights, animations, Smart Bones as driven morph targets) and reference runtimes in TypeScript, Godot and Unity.
- [ ] **14f, interchange:** layered EXR passes (from M11), OpenTimelineIO, and, as stretch goals, a compositor handoff package and interactive HTML5 publishing.
- [ ] Tests: a corpus of small projects covering every supported and unsupported feature with golden outputs and a check of the report; baked rigs match the canvas frame by frame; hostile projects export or fail with a message.

Showcase and templates: the web banner and game character demo films are made.

Not in M14, and why: Spine-compatible JSON waits for counsel (D2-7); Lottie import is a stretch; an Unreal plugin is Phase 4.

### M15: Showcase and 1.0

Goal: close the phase and launch. Defined in [release-1.0.md](release-1.0.md#7-the-path-to-the-gate). Covers WS11 and the Gate 1 criteria in section 6.

Exit check: every Gate 1 criterion in section 6 passes.

Depends on: M7 to M14, and the people-limited work that has run beside them: the films, the examples and the handmade templates.

- [ ] **15a, freeze and showcase:** the feature freeze (firm); at least 10 films, the flagship film, the show-reel, 40 examples and 8 walkthroughs, each reviewed with its project file; every shipping template handmade with a provenance row and a signed licence; the release check that no synthetic content ships.
- [ ] **15b, public beta of at least 8 weeks:** the scripting API in beta with outside authors, then the API freeze; the crash-free measurement; the Moho corpus at 90%; performance, crash and licence audits; documentation.
- [ ] **15c, release candidate and launch:** legal sign-off including the template assets; code signing and notarisation; the site, showcase page, downloads and release notes; support and moderation in place.

Not in M15, and why: no feature is added after the freeze; if a film or a template is late, the launch waits or the weakest item is dropped.

## Contents

0. [Status and the milestones M7 to M15](#status)
1. [Where Phase 2 starts](#1-where-phase-2-starts)
2. [Phase 2 scope](#2-phase-2-scope)
3. [Rules that apply to every workstream](#3-rules-that-apply-to-every-workstream)
4. [Workstreams](#4-workstreams)
5. [Milestones](#5-milestones)
6. [Gate 1 at the end of Phase 2](#6-gate-1-at-the-end-of-phase-2)
7. [Phase 2 risks](#7-phase-2-risks)
8. [Decisions needed](#8-decisions-needed)
9. [First 30 days](#9-first-30-days)

---

## 1. Where Phase 2 starts

### 1.1 Entry conditions

Phase 2 follows the rule Phase 1 set for itself: building ahead is fine, closing early is not. Phase 1 no longer ends at a public release (section 2 of [release-1.0.md](release-1.0.md)); it ends at a close-out, so Phase 2 starts from a closed alpha, not a public beta. Concretely:

- **M7 (foundations and carry-over) may start before the Phase 1 close-out is finished**, because it changes no behaviour a tester sees except bug fixes, and it makes the later work cheaper. It must not delay a stability release.
- **Phase 2 features go to the closed alpha as they are built.** Testers and the film makers need them in real builds long before 1.0. The project rule that later-phase features are not added during Phase 1 unless the user asks still applies until the owner starts Phase 2 in writing; until then they live behind a build feature (`--features phase2`) or on a branch.
- **A fix for something an animator hit comes before any Phase 2 feature.** The Phase 1 rule "fixing what an exit check finds comes before new features" carries over.
- **Gate 1 is not a calendar date, and it is now the gate for everything.** The public beta, the crash-free figure, the Moho import rate, the frozen API, the films and the handmade templates all have to be ready (section 6). Nothing here is released publicly before then.

### 1.2 What Phase 1 leaves behind

These were promised in the Phase 1 scope or listed under "Not in M3/M4/M5, and why" or "Not built in this pass". Each needs a home, so none is lost. "State" is as of 0.3.2.

| Item | Source | State | Proposed home |
|---|---|---|---|
| Rig templates (biped, quadruped, bird, face) | Phase 1 WS5, M3 note | Not built; needs artwork and testers | WS9, M8 |
| Retargeting between rigs | Phase 1 WS5, M3 note | Not built | WS9, M8 |
| Physics (springs, wind, gravity, bake to keys) | Phase 1 WS5, M3 note | Not built; the first thing Phase 1 allowed to slip | WS9, M8, on top of the stateful-node cache in WS5 |
| Per-limb FK/IK blending | M3 note | Not built; "if testers ask" | Only on tester request (section 2.4) |
| Moho actions and Smart Bones on import | M5 note | Not imported; needs corpus files | WS1 carry-over, M7, when the corpus has files |
| Animate shape tweens and motion tweens | M5 note | Held at their keyframes | WS9 (shape tweens with hints), M8 |
| Project sync between the web build and desktop | M5 note | Not built; projects move as zip files | Phase 3 (Hub), see the [Phase 3 plan](phase-3.md) |
| Fedora RPM | M5 note | Not built | Only if testers ask |
| Sound in AV1 export, MP3 and OGG export | M4 note | Not built | WS8 (when an Opus encoder exists in Rust) |
| Video layers (reference clips) | [video-import.md](video-import.md) | Design note only | WS1 carry-over, M7, using that note's open questions as the starting point |
| A keyed pivot | UX pass 3 | Needs a format field and a rule for what it does to the look | WS9, small item |
| Offset Path | UX pass 3 | Not built | WS9, small item |
| Eraser and Freehand snapping | UX pass 3 | Not built | WS9, small item |
| Moho keys for the camera tools, Hide Edge, Go to Rest Pose, Pick Pivot | UX pass 3 | Left unbound; Moho's own keys are not documented where we may take them | Stays unbound until the animators supply the keys they use |
| Before and after numbers for the UX pass 3 changes | UX pass 3 | Pending | M7, before any new per-frame code |
| Change tracking (only recalculate what changed) | Phase 1 WS1 | Carried to M3 in the Phase 1 plan; confirm its status in M7 | WS1, M7, because the node graph and physics need it |
| Krita `.kra` import | Vision migration table (version 1.0) | Not in the Phase 1 scope text, which lists PSD only | WS3, M10, with raster layers (there is little to import until a raster layer exists) |
| Lottie import | Vision migration table (version 1.x) | Not built | WS8 stretch |
| Moho and Animate import corpus measurement | M5 | Waits on files with consent | Continues; Gate 1 depends on it |

### 1.3 Vision features no phase owns

The vision describes these in its Animate and Harmony tables but gives them no phase in its roadmap table or appendix, and Phase 1 did not list them. A grep of the codebase finds no graph editor, pose library, simple mode, canvas rotation or linked palettes. The vision calls them part of what makes Fanimate "a credible alternative to Harmony and Animate", so they need an explicit decision, not silence.

| Feature | Vision section | Value for an animator | Cost | Proposed home |
|---|---|---|---|---|
| Symbols and a Library (group with its own timeline, loop options, instances) | Taken from Animate | Very high for Animate converts; also what text templates and game export want | High: new layer kind, evaluation, format | **Committed**, WS9 |
| Graph editor (value against time, bezier handles) and easing and motion presets | Taken from Animate; timeline section | Very high; the vision lists "dope sheet, graph editor and X-sheet" | Medium | **Committed**, WS9 |
| Master controllers: Smart Bone actions as on-canvas sliders, dials and 2D pads | Taken from Harmony | High for rigs that animators share | Medium | **Committed**, WS9 |
| Pose library and action library | Timeline section | High; cheap once actions exist | Low to medium | **Committed**, WS9 |
| Simple mode (hides advanced panels) | Taken from Animate | Matters for schools; it is the education programme's base | Low | **Committed**, WS10 |
| Canvas rotation for drawing | Layout table | High for drawers; tablets need it | Low | **Committed**, WS10 |
| Shape tweens with hints | Taken from Animate | High for Animate converts; also fixes the importer gap | High: needs shape correspondence | **Conditional**, WS9 |
| Animation layers (stacked, weighted) | Timeline section | Medium to high | Medium | **Conditional**, WS9 |
| Curve and envelope deformers | Taken from Harmony | High for Harmony converts | Medium to high | **Conditional**, WS9 |
| Asset Warp (pins that bend art without bones) | Taken from Animate | Medium | Medium; extends the image mesh to vector art | **Conditional**, WS9 |
| Multiplane depth, 3D camera, glTF models | Taken from Harmony | Medium | Very high: changes the transform pipeline and adds a 3D path | **Defer to Phase 4** (section 2.4) |
| Interactive HTML5 publishing with TypeScript actions | Taken from Animate | Medium; Animate's legacy use is banners and e-learning | Medium once the web player and scripting host exist | **Stretch**, WS8 |
| Layer parenting dropdown | Taken from Animate | Medium | Low; confirm what already exists before building | Check in M7 |
| Harmony-style keymap and full key remapping | Interface section ("follow later") | Medium | Low to medium | **Stretch**, WS10 |
| Live link for SVG (re-import keeps rigging attached) | SVG section | Medium | Medium | **Stretch**, WS3 |

Section 2.3 says what leaves Phase 2 for each of these that is accepted. The project's rule is that any addition needs a removal.

### 1.4 What the animators told us (to be filled in)

The Phase 1 plan names the missing input: "No advisers are recruited yet". Animators have now used 0.3.1. Before M7 closes, write down in this section, with their consent:

- the five things they did most, and the five that blocked or annoyed them most;
- which of section 1.2 and 1.3 they asked for, unprompted;
- which Phase 2 headline feature (text, paint, node graph, scripting, AI, export) they would use first.

The order of milestones in section 5 is my reading of dependencies and the vision. If the animators' answers disagree with it, the answers win.

---

## 2. Phase 2 scope

### 2.1 In scope (committed, from the vision)

| Area | Deliverable | Pain points |
|---|---|---|
| Text | Text layer, per-character, per-word and per-line animators with range selectors, text on a path, variable fonts, OpenType features, right-to-left and complex scripts, a template gallery, SRT import, caption styling, auto-transcription | P3 |
| Paint | Raster layers and one brush engine for raster and vector, Krita-class brushes, brush import | P10 |
| Colour | Gap-tolerant bucket, colour marks tracked across frames with a review panel, palettes linked to styles | P11 |
| Pipeline | Optional node graph synced with the layer list: compositing nodes, rig-logic nodes, render passes | P4 |
| Automation | Scripting API: Python, TypeScript and WASM plugins; headless mode; semantic versioning; frozen at 1.0 (Gate 1) | P18 |
| AI | Local AI tools, off by default: line clean-up, inbetween assist, auto-rig, colour assist, lip sync plus, pose from reference | P8 |
| Output | Lottie and dotLottie, animated SVG, sprite sheets, game-engine export, layered EXR passes | P19 |
| Quality | Performance budget kept, stability release train continues, a public beta at the end | P14, P16 |
| Showcase (moved from the old Gate 2, enlarged) | Films, worked examples, a show-reel and handmade templates, all made before the public release ([release-1.0.md](release-1.0.md)) | P20 |

### 2.2 Carried over

Everything in section 1.2 that has a home in Phase 2 is in scope, in the milestone named there.

### 2.3 Proposed additions, and what leaves for each

Section 1.3 lists them. The proposal is to commit the six "Committed" rows and treat the four "Conditional" rows as the first to be dropped. The exchange, so the scope stays believable:

| Added to Phase 2 | Leaves Phase 2 (moves to Phase 4 or is dropped) |
|---|---|
| Symbols and Library | Lottie import (stretch anyway); the After Effects, Natron and Blender project export; Unreal plugin |
| Graph editor and easing presets | The live link for SVG; the Harmony-style keymap |
| Master controllers, pose and action library | Interactive HTML5 publishing (stretch) |
| Simple mode, canvas rotation | None: small |
| Shape tweens with hints, animation layers, curve deformers, Asset Warp (conditional) | Each is added only if physics or the AI tools slip; if both stay on time, none of these four is built in Phase 2 |

### 2.4 Out of scope for Phase 2

| Deferred feature | Moved to |
|---|---|
| Cloud AI generation (background and prop generation, script to animatic, command assistant), AI credits | Phase 3 |
| Marketplace, Fanimate Hub, cloud render, accounts, billing | Phase 3 |
| Tablet apps, live co-editing, Harmony import, education programme, certification | Phase 4 |
| Multiplane depth, 3D camera and glTF import | Phase 4 |
| Per-limb FK/IK blending | Only if testers ask; not planned |
| Unreal Engine plugin | Phase 4 or a community plugin |
| A bundled Python interpreter | Never: Python is a client of Fanimate (WS6) |

---

## 3. Rules that apply to every workstream

These come from `CLAUDE.md` and apply to Phase 2 work unchanged. They are repeated here because the new features touch each one.

1. **The file format is a public contract.** Every new layer kind or field needs: a spec update in [`fnm.md`](../spec/fnm.md), a version bump when an older reader would misread it, a migration, validator coverage, a round-trip test, and a place in `damaged_projects.rs`. Planned bumps are batched at most once per milestone. Section 4 (WS1) adds a `requires` list so a newer file is refused by an older reader with a clear message instead of being silently half-loaded.
2. **Never lose work.** A new layer kind's data goes through the same atomic save, the journal and the per-layer history (`HistoryState` shares unchanged layers by `Arc`; raster tiles and text caches must fit that).
3. **Fast by default.** The Gate 1 budget stays a release blocker: 60 fps for the benchmark rig, open under 3 s, no UI action over 100 ms. Per-frame code must not allocate needlessly. Every WS gives before and after numbers from `fanimate-cli bench`, and new benchmark scenes are added (section 6).
4. **Keep the crate rules.** `fanimate-core` stays free of UI, GPU and file IO and keeps compiling to WebAssembly. `fanimate-editor` stays free of egui. New crates point downwards only.
5. **Every command is an `Action`**, listed in `Action::edits_document` if it changes the document, bound in **both** keymaps (or deliberately left unbound in Moho), shown in the command palette, with a right-click menu where it applies. One gesture is one undo step. Nothing is edited while playing.
6. **The keying rule holds** for every new animatable property: frame 0 changes the value, a later frame sets a key and seeds frame 0.
7. **No modal dialogs for properties.** New settings live in docked panels. Modals are for decisions (first use of AI, a missing font).
8. **Importers and exporters never panic and always report** (`Notes`, counted by kind). A new exporter returns the same kind of report of what it simplified.
9. **No network dependence, no telemetry by default.** Optional downloads (AI models, fonts) are user-started, signed and checksummed, and everything works without them.
10. **Clean room.** Nothing from a closed product's code, icons, artwork or text. File-format knowledge comes from public documentation and from files we may use. Rule 1 applies to brush formats and game-engine formats as much as to Moho.
11. **Licences.** Every dependency and model must pass `scripts/licences.py --check`. The candidate crates named below are starting points to evaluate, not decisions; confirm licence and age (at least two weeks old) with `cargo search` first.
12. **UI text has no em dashes**, uses Lucide icons, takes colours from `brand/tokens/tokens.json`, and is legible on a 1366x768 screen.
13. **API-first (new in Phase 2).** A feature is not finished until it is reachable from the scripting API (WS6) with a test. This is what lets the API be frozen at 1.0 without a feature arriving after it.

---

## 4. Workstreams

Eleven workstreams. WS1 comes first because the others stand on it; WS2 to WS8 are the vision's headline items; WS9 is the carry-over and the conditional depth; WS10 and WS11 cut across.

### WS1. Foundations and the API-first core

**Goal.** Make the engine ready to take new layer kinds, graphs, plugins and models without each one inventing its own plumbing, and clear the Phase 1 leftovers that block them.

**Design.**

- **A command and query surface, `fanimate-api`** (new crate, above `fanimate-editor`, no egui). The UI drives the editor through `Action`, which is selection-based and mostly parameterless. Scripts need to name their targets. The API adds explicit-target operations (`layer.add_key(layer_id, channel, frame, value)`), each implemented by the same `Editor` methods the UI already uses, so there is one code path to test. Widgets still contain no editing logic.
- **Stable ids.** Layers, bones, shapes, actions, media and palette entries need ids that survive save, reload, undo and import. Layers already have ids; audit the rest and add the missing ones in the format version that M7 introduces.
- **Transactions.** A script run, a node-graph edit or an AI accept is one `Editor::checkpoint` and so one undo step. An error part-way restores the snapshot. A transaction records the document revision it started from; if the user edited meanwhile it is retried or refused, never merged blindly.
- **Events, coalesced.** Plugins get `revision_changed`, `selection_changed`, `frame_changed`, delivered at most once per UI frame. There is no per-frame callback into plugin code.
- **Read model.** The API's data model is the one in `docs/spec/fnm.md`, so a plugin author reads one document. The API never exposes egui, wgpu or winit types, because their versions are coupled and change.
- **Required features in the file.** The manifest gains a `requires` list (for example `["text@1", "raster@1"]`). A reader that does not know a name refuses the file and says which Fanimate version is needed. This answers the open question in the video-import note ("it must refuse the file rather than drop the layer silently") once for every future layer kind.
- **Provenance.** Layers and media get an optional `provenance` record (tool, model, version, date) so AI output is labelled in the file, as the vision requires. It is added now so WS7 and Phase 3 need no second bump.
- **Change tracking and the stateful-node cache.** Evaluate only what changed (the Phase 1 task never confirmed complete) and add a per-node cache keyed by input hash, with a rule for stateful nodes (physics, followers): their state is a pure function of (graph, frame 1 to N), cached every K frames, and scrubbing evaluates from the nearest cache. This is the one hard problem the node graph and physics share, so solve it once.
- **Offline switch.** One setting, and a policy file for managed installs, turns off every optional network and AI feature.

**Tasks.**

| Task | Detail |
|---|---|
| Triage | Walk section 1.2 and 1.3 with the owner; fix the homes; record decisions D2-1 to D2-4 (section 8) |
| `requires` and provenance | Spec, validator, migration, tests; the format bump for M7 |
| Stable ids audit | Every addressable object has an id; ids survive import |
| Transactions | `Editor` transaction helper with rollback; used by API, graph and AI |
| Evaluation scheduler | Confirm or finish change tracking; node cache; benchmark before and after |
| Carry-over | Per section 1.2: video layer (follow the note), Moho actions import (when files exist), before and after numbers for UX pass 3 |
| Benchmarks | New scenes in `fanimate-cli bench`: text-heavy, raster-heavy, effect-heavy, plugin-heavy (WS6) |
| Fuzzing | Extend `damaged_projects.rs` to run per new layer kind as each lands |

**Done when.** An empty graph, a stub plugin and a stub model all run through the new plumbing with a test each; a file with an unknown required feature is refused with the right message; the Phase 1 benchmark is no slower.

### WS2. Text

**Goal.** Text is a first-class animated object (P3): typed on the canvas, shaped correctly in any script, animated per character, and available as templates and captions.

**Design.** Text is evaluated into ordinary shapes in `eval`, so the tessellator, renderer, exporters and the web player need no text code. Layout is cached by a hash of (text, style, box); per frame only the animator transforms run.

- **Text layer.** Holds runs of styled text (font family, weight, style, size, tracking, leading, language, direction, OpenType feature list, variable axes), a box (point text or area text with wrap), alignment, and fill and stroke taken from the Style panel's model.
- **Fonts.** Bundled: Inter and JetBrains Mono (already OFL) plus a small pack of OFL fonts for the main scripts, chosen to keep the installer small (CJK fonts are tens of megabytes: they are an optional download). System fonts are discovered. A project embeds a font only when its licence and its embedding flag allow it; otherwise it stores a reference, shows a missing-font warning and offers "Convert to outlines". Cross-platform parity tests use bundled fonts only.
- **Shaping.** HarfBuzz-class shaping, bidirectional text, line breaking and grapheme clusters. Candidates to evaluate: `rustybuzz`, `ttf-parser`, `unicode-bidi`, `unicode-linebreak`, `unicode-segmentation`, `fontdb`, and `cosmic-text` or `swash` for layout (check each licence; ICU4X's Unicode licence needs an entry in the licence record). Shaping must be deterministic across platforms so renders stay identical.
- **Animators.** A text layer has a list of animators, each with a selector and properties, in the way a motion-graphics tool does it. The selector has a range start, end and offset, a unit (characters, words, lines), a shape (square, ramp up, ramp down, triangle, round, smooth), a mode (add, subtract, intersect), and an optional random order with a seed. The properties are position, rotation, scale, anchor, opacity, fill colour, stroke colour, stroke width and tracking. Each value and each selector field is an ordinary channel, so keys, interpolation, loops and the graph editor work unchanged. Blur needs the effect pipeline, so it arrives with WS5 as a per-character mask-and-blur option or is dropped.
- **Text on a path.** Any vector shape can be the path, referenced by id, with a keyed offset, a side flip, and an alignment. A closed path wraps. If the path is edited the text follows; if it is deleted the layer falls back to a straight baseline and the validator notes it.
- **Templates and exposed parameters.** A template is a document with a small list of exposed parameters (text strings, colours, durations, a font) mapped to properties. Applying a template makes a group in the open project. Phase 2 ships bundled templates (titles, lower thirds, captions, kinetic typography) **designed and built by hand by motion designers and animators, not generated by script** (the rules, the review and the provenance record are in [release-1.0.md](release-1.0.md#5-handmade-templates)), and a local template folder. Sharing through the marketplace is Phase 3, and the same exposed-parameter mechanism is what marketplace node setups and rigs will use.
- **Captions.** Import SRT and WebVTT into a caption layer (a list of cues with start, end and text, laid out in the safe area and styled by a template). Export SRT and WebVTT back. Auto-transcription is a WS7 tool using a local speech-to-text model on an audio layer, with the script option for better alignment, as Rhubarb has.
- **Import.** SVG text, Moho text layers and Animate text fields, left out today, become text layers where the font is available and outlines where it is not (the import note says which).

**Format.** A `text` layer kind and a `captions` layer kind; `requires: ["text@1"]`.

**Interface.** A Text tool (T in the Fanimate keymap, as in Animate and Illustrator; unbound in the Moho keymap until Moho's key is known). Typography lives in the Properties panel. Two stages for editing: first a text box in Properties (ships first), then typing on the canvas with a caret and selection, including input-method support for Chinese, Japanese, Korean and Indic scripts, which is the hardest piece of interface work in the phase. Right-click menus on text layers and on animators in the timeline.

**Tests.**

- Shaping goldens for each script (Latin with ligatures and kerning, Arabic with joining, Devanagari with conjuncts, Hebrew, Thai, a CJK sample): glyph ids and advances compared with recorded reference output.
- Rendering goldens: the same text renders to the same pixels on Windows, macOS and Linux (1/255 tolerance, as Gate 1).
- Hostile input: a 10-million-character string, NaN sizes, negative tracking, a font file with broken tables, zero-width box, a cyclic path reference. None panics; the validator repairs or the layer is skipped with a note.
- A title of 1,000 glyphs animating per character holds 60 fps on the benchmark laptop; editing text re-lays out in under 16 ms for 1,000 glyphs.

**Done when.** A tester types a title in Arabic and in Devanagari, animates it per word with a ramp selector, puts a line of it on a curve, saves, reopens on another platform, exports to MP4 and gets the same result.

**Open questions.** Vertical text, ruby, and text effects such as extrusion are not planned. Blur per character depends on WS5. Font embedding rules need a lawyer's note (D2-8).

### WS3. Raster layers and the brush engine

**Goal.** One brush engine paints vector strokes or pixels on any layer type, on any frame (P10). It is good enough for a rough sketch and a finished painted background, not a Krita replacement.

**Design.**

- **Raster layer.** Sparse tiles (256 by 256, RGBA, 8-bit, premultiplied, sRGB), one set per drawing so frame-by-frame raster layers work and the rough, colour, line and overlay sub-layers can each be vector or raster. Tiles are copy-on-write (`Arc`), which fits the per-layer history: an undo step stores only the tiles a stroke touched.
- **Storage.** A drawing up to 2048 by 2048 is one PNG in `media/`, named by content hash and checked with `is_media_name`; larger drawings are a tile bundle. Only the visible frame's tiles stay uncompressed in memory; a frame-by-frame layer of 300 frames of 4K art would otherwise be 19 GB. Caps on canvas size and tile count apply on load.
- **GPU brush.** Stamps are composited on the GPU onto the tile textures with float blending, written back to 8 bits. A CPU reference implementation of the same maths exists for tests and headless runs, because GPU tests skip when no adapter exists and determinism needs a reference.
- **Dynamics.** Size, flow, opacity, rotation and scatter driven by pressure, tilt, speed, direction and random. Brush tips are procedural (round, hard, soft) or a grey image; textures such as paper grain; blend modes (normal, multiply, screen, overlay, add, erase), paint behind and paint inside (Animate's paint modes); smudge and colour mixing. The existing stabiliser applies to raster strokes unchanged.
- **Vector and raster together.** The same Freehand tool paints either, by layer type; vector strokes keep the centreline and width profile. Rasterise and Trace convert between them (an own tracer or a permissively licensed one: Potrace is GPL and cannot be used).
- **Selection and transform.** Lasso, rectangle and ellipse selections on raster, move and free-transform with resampling. No filter library: blur, glow and colour correction are nodes (WS5).
- **Brush formats.** Own format first (JSON plus a tip image, documented). Then import of Krita presets and Photoshop `.abr` brushes with an import report in the same style as the importers ("3 dynamics left out"). Both are read from user-supplied files; neither format's owner's code is used. Legal review covers `.abr` (D2-8). Also OpenRaster `.ora` import and export (a zip of PNGs and XML, so cheap) and Krita `.kra` import, which is a zip as well.
- **Style panel.** Saved styles (`StyleBook`) generalise to raster brush presets; the one-line control stays.

**Format.** `raster` layer kind and a `raster` field on drawings; `requires: ["raster@1"]`.

**Interface.** A Brush panel, docked, with a live preview and no modal. Tool names follow the Draw group; Eraser, Smudge, Lasso and Transform Raster are added, with tool letters from Animate and Illustrator.

**Tests.** Deterministic stroke replay (CPU reference against GPU within tolerance); undo and redo by tile diffs; save and load round trip including a missing tile (becomes transparent plus a validator issue); memory with 300 frames; hostile sizes. Latency: a stroke at 120 Hz pen input on a 2048 by 2048 layer stays under 30 ms end to end on the benchmark laptop, and committing a stroke stays under the 100 ms rule.

**Done when.** An artist paints a textured background and a frame-by-frame raster cycle, imports their Krita brushes, saves, recovers after a forced kill, and exports the same pixels the canvas showed.

### WS4. Colour marks, the bucket and palettes

**Goal.** Colour a frame once and have it carry through the cycle (P11), and change a colour once and have it change everywhere.

**Design.**

- **Regions from line art.** The current Paint Bucket works on shapes. Colouring line art needs closed regions found from strokes. Build a planar-map step: take the strokes of a drawing, close small gaps (endpoint to endpoint or endpoint to curve, within a tolerance in document units and aligned), build a half-edge structure and enumerate its faces. The raster version thresholds alpha, closes gaps with a distance-transform dilation, and flood-fills, growing the fill one or two pixels under the line to avoid a halo.
- **Gap-tolerant bucket.** The Paint Bucket uses the region step, with a tolerance slider and a preview of the gaps it bridges. Fills go to the colour sub-layer, so line work is never damaged.
- **Colour marks.** The user places a small mark of a palette colour inside a region of a key drawing. Tracking maps each region to the same region in the next drawings. Matching uses region area, centroid after a global motion estimate, overlap (intersection over union), adjacency in the region graph and the colour history, solved as an assignment problem so no two regions claim the same mark. Splits (a hand covers a mouth) and merges are detected and treated as uncertain; a region with no predecessor is reported as unmarked. The marks are the source of truth; the fills are derived and cached, so Re-track is non-destructive. A manual mark on a failing frame restarts tracking from there.
- **Uncertainty.** Each assignment has a margin (best cost against second best) and an overlap. The score of a frame is the minimum over its regions. The **review panel** lists frames above a threshold with thumbnails and the uncertain regions highlighted, with next and previous, and "accept".
- **Palettes.** Palette entries (id, name, colour) live in the document. A fill or stroke colour can be a literal (as today, so every old file stays valid) or a reference to an entry. Editing an entry updates every use, including inside linked rigs once Phase 3 adds them, with per-scene palette variants (a night version) as the override. An "Extract palette" command turns repeated literals into entries, opt in. Keyed colours keep literal values; keying a palette entry itself is not planned.
- **AI assist** (WS7) fills the regions tracking could not decide; it works from the same review panel.

**Format.** `palettes` on the document; `ColourRef` in fills and strokes; `marks` on drawings; derived fills are not stored beyond the cache. `requires: ["palette@1", "marks@1"]`.

**Tests.** A labelled set: at least 30 sequences of 24 to 120 frames of clean-line animation with ground-truth colouring (made or donated with consent, kept in `corpus/colour/`). Proposed pass: at least 90% of region-frames filled correctly with no manual fix, and at least 90% of wrong region-frames flagged for review. A fuzz of strokes (self-intersections, tiny loops, huge counts) never panics; region extraction on 10,000 strokes stays under 100 ms for a typical drawing or runs off the UI thread with progress.

**Done when.** An animator colours the first frame of a 48-frame walk with marks, tracking fills the rest, the review panel flags the frames where the hand crosses the face, they fix two marks, and changing the shirt in the palette updates every frame.

### WS5. The node graph

**Goal.** An optional graph beside the layer list (P4) for effects, compositing, rig logic and render passes. A beginner never opens it; the layer list never lies about what the graph does.

**Design.**

- **Layer list is the structure; the graph adds to it.** Every layer and group has an implicit node, and the stack is implicit edges ending in a composite and an output. The saved graph holds only explicit nodes, their edges, parameters and positions, referring to layers by id. Reordering the list changes the implicit edges; wiring a layer away from its group's composite in the graph is stored as an override and the Layers panel shows a "routed through graph" badge on that layer. Cycles are refused when a wire is made.
- **Two graphs, one editor.** The **image graph** runs at render time and carries images, masks and colours. The **data graph** runs in `eval` before the skeleton is posed and carries numbers, vectors, angles and booleans. Ports are typed and a type mismatch shows in the node; implicit conversions are few.
- **Image nodes.** Blur (gaussian, directional), glow, colour correction (hue, saturation, brightness, curves, levels, tint), drop shadow and light-and-shadow, mask and matte (alpha, luma, shape), cutter, blend modes, transform, crop, merge, switch, grain, vignette. Each is a WGSL pass with parameters in document units so results do not change with zoom. Render targets are pooled and sized to the region of interest. Subgraphs that do not change between frames are cached. A draft mode renders at half resolution and "Cache frames" gives a RAM preview for heavy comps, so the 60 fps budget stays honest: effects are costed in the node and the status bar, not hidden.
- **Data nodes.** Constants, time (frame, seconds), math (add, multiply, clamp, map range, lerp, step, sine), seeded noise, vector ops, a **seeded random interval** node ("blink every 3 to 5 seconds": a pure function of (seed, interval index), so scrubbing is consistent), **sample channel** (read a bone angle or layer value at a frame offset), and an **expression node** with a small language (arithmetic, comparison, conditions, a fixed function list; no loops, no recursion, bounded evaluation, type-checked when edited, errors shown in the node, no scripting engine involved). Constraint nodes: look at, copy transform, limit, follow. Stateful nodes (spring follow-through, physics in WS9) use the cache from WS1.
- **Driving.** Outputs drive a bone angle or length, a layer property, a switch layer choice, a Smart Bone action value or a palette entry. A driven channel shows a badge; its own keys are kept; the mode is override or additive. "Bake driver to keys" is a command, and export (WS8) bakes automatically.
- **Render passes.** Pass output nodes, or the sub-layers (line, colour, rough, overlay), define passes. Export writes layered OpenEXR (the OpenEXR sequence writer exists from Phase 1) with a named layer per pass. An ID or cryptomatte-style pass is a stretch.
- **Interface.** A Node view tab beside Layers (as the vision says) and a full node canvas that can dock anywhere. Search-to-add menu, right-click menus on nodes, ports and wires, group to subgraph, comment frames, auto-layout, a minimap, keyboard navigation, snapping. Properties edit in the Properties panel. Per-node preview thumbnails are off by default because they cost GPU time. All actions in both keymaps.

**Format.** A `graph` entry. Nodes carry `type@version` and typed parameters, so a node can evolve without a file bump; an unknown node type needs a newer Fanimate and is refused through `requires`.

**Tests.** Headless evaluation tests for the data graph (deterministic: same frame gives the same value in any order of scrubbing); golden renders for each image node on the three platforms; cycle and type errors; round trip; hostile graphs (dangling edges, 100,000 nodes, NaN parameters, a 20-deep subgraph chain) repaired or refused by the validator, never a crash.

**Performance.** An empty graph costs nothing. An effects benchmark scene (10 nodes at 1920 by 1080) is added and its numbers published for the floor machine, which is a 2015 integrated GPU; it is not held to 60 fps, the benchmark rig without effects is.

**Done when.** A tester adds a glow and a shadow to a character, mattes a mouth inside a head, drives a blink from a random-interval node, exports a layered EXR with line, colour and shadow passes, and sees the same frame in the canvas and in the export.

### WS6. Scripting API and plugins

**Goal.** Replace Moho's scattered Lua with a proper, stable API (P18) that studios can build pipelines on, usable headless, frozen at 1.0.

**Design.**

- **Three hosts, one API.**
  - **Python is a client, not an embedded interpreter.** A `fanimate` Python package talks to Fanimate over JSON-RPC. It works with the user's own Python and packages, from a terminal, a studio tool or a render farm, against a running app or a headless instance. No interpreter is bundled and nothing depends on Python being present. (The PyPI name is reserved in the Phase 1 first-30-days list.)
  - **TypeScript runs in an embedded JavaScript engine** for panels and lightweight tools. TypeScript is stripped to JavaScript at install or build. Candidates: QuickJS through `rquickjs`, or Boa; a transpiler such as `swc`. Time and memory limits are enforced by the host.
  - **WASM plugins** (component model) for fast tools, brushes, effects, importers and exporters, in-process and sandboxed. Candidate runtime: `wasmtime`, with fuel and memory limits. The web build uses the browser's own WebAssembly.
- **Transport.** JSON-RPC 2.0 over stdio and over a local socket (Unix domain socket or named pipe). Not network-reachable by default; if TCP is enabled it binds to 127.0.0.1 and needs a token from a user-only file. In the GUI the socket is off until the user turns on "Allow scripts to connect". Large data (images, media) goes by file path or shared buffer, not inline.
- **Permissions.** A plugin manifest lists permissions: `document.read`, `document.write`, `selection`, `timeline`, `render`, `clipboard`, `ui.panel`, `fs.read:<scope>`, `fs.write:<scope>`, `net:<host>`, `process.spawn`. The user approves them. The host enforces them for WASM and TypeScript. A Python script run by the user's own interpreter cannot be sandboxed by Fanimate, so its permissions are advisory and the documentation says so plainly; Phase 3 decides what the marketplace does about that (D3-3).
- **Surface.** Projects (open, save, import, export); the document (layers, groups, shapes, bones, actions, switch, audio, media, camera, guides, styles, palettes, text, graph, marks); animation (channels, keys, interpolation, loops, order); evaluation (read-only evaluated frame); render (a frame to bytes, or a queue job); and, in the app, selection, view and **UI contributions**. UI contributions are declarative (label, button, slider, text, dropdown, colour, list) so they match the brand and the API does not leak egui; plugins can also register commands that go through the keymap, the command palette and context-menu items.
- **Headless.** `fanimate-cli` gains `serve` and `run`, using the same `Editor::run` path as the app, so the same commands give the same result with or without a window. This is also the base for render farms (Phase 3).
- **Versioning.** The API has its own semantic version. Before the freeze it is 0.x, under a visible `unstable` namespace. At the freeze (1.0) it becomes 1.0 and deprecated calls keep working for two major releases. An `api-surface.json` snapshot is checked by `pnpm release`, so a breaking change cannot ship by accident.
- **Developer experience.** `fanimate-cli plugin new | build | test | pack`; type definitions for TypeScript; stubs for Python; a Rust SDK crate for WASM plugins; a Console panel for logs and errors; a Plugins panel to enable, disable and see permissions; hot reload while developing. A plugin that crashes, hangs or runs out of fuel is stopped and reported; it never takes the app down.
- **Documentation.** A reference generated from the schema, and a gallery of worked examples chosen from real studio tasks: rename layers by a pattern, export each layer as PNG, batch lip sync, run the rig health check on a folder, import from a spreadsheet of shots.

**Tests.** A conformance suite of scripts run against each release in Python, TypeScript and WASM; permission denial; malformed JSON-RPC fuzzing; resource limits; a plugin crash leaves the app and the document intact; headless and GUI give identical documents for the same commands; the API-surface snapshot.

**Done when.** A studio technical director writes a Python script that opens 20 scenes headless, lip syncs each, runs the health check and exports MP4s overnight; a TypeScript panel and a WASM brush are installed from a folder; the API has been in beta with outside plugin authors for at least 8 weeks before the freeze.

### WS7. Local AI tools

**Goal.** Assistive tools (P8) that run on the artist's machine, are off by default, and always produce ordinary editable layers, keys and bones.

**Ground rules** (from the vision and `CLAUDE.md`): nothing is uploaded; nothing is on by default; the app works fully without them; output is editable and labelled; user projects are never used for training. Phase 2 has no cloud AI at all.

**Runtime.** A `fanimate-ml` crate behind an optional download. Two candidate runtimes: ONNX Runtime (through `ort`), which has the widest set of models and hardware back ends (DirectML, CoreML, CUDA, CPU), or a pure-Rust runtime (`candle`, `tract` or `burn` on wgpu), which shares the GPU stack and could run in the browser. Recommendation: start with ONNX Runtime as an optional component, behind a small trait, so the choice can change (D2-5). The base installer does not grow.

**Models.** Downloaded on request from Fanimate releases, signed with the same Ed25519 scheme as updates, checksummed, with a licence file each, recorded in a new `docs/legal/models.md` and checked in the release script like `licences.py`. Side-loading from a file supports offline installs. **No model enters the set unless its weights licence allows redistribution and commercial use**, and its training-data provenance is recorded. Responsible-AI licences with use restrictions need a lawyer's decision first. Where no suitable open model exists, the plan is a small task-specific model trained on commissioned or donated data with consent, not on scraped art (D2-6). This is the largest risk in the workstream.

**Tools.**

| Tool | Input | Output | First version |
|---|---|---|---|
| Line clean-up | Rough strokes (vector or raster) | Clean vector strokes on the line sub-layer; the rough is kept | A classical stroke-fitting baseline first, a model for raster centreline extraction second |
| Inbetween assist | Two key drawings, a count | Suggested drawings on new exposures | Stroke correspondence (descriptors and assignment) plus arc interpolation, a model only for ranking; for rigs, a smoothing suggestion between two poses |
| Auto-rig | Layered art (PSD or SVG) or flat art | Bones, weights and bindings from a rig template, then the health check | Name heuristics on layers first (as `cast.rs` does for the templates), a keypoint model for flat art second |
| Colour assist | Uncertain regions from WS4 | Proposed assignments in the review panel | Region-embedding similarity |
| Lip sync plus | Audio, optional script | Phonemes and mouth keys, with emotion-aware shapes | A forced-alignment model compared with Rhubarb on the same set; Rhubarb stays the default |
| Pose from reference | A photo | Keys on bone angles through a bone map, smoothed by IK | Images only; video waits on video layers |
| Speech to text (for WS2 captions) | Audio layer | Caption cues | A local speech-recognition model (candidates carry permissive licences) |

**Interface.** An AI panel, docked. Each tool shows its result as a ghost (like onion skin) with Accept and Reject; accepting is one `Action` and one undo step. Compute runs off the UI thread with progress and cancel. Preferences has a model manager (size, licence, delete) and a hard "Turn off all AI" switch. First use shows a decision dialog saying what runs locally. Generated layers carry `provenance` and a badge.

**Tests.** Signature and checksum failures refuse the model; a no-network test (the network blocked) runs every tool; "AI off" loads no model and shows no tool; a damaged model file never panics; quality sets in `corpus/ai/` with consent, and a published table of results per tool; inference time on the floor machine is recorded.

**Done when.** On a machine with no network, a tester cleans up a rough walk cycle, gets suggested inbetweens they edit by hand, and auto-rigs a layered character; the file says what was generated.

### WS8. Output: Lottie, animated SVG, game and interchange

**Goal.** Fanimate work leaves the app as web animation, game assets and files other tools read (P19), with an honest report of what each route could not carry.

**Design.** Every exporter returns a **report of what it simplified or left out**, counted by kind, shown in the status bar and kept with the export, and never panics on a bad project.

- **Fidelity ladder.** Each target offers the highest rung it can: (1) exact pictures (sprite sheet or image sequence); (2) **baked** geometry, where the rig, Smart Bones, deformers and graph are evaluated per frame into path or vertex keys, simplified to a tolerance, with a size estimate shown first; (3) a **skeletal** description that needs a runtime. The export dialog states the rung and the size before it starts.
- **Lottie and dotLottie.** Shape layers with paths, fills, strokes, gradients, keyed transforms and opacity, easing as bezier handles, groups as precompositions, images and trim paths. Text goes out as outlines by default. Rigs, meshes, Smart Bones and frame-by-frame layers are baked (frame-by-frame becomes per-frame visibility keys). Effects and expressions are left out and reported. Validated against the Lottie specification's schema, and rendered by at least two independent players in CI and compared with Fanimate's own render within a tolerance for the supported subset. The specification's licence is checked before the schema is bundled. Lottie import is a stretch goal.
- **SVG.** Export of a single frame as SVG (cheap and useful on its own); animated SVG with SMIL and CSS for simple animation.
- **Sprite sheets.** Packing with trim, padding and power-of-two options; clips from actions or time ranges; JSON atlases in a generic format and in the common engine-specific layouts; per-part sheets for skeletal use.
- **Game runtime format.** An open Fanimate format (JSON plus an atlas) for bones, slots, attachments, meshes with weights, animations, and Smart Bones as driven morph targets. Reference runtimes: TypeScript for the web, then Godot and Unity (a Godot extension and a Unity package, each in its own repository under a permissive licence). Unreal is Phase 4 or a community plugin.
- **Spine-compatible JSON.** Only after counsel decides (D2-7). Spine's file format is documented, but its runtimes are under a licence that requires users to hold a Spine licence, and the interoperability position is a legal question, not a coding one. Until then the open format above is the route.
- **Interchange.** Layered EXR passes (WS5); OpenTimelineIO export of the shot list and audio; a handoff package for compositors (rendered passes plus a script or XML describing layers and camera) for After Effects, Natron and Blender is a stretch goal to be shaped by what the pilot studios actually use.
- **Interactive HTML5.** The web player plus a project plus TypeScript actions (buttons, hit areas, frame labels, go to, play, stop) as a folder or a single page. Stretch, because it needs the JavaScript host in the browser and a web player that handles text and effects.

**Tests.** Each exporter has a corpus of small projects covering every supported and every unsupported feature, golden outputs, a check of the report, and a player-render comparison; a baked rig matches the canvas frame by frame within the tolerance; hostile projects export or fail with a message.

**Done when.** The character-rig template exports to a dotLottie that plays correctly in two players with a size estimate that matched, to a sprite sheet with named clips, and to the open runtime format that the TypeScript and Godot runtimes play.

### WS9. Animation and rig depth (carry-over and conditional)

Small and medium items from sections 1.2 and 1.3. Committed items first; each has its own tests and API entries.

| Item | Design in short | Status |
|---|---|---|
| **Symbols and Library** | Any group becomes a symbol with its own timeline and a loop mode (loop, play once, single frame). Instances are a layer kind that references a symbol id, with overrides (tint, transform, start offset). Edit in place; nested; cycles refused. A Library panel with search and folders. The Animate importer upgrades from "a group on every keyframe" to real symbols. Format bump. | Committed |
| **Graph editor and easing presets** | Value against time per channel with bezier handles and tangent modes (auto, linear, flat, stepped); multi-channel selection; normalise view. A presets panel (bounce, overshoot, ease in and out, fade) applied to selected keys, with user presets. | Committed |
| **Rig templates** | Biped, quadruped, bird and face, with bundled OFL or CC0 art in the same way as the templates, and fitting by layer names (`cast.rs` already binds names to bones for the bundled characters; generalise it). | Committed |
| **Retargeting** | Map bones by name or by hand; scale translations by proportions; keep rest offsets; foot lock as an option. Copy an action between rigs. | Committed |
| **Physics** | Spring and damper chains for hair, tails and cloth strips, wind and gravity, a ground plane, fixed-timestep integration with substeps so results do not depend on the frame rate; stateful, so it uses the WS1 cache; **Bake to keys** is the deliverable that matters for export. | Committed |
| **Master controllers** | A controller is a non-rendering object placed on the canvas: a slider, a dial or a 2D pad that drives one or more action values (a pad blends four poses bilinearly, as a head turn does). Stored in the document; operable while playing is off. | Committed |
| **Pose and action library** | User-level and project-level; drag a pose onto the timeline; mirror a pose left to right by a naming convention. | Committed |
| **UX pass 3 leftovers** | Keyed pivot (format field and rule), Offset Path, Eraser and Freehand snapping. | Committed, small |
| **Shape tweens with hints** | Morph two vector shapes of different structure: resample both to a common point count, match with hint points the user places, interpolate along arcs. Also lets the Animate importer stop holding shape tweens at keyframes. | Conditional |
| **Animation layers** | Additive and override layers of keys on a bone layer, each with a weight (keyed), mute and solo, and "merge down". | Conditional |
| **Curve and envelope deformers** | A spline or cage that deforms bound art smoothly, built on the bone-binding machinery. | Conditional |
| **Asset Warp** | Pins on vector or raster art that drive a mesh without bones, extending the image mesh. | Conditional |

**Done when** (committed items): a Moho or Animate user can build a reusable walking extra as a symbol, retarget a walk cycle between a tall and a short rig, add a spring tail that bakes to keys, and move a head from a 2D pad.

### WS10. Interface, learning and accessibility

| Task | Detail |
|---|---|
| New panels | Node view, Library, Graph editor, Brush, AI, Plugins, Console, Palette, Caption list; each dockable, with a workspace entry |
| New workspaces | Compositing, Titles, Scripting, added to Animate, Draw, Rig, Frame by Frame and Review |
| Simple mode | Hides advanced panels and tools behind one switch for students and beginners; the base for the education work in Phase 4 |
| Canvas rotation | Rotate the view with a gesture or key and reset it; rulers, guides and snapping stay correct |
| Keymaps | Every new action in both tables; a Harmony-style keymap and full remapping as a stretch |
| Icons | New Lucide icons for nodes and tools; Fanimate-drawn ones where Lucide has none |
| Built-in learning | Interactive tutorials and contextual first-use hints, as the vision asks, starting with text, paint and the graph |
| Accessibility | High-contrast theme and keyboard navigation through each new panel, including the node editor; the contrast tests pass |
| Strings | Move user-visible strings into one place, ready for Phase 4's translation work (cheap now, expensive later) |
| Documentation | The user manual grows a chapter per headline feature; the API reference; a "Coming from Harmony" glossary |

### WS11. Quality, corpus and the showcase

- **Standing quality.** The stability release train continues about monthly while the feature train runs about every four months. Crash-free tracking stays on. Before and after numbers accompany any per-frame change.
- **New corpora**, each with consent recorded and none private studio material: shaping strings per script, labelled colouring sequences, Lottie and sprite test projects, AI test sets, API conformance scripts, effect goldens.
- **Performance gates in the release script.** `fanimate-cli bench` results are compared with the last release; a regression above 10% in a budget scene fails `pnpm release` until acknowledged.
- **The showcase** is a Gate 1 criterion and the best test of the phase. [release-1.0.md](release-1.0.md#4-the-showcase-films-and-examples) defines it: at least 10 short films, a flagship film of at least 3 minutes (the old "pilot short film": made by people outside the core team, using at least four Phase 2 features such as text, raster backgrounds, colour marks, the graph and a script), a show-reel, 40 worked examples and 8 walkthroughs, every one made by hand in Fanimate, none published before 1.0. Each film starts when the features it needs reach the alpha channel (text films after M9, painted films after M10, effects after M11, scripted pieces after M12), logs every bug it finds, is triaged weekly, and ends in a written post-mortem. Production takes calendar weeks that no coding speed shortens, so recruit the makers in the first 30 days.
- **Handmade templates** replace the generated ones before release (release-1.0.md, section 5). Their briefs need only Phase 1 features, so the first four start in month 1; the Phase 2 templates (titles, lower thirds, captions, rig templates, symbol libraries) are made by hand as their features land.

---

## 5. Milestones

Numbering continues from Phase 1. The order is built around dependencies and risk: plumbing first, then the features that other features stand on, then output and hardening. Each milestone has its own subsection in the [Status](#status) section above, with the goal, the exit check, what it depends on, a checklist and what it leaves out; this table is the summary.

| Milestone | Theme | Main deliverables | Exit check |
|---|---|---|---|
| **M7: Foundations and carry-over** | Plumbing | WS1 complete; triage decisions; format bump with `requires`, provenance and stable ids; stateful-node cache; carry-over items that need no testers; animators' notes written up | Stub plugin, stub graph and stub model run through the new plumbing; Phase 1 benchmark no slower; an old file opens |
| **M8: Depth** | Things animators asked for | Committed items of WS9; WS10 simple mode and canvas rotation | A reusable symbol, a retargeted walk, a baked spring tail and a controller pad, shown by an animator |
| **M9: Text** | P3 | WS2 in three steps: static text and fonts; animators and paths; templates and captions | Section 4 WS2 "done when" on all three desktop platforms |
| **M10: Paint** | P10, P11 | WS3 and WS4 | Section 4 WS3 and WS4 "done when"; colour-mark set results published |
| **M11: Pipeline** | P4 | WS5, image graph first, then the data graph, then passes | Section 4 WS5 "done when" |
| **M12: Automation** | P18 | WS6; API beta published for outside authors | Section 4 WS6 "done when", minus the freeze |
| **M13: Intelligence** | P8 | WS7, in the order line clean-up, auto-rig, inbetween, colour assist, speech to text, lip sync plus, pose from reference | Section 4 WS7 "done when" |
| **M14: Output** | P19 | WS8 | Section 4 WS8 "done when" |
| **M15: Showcase and 1.0** | Quality gate and launch | In three steps ([release-1.0.md](release-1.0.md#7-the-path-to-the-gate)). **15a:** feature freeze; showcase complete (films, flagship film, show-reel, examples, walkthroughs); every template handmade. **15b:** public beta of at least 8 weeks, with the scripting API in beta for outside authors, then the API freeze; crash-free measurement; Moho corpus final; performance, crash and licence audits; documentation. **15c:** release candidate, legal sign-off, code signing, launch | Every Gate 1 criterion in section 6 passes |

**Dependencies.** M7 before everything. M9 text uses the graph only for per-character blur, so it does not wait for M11. M10 colour marks use the raster path and the bucket together. M11 stateful nodes feed M8 physics, so M8's physics lands after the WS1 cache and the rest of M8 does not wait for it. M13 needs M8 (rig templates), M10 (marks) and M9 (captions). M14 needs M9, M11 and the baking from M8. M12 runs alongside M9 to M11, because each of those features must reach the API as it lands (rule 13). The showcase runs alongside M8 to M14: each film or example starts when its features reach the alpha channel, and M15a does not close until the showcase and the handmade templates do. M15b cannot start before the feature freeze, because a public beta of moving features would not measure anything.

**Cut order if time runs short** (cut scope, not quality): the four conditional WS9 items; pose from reference; lip sync plus (Rhubarb stays); interactive HTML5; the handoff packages; Lottie import; the Harmony keymap; Spine-compatible JSON; vertical-text-style extras; then, only with the owner's approval, text blur and the ID pass. Not cuttable: WS1, the file-format rules, the API freeze criteria, the flagship film, the handmade templates, the licence audit. If the showcase is behind, the launch waits or the weakest items are dropped; features are never cut to make room for a film.

### 5.1 Schedule

The vision plans about six months for the features, and the public beta adds at least eight weeks, so 1.0 is about eight to nine months from the start of Phase 2. The Phase 1 experience is that code written with AI help is fast and the calendar is set by other things. For Phase 2 those are:

| Constraint | Why it takes calendar time | Gates |
|---|---|---|
| Human review | Every change is reviewed by a person; text, raster and the graph are each several thousand lines | Every milestone |
| Animators and testers | Colour-mark, brush, text and rig-depth quality are judged by artists; no advisers are recruited yet | M8, M9, M10, M13 |
| Labelled data | The colour-mark set and the AI test sets need artists' time and consent | M10, M13 |
| Model sourcing and training | Finding permissively licensed models, or commissioning and training small ones, takes weeks | M13 |
| Legal review | Brush formats, model licences, Spine-compatible export, font embedding | M10, M13, M14 |
| Outside plugin authors | The API cannot be frozen on the core team's own use; 8 weeks of beta use by others, which the public beta provides | M12, M15 |
| Films and examples | 10 films, a flagship of 3 minutes or more, 40 examples and 8 walkthroughs by people outside the core team take weeks each and cannot be rushed by coding | M15a |
| Handmade templates | Illustrators, animators, riggers, voice actors and a composer, plus licences, consent and the review panel | M15a |
| Legal and rights | Counsel reviews the licence and consent forms before the first commission and the final assets before launch | M15a, M15c |
| Code signing | Apple Developer and Windows certificate identity checks take weeks; the launch needs them | M15c |
| Public beta | At least 8 weeks, and thousands of sessions are needed to show 99.5% crash-free; this now happens once, at M15b | M15b |
| Closed-alpha stability | The stability release train and crash tracking among testers continue throughout | All |

A plausible plan is three tracks. **Track A (editor):** M7, M8, M9, M10, M11. **Track B (platform):** M12 from the end of M7, M13 once M8 and M10 land, M14 once M11 lands. **Track C (people and showcase):** animators' notes and maker recruitment in month 1; handmade-template briefs and the licence forms in months 1 to 2; the first template and film work from month 2; films and examples as features land; the flagship film from M9 onwards; the feature freeze at M15a. Re-plan after M9, when the first real builds meet the film makers.

---

## 6. Gate 1 at the end of Phase 2

Gate 1 (version 1.0) is a release decision, not a date. Since 2026-10-30 it is decided here, at the end of Phase 2, and it **includes the old Gate 2**; there is no separate Gate 2. The full list of what must be true is in [release-1.0.md, section 6](release-1.0.md#6-gate-1-criteria-now). This section holds the Phase 2 parts of it: the criteria that come from the old Gate 2, the new showcase and template criteria, and the tests the Phase 2 features add to the Phase 1 list in [phase-1.md, section 5](phase-1.md#5-gate-1-acceptance-tests).

### 6.1 Criteria that come from the old Gate 2 and from the 2026-10-30 decision

| Criterion | How it is measured | Target |
|---|---|---|
| Flagship film | Released at launch, with the definition in WS11 and release-1.0.md section 4.2 | **Ships**, with project file and post-mortem |
| Showcase | Count and coverage against the matrix in release-1.0.md section 4.3 | At least 10 films, a show-reel, 40 examples and 8 walkthroughs, each reviewed and each with its project file |
| Handmade templates | The inventory and provenance record in release-1.0.md section 5 | Every shipping template is handmade, has a provenance row and a signed licence, and passed the animator review; none is generated |
| Scripting API | The API-surface snapshot, conformance suite and outside use | **Frozen** at 1.0 after at least 8 weeks of public beta use with outside authors |

### 6.2 Supporting tests added for Phase 2

| Test | Pass condition |
|---|---|
| Phase 1 budgets hold | The Gate 1 benchmark still holds 60 fps, open under 3 s and no action over 100 ms with every Phase 2 feature present but unused |
| Text correctness | Shaping goldens pass for all listed scripts; the same text renders within 1/255 across the three platforms |
| Text performance | 1,000 animating glyphs at 60 fps; re-layout under 16 ms |
| Raster correctness and latency | Stroke replay matches the CPU reference; stroke latency under 30 ms on a 2048 by 2048 layer; commit under 100 ms |
| Raster memory | 300 frames of 2048 by 2048 raster animation stay within a stated memory budget |
| Colour marks | At least 90% of region-frames right with no fix and at least 90% of wrong ones flagged, on the labelled set |
| Graph correctness | Data graph is deterministic under any scrub order; effect goldens match across platforms; cycles and type errors refused |
| Graph cost | Effects benchmark numbers published; an empty graph costs nothing measurable |
| API conformance | The conformance suite passes in Python, TypeScript and WASM; the snapshot is unchanged since the freeze |
| Plugin containment | A plugin that crashes, hangs, exhausts memory or asks for an ungranted permission leaves the app and document intact |
| Headless equivalence | The same commands give the same document with and without a window |
| AI offline and off | With the network blocked every tool works; with AI off no model loads |
| AI provenance and licences | Every generated asset is labelled; every model has a recorded, permitted licence and verified signature |
| AI quality | Published results on the test sets; no tool ships below the bar agreed in M13 |
| Export fidelity | Lottie matches Fanimate's render within tolerance in two independent players for the supported subset; baked rigs match frame by frame; every export reports what it dropped |
| File safety | Every new layer kind is in `damaged_projects.rs` and opens, repairs or reports without a crash; round trips are byte-stable; an unknown required feature is refused with a clear message |
| Crash recovery | 100 of 100 kills recover, now including raster and text layers |
| Crash-free sessions | 99.5% across the **public** beta (M15b), as the vision's Gate 1 requires; the closed alpha gives early numbers only |
| No synthetic content ships | The release script fails if a shipping template or sample asset is generated or has no provenance row |
| Every film opens | Each showcase project opens in the release candidate, plays at the budget on the floor machine and exports; no film needed a workaround for a data-loss bug |
| Licence audit | Every crate, font, model and brush pack has an allowed licence recorded |
| Accessibility | High-contrast theme and keyboard navigation work through the new panels |
| Documentation | Every headline feature has a manual chapter, a tutorial and an API reference entry |

### 6.3 Results so far

None. Nothing in Phase 2 is built.

---

## 7. Phase 2 risks

In addition to the project risks in the vision and the Phase 1 plan.

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Scope: eight headline features plus carry-over in about six months | High | High | The cut order in section 5; the rule that an addition needs a removal; conditional items built only if others slip |
| Model licences or training-data provenance rule out the useful models | High | High | Policy before code (D2-6); classical baselines for clean-up, inbetween and auto-rig that need no model; commissioned data; drop a tool rather than ship an unclear licence |
| The API is frozen too early and is wrong | Medium | High | 8 weeks of outside beta use; the `unstable` namespace; API-first rule so every feature reaches it before the freeze; freeze only the surface the conformance suite covers |
| Python as a client is too limited for what studios want | Medium | Medium | Ask the pilot studios early; the transport is the same one an embedded interpreter would use, so embedding can still be added later |
| Plugin sandboxing is weaker than users assume | Medium | High | State plainly that Python scripts are trusted code; permissions enforced by the host for WASM and TypeScript; Phase 3 audit before any marketplace |
| Text input methods and complex scripts take far longer than planned | High | Medium | Ship the Properties text box first; on-canvas editing second; test scripts with native speakers; remove vertical text and ruby from scope (done) |
| Raster memory and speed on the floor machine (a 2015 integrated GPU) | High | Medium | Tile compression, one resident frame, document-size caps, a draft mode; publish honest numbers |
| Colour-mark tracking is not accurate enough to save time | Medium | High | The labelled set decides; the review panel makes failure cheap; manual marks restart tracking; if under target, ship the gap-tolerant bucket and palettes alone |
| The node graph and the layer list drift apart | Medium | High | The layer list is the structure; overrides are shown as badges; one evaluation path; tests that the list and graph agree after random edit sequences |
| Effects break the 60 fps promise | Medium | Medium | Costs shown in the node; draft mode; frame cache; effects benchmark published separately from the budget scene |
| Export fidelity disappoints (rigs baked into large Lottie files) | High | Medium | Size estimate before export; keyframe reduction tolerance; the report says what happened; the fidelity ladder gives a smaller route when a runtime is acceptable |
| Spine-compatible export attracts a legal complaint | Low | High | Do not build it before counsel decides; the open runtime format is the default |
| The films, examples or handmade templates slip, or are made by the core team only | High | High | Recruit in month 1; budget it; builds from the alpha channel; the definition requires outsiders; fewer, better; the freeze rule; the launch waits rather than shipping a synthetic template |
| No public feedback until 1.0 steers the features the wrong way | Medium | High | A widening closed alpha (at least 20 active testers), monthly surveys, makers as testers, an invite-only wider beta at M12 if the owner agrees (release-1.0.md, section 8) |
| One public beta has to harden a very large feature set | High | High | The cut order; the stability train; the 8-week beta is a minimum; no beta before the freeze |
| Maintaining three platforms and a web build for every new feature | High | Medium | Core, fnm, editor, render and import keep compiling to WebAssembly on every release; a feature the web build cannot run says so in the file's `requires` and in the web player |
| Format churn: five or more bumps in six months | Medium | Medium | At most one bump per milestone; the migration chain tested end to end from version 1; node types versioned inside the graph |
| The API and plugin work pulls the team away from stability | Medium | High | Stability release train unchanged; M12 runs as a separate track |

---

## 8. Decisions needed

Each has a recommendation, so the owner can answer yes or name a change.

| ID | Decision | Recommendation |
|---|---|---|
| D2-1 | When may Phase 2 code start? | M7 now, behind a feature flag; features go to the closed alpha as they are built (the public release now waits for Gate 1 at the end of this phase) |
| D2-2 | Accept the committed and conditional WS9 items and the trade table in section 2.3? | Accept the committed items; leave the conditional ones until physics and the AI tools are on time |
| D2-3 | Multiplane depth, 3D camera, glTF | Defer to Phase 4 |
| D2-4 | Should the flagship film be a commissioned short or a community call? | A small commissioned team of two or three animators the project already works with, plus a community call for the voice and music; the other films follow the process and budget in release-1.0.md (decisions D1-3, D1-4) |
| D2-5 | AI runtime | ONNX Runtime as an optional download, behind a trait |
| D2-6 | Model and data policy | Permissive redistribution licences only; documented provenance; commissioned or donated data for any model Fanimate trains; no scraped art |
| D2-7 | Spine-compatible JSON | Wait for counsel; ship the open runtime format first |
| D2-8 | Legal questions to add to the review scope | Font embedding rights; reading Photoshop brush files; model licences; game-engine formats |
| D2-9 | Python as a client library, not an embedded interpreter | Yes |
| D2-10 | Which text scripts are release blockers? | Latin, Arabic, Hebrew, Devanagari, Thai, and a CJK sample; Nigerian languages are Latin-based and covered, and the project's first target users make Yoruba and Igbo diacritics a named test |
| D2-11 | Start the dataset collection for colour marks and AI | Yes, in the first 30 days, with a consent form like the import corpus |

---

## 9. First 30 days

In priority order, starting when the owner approves D2-1.

1. **Write up the animators' feedback** (section 1.4) and revise sections 1.2, 1.3 and 5 with it.
2. **Answer D2-1 to D2-11**, and add the legal questions in D2-8 to the [review scope](../legal/review-scope.md).
3. **Recruit the showcase makers, the template crew and the review panel** (release-1.0.md, sections 4 and 5), and the first outside plugin authors, using the animators already testing. Write the licence, consent and provenance forms and settle the budget first (D1-3, D1-4), since they block the commissioning.
4. **Start the datasets**: labelled colouring sequences, shaping strings per script from native speakers, AI test sets, each with a consent record in `corpus/`.
5. **Format work for M7**: `requires`, provenance, stable ids, the version bump, migration, validator, tests; update `fnm.md`.
6. **Transactions and the stateful-node cache** in `fanimate-editor`, with tests and before and after numbers.
7. **Spike, one day each, to reduce risk**: shape a line of Arabic and Devanagari to outlines and tessellate it; stamp a brush on a GPU tile and read it back; call the editor over JSON-RPC from a Python script; run one ONNX model on the floor machine.
8. **Add the new benchmark scenes** to `fanimate-cli bench`.
9. **Create `fanimate-api` and `fanimate-ml` as empty crates** with the dependency direction checked, and reserve the package names (`fanimate` on PyPI and the npm scope for the TypeScript types).
10. **Update `CLAUDE.md`** with the new rules (API-first, required features, optional downloads) once D2-1 is answered.
