# Fanimate Phase 4: Reach

Companion to the [product vision](../product-vision.md) and the plans for [Phase 1](phase-1.md), [Phase 2](phase-2.md) and [Phase 3](phase-3.md). The first three phases make Fanimate a complete tool for animators, teams and studios on desktop computers. Phase 4 takes it where more people are: tablets, shared live sessions, the files other tools produce, classrooms, and every language. The vision lists four items for it (tablet apps, live co-editing, Harmony import, education and certification) and gives it no exit gate and no end date. This plan keeps that shape: it has **checkpoints** that test whether the work is reaching people, not a single gate.

> **Status: planning only. Nothing in this document is built.** Phase 4 is the furthest from today, so it is the least certain. Sections 5 and 6 plan the first six months in detail, the next six in outline and the rest as a direction. Revise it at each checkpoint, and expect parts to change after Phase 3 shows what studios and schools ask for.

> **What comes from the vision and what is new.** The four scope items, the platform table (tablets after 1.0), the education and certification revenue lines, the target users and the success metrics are taken from the vision. The additions in section 2.2, the designs, the checkpoint tests, the milestones and the decisions are new proposals.

## Contents

1. [Where Phase 4 starts](#1-where-phase-4-starts)
2. [Phase 4 scope](#2-phase-4-scope)
3. [Rules that apply to every workstream](#3-rules-that-apply-to-every-workstream)
4. [Workstreams](#4-workstreams)
5. [Milestones and waves](#5-milestones-and-waves)
6. [Reach checkpoints](#6-reach-checkpoints)
7. [Phase 4 risks](#7-phase-4-risks)
8. [Decisions needed](#8-decisions-needed)
9. [First 30 days](#9-first-30-days)

---

## 1. Where Phase 4 starts

### 1.1 Entry conditions

- **Gate 3 is passed**, or the owner decides otherwise in writing. Live co-editing needs the Hub and its real-time channel; the education programme needs organisation accounts and the review tools; the tablet apps need the cloud render and sync services to be useful away from a desk.
- **Some work starts earlier, because it is cheap there and expensive later:** moving user-visible strings into one place (Phase 2, WS10), a Simple mode (Phase 2), canvas rotation (Phase 2), and the research and legal review for Harmony import (section 4, WS3). Phase 2 and 3 plans already schedule the first two.
- **The long-lead work is people, not code:** teachers, schools, training partners, translators and testers on tablets. Recruit them during Phase 3.

### 1.2 What exists today that Phase 4 builds on

| Existing piece | Where | How Phase 4 uses it |
|---|---|---|
| One Rust engine that compiles to WebAssembly (core, fnm, editor, render, import) | Architecture | The tablet apps and the web editor reuse it |
| wgpu on Metal, Vulkan and WebGPU | `fanimate-render` | iPad uses Metal and Android uses Vulkan through the same renderer |
| The browser player on WebGPU | `fanimate-web` | The base of the web editor and of tablet review |
| The Hub, real-time channel and merge machinery | Phase 3 | Co-editing and class hand-in run on it |
| The scripting API and the packages | Phase 2, 3 | Importers from the community, certification checks and classroom tools are plugins |
| Simple mode, canvas rotation, strings in one place | Phase 2 | Tablet UI, classroom use and translation start from them |
| Per-layer shared history | `fanimate-fnm` | The natural unit for live changes |
| Workspaces, the command palette, both keymaps | Phase 1 | Touch-first menus and radial menus complement them |
| The floor machine (a 2015 integrated GPU) as the design target | [reference-hardware.md](../reference-hardware.md) | The same low-spec thinking applies to classroom computers and low-end tablets |

### 1.3 What the earlier phases leave behind

| Item | Source | Proposed home |
|---|---|---|
| Multiplane depth, 3D camera and glTF models | Vision (Harmony table); Phase 2 section 2.4 | WS7 |
| Unreal Engine plugin | Phase 2 WS8 | WS7, or a community plugin |
| Spine-compatible export, if counsel agreed | Phase 2 D2-7 | WS7 |
| Interactive HTML5 publishing, if not done in Phase 2 | Phase 2 WS8 stretch | WS7 |
| In-app purchases on tablets for credits and paid listings | Phase 3 section 2.3 | WS1 |
| Live co-editing | Vision; Phase 3 section 2.3 | WS2 |
| Importers for OpenToonz, Synfig and Spine, and Lottie import | Vision migration table | WS3, as community plugins |
| Harmony import | The vision's migration table says "Version 1.x", its roadmap table says Phase 4 | WS3; see D4-1 |
| Certification, school programme at scale | Phase 3 WS9 | WS4 |
| Harmony-style keymap, if not done | Phase 2 WS10 stretch | WS5 |
| Further hosted regions | Phase 3 D3-12 | WS8 |

---

## 2. Phase 4 scope

### 2.1 In scope (from the vision)

| Area | Deliverable | Pain points |
|---|---|---|
| Tablets | iPad and Android apps on the same engine, with a touch-first interface for drawing and posing | P1 |
| Co-editing | Two or more people in one scene at once, on different characters, seeing each other's changes in real time | P17 |
| Harmony | Import of Toon Boom Harmony scenes: drawings, deformers as bones, timing | P19, P20 |
| Education | A free programme for schools, certification and course content | P20 |
| Community importers | OpenToonz, Synfig and Spine through community plugins | P19 |

### 2.2 Proposed additions

| Added | Why | Cost |
|---|---|---|
| Localisation and a screen-reader pass | Schools and the vision's target users in emerging markets need other languages; accessibility is a standing rule | Medium; large for right-to-left |
| A light editor in the browser | The vision says the browser build is for "learning, review and light work", and Phase 1 left web and desktop sync unbuilt | Medium to high |
| Long-term support releases | Studios and schools cannot upgrade every four months | Low to medium, mostly policy |
| A measurement plan that needs no telemetry | The vision's success metrics need numbers; the project's rule forbids default telemetry | Low |
| Depth and 3D camera (carried from Phase 2) | Harmony users expect parallax | High; kept optional |

### 2.3 Out of scope for Phase 4

| Deferred feature | Why |
|---|---|
| A full 3D animation mode | Fanimate is a 2D tool; depth is a camera and layer feature |
| Console or VR targets | No pain point behind them |
| A phone app | The tablet apps' touch UI could scale down, but the screen is too small for the workflow; revisit on demand |
| Video editing and a full sequencer | The scope of other tools; export and interchange cover the handoff |
| An AI model trained on user work | Never, by project rule |

---

## 3. Rules that apply to every workstream

The earlier rules continue. These matter most here.

1. **One engine, one file format.** A tablet app opens the same `.fnm` file. A feature that exists on one platform and not another is shown but marked, never hidden or faked, and the file records what it needs (`requires`) so no platform silently drops data.
2. **Offline first and no telemetry by default**, including on tablets and in classrooms. A classroom with no internet works.
3. **Platform store rules are designed for, not discovered late.** Each store's rules on downloaded code, payments, privacy labels and updates are read before the design is final (section 4, WS1).
4. **No loss of data when many people write.** Co-editing and class hand-in use the Phase 3 integrity machinery and add convergence tests.
5. **Clean room for every importer.** Harmony and any other closed format are handled by the clean-room procedure in WS3. The rule about leaked or decompiled material has no exception.
6. **Fast on low-end hardware.** The tablet baseline and the classroom baseline are named devices with measured budgets, as the laptop floor is.
7. **Accessible and translatable.** Every new screen is keyboard and screen-reader reachable, has no hard-coded user text, and fits the smallest supported screen with the longest likely translation.
8. **Children's data.** Classroom features use organisation accounts and collect the minimum; nothing about a student is public by default.

---

## 4. Workstreams

### WS1. Tablet apps (iPad and Android)

**Goal.** Fanimate on the devices many animators draw on (P1), with the engine unchanged and an interface made for touch and a pen.

**Design.**

- **Shell choice (D4-2).** Three options: the existing eframe and egui shell on mobile; a custom shell built on winit and egui's wgpu back end; or a native shell (SwiftUI on iPad, Kotlin on Android) around a Rust canvas view. The project's thesis is one codebase, so spike the first two on both platforms before choosing. Known facts to verify in the spike: winit has mobile back ends; eframe's mobile support is limited; pen tilt and azimuth may need platform glue (winit reports force and altitude for touches); input-method and file-picker integration differ by platform. Fall back to the native shell for iPad only if touch quality is not acceptable.
- **Rendering.** The same renderer: Metal on iPad, Vulkan on Android (with OpenGL ES as the fallback driver list, kept short because the Phase 1 experience was that a single driver can crash the process; the backend choice stays in `fanimate_render::backends()`).
- **Pen and touch.** Pressure and tilt from Apple Pencil and Android styluses, and hover where the hardware gives it. Palm rejection from the platform. Touch gestures: pinch to zoom, two-finger pan and rotate the canvas (Phase 2's rotation), and system conventions for undo and redo (iPadOS reserves three-finger gestures for undo, redo and copy; Fanimate follows them rather than competing). Pencil double-tap and squeeze switch the tool or open the radial menu where the hardware supports it.
- **Touch-first interface.** One main panel at a time with a bottom sheet for the timeline; a radial menu for tools; on-canvas controls (the bone, pivot and handle gizmos get larger touch targets); a floating quick bar for undo, redo and the playhead; the same Actions underneath, so the command palette and every command exist. The desktop rule "right-click menus everywhere" becomes long-press menus, with the same contents. Both keymaps apply when a keyboard is attached.
- **What the first version does.** Draw (vector and raster), frame-by-frame with onion skin, pose and key rigs, the timeline and X-sheet, text edit, review, cloud render and export, sync through the Hub, and the template projects. It does not edit the node graph (it shows and plays it) and does not run local AI tools or WASM plugins on iPad (see the store rules below). Heavy rig editing (Smart Bone setup, weight painting) arrives in the second release.
- **Files.** The platform's file picker and share sheet; iCloud Drive, Google Drive and similar through the picker; Hub sync. The crash journal and atomic save work as on the desktop, with the platform's rules for background time (save on backgrounding).
- **Store rules.** Verify before the design is final, because they change: whether downloaded plugin code is allowed (the App Store restricts downloaded executable code, so marketplace data packages may be allowed on iPad while WASM plugins may not; plan data-only packages there); how credits and paid listings must be sold (in-app purchase rules and their fees, and whether outside links are allowed in each region); privacy labels; account deletion inside the app; no self-updater (the stores update the app); how the crash-report-by-the-user rule fits (a user-started report still works).
- **Performance.** Name a baseline device for each platform (an older, mid-range iPad and a mid-range Android tablet) and measure: the Gate 1 budget scaled to the device (60 fps for the benchmark rig where the hardware allows, with a published floor), project open time, memory limits (tablets kill apps that use too much; raster tile compression and one-resident-frame rules from Phase 2 are essential), and thermal behaviour in a 30-minute session.
- **Web stepping stone.** The browser build on a tablet (Safari and Chrome with WebGPU) is the first tablet product for review and light work, and the place to find touch problems cheaply.

**Tests.** Pen latency from touch to ink on the baseline devices (target under 30 ms for the raster brush); gestures (a recorded set replayed); background and kill during a save loses nothing; opening every template; a 30-minute thermal session; a screen-reader pass; the store review checklists done before submission.

**Done when.** An animator draws a frame-by-frame cycle and poses a template rig on an iPad with a pencil, syncs it through the Hub, and opens the same file on a laptop with nothing lost; the same on an Android tablet.

### WS2. Live co-editing

**Goal.** Two or more people work in one scene at once, normally on different characters, and see each other's changes as they happen (P17).

**Design.** Build in three steps so each is useful and testable. Start with the simplest model that meets the vision's example ("different characters") and extend only if users need more.

- **Level 1: presence.** Who is in the scene, their playhead, their selection and which layers they hold, shown in the Layers panel and on the timeline. The Hub's real-time channel (WebSocket, or WebTransport where available) carries it. No document changes yet.
- **Level 2: layers held by one editor, changes streamed.** Building on Phase 3's locks, a layer is held by one person at a time, and their changes to it stream to the others as layer-level updates. Other people see them live but cannot edit that layer. This is a **central sequencer** design: the Hub orders updates, so there is no need for a general merge algorithm. The unit is the top-level layer because the editor already shares history per top-level layer (`HistoryState`). For large layers the update is a diff (changed shapes, changed raster tiles, changed keys), not the whole layer.
- **Level 3: concurrent edits of one layer.** Field-level merging for layers more than one person touches, with last-writer-wins on single fields and explicit rules for lists (points, keys, drawings). Whole-shape granularity first. A conflict-free replicated data type library (candidates under permissive licences exist, such as `yrs` or `automerge`) is a possible base, but bezier paths with width profiles and keys are a poor fit for a text-oriented structure, so evaluate it in a spike before committing.
- **Undo.** Each person's undo steps only their own changes. Undoing a change to a layer someone else has since changed refuses with a message, never silently overwrites.
- **Gestures.** A remote update is applied between local gestures, never during a drag, and only to layers the local user does not hold. This keeps "one gesture, one undo step" true.
- **Scene-wide data** (camera, guides, palettes, styles, graph) belongs to a scene owner role; others request changes. This is a deliberate limit.
- **Offline.** A person who loses the connection keeps editing the layers they held; on reconnect Phase 3's three-way merge reconciles, and a conflict goes to the merge panel.
- **Bandwidth and latency.** Targets: a change appears for a collaborator on the same continent within 300 ms; a typical vector stroke update under a few kilobytes; raster updates are tile diffs and are throttled.
- **Security.** Per-scene rooms with the Hub's permissions; rate limits; no plugin code runs from a remote peer; remote data is validated like any file.

**Tests.** A deterministic simulation with random edit schedules, delays, drops and reconnects that checks all replicas converge and no committed edit is lost; property tests on the update format; a fuzz of remote updates (they are untrusted input); latency measured on real links; recorded sessions replayed.

**Done when.** Two animators in different cities animate two characters in one scene, see each other's layers move, one drops off the network for ten minutes and reconnects, and the scene is identical on both machines.

### WS3. Harmony import and community importers

**Goal.** Studios and animators leaving Toon Boom Harmony bring their work (P19, P20) without the project copying anything from a closed product.

**Procedure first.**

1. **Legal review before any code**, added to the review scope: what Harmony's licence and the law of the relevant jurisdictions allow when examining files that users own for interoperability. The EU and the US treat this differently, and this is a question for counsel, not for the code.
2. **A clean-room split.** One person examines sample files supplied with consent and writes a format description from what the files contain. A different person implements from that description. Nobody uses decompiled, leaked or reverse-engineered program material (project rule 1). The description is published like the other specifications.
3. **A research spike** (one to two weeks) establishes which artefacts are readable and by what route: the scene description, the drawing files, the palettes, the exports Harmony itself offers. Do not assume that any of these is documented; the result of the spike decides whether a faithful import is possible or only a partial one.

**Design (subject to the spike).**

- **Mapping.** Drawings become vector layers with variable-width strokes; timing and exposure become frame-by-frame layers and the X-sheet; palettes become Phase 2 palettes; pegs become group transforms or bones; deformers become bones, or curve deformers where Phase 2 built them (otherwise they are baked and the import says so); the node view becomes the Phase 2 graph for the node types that have an equivalent, and the rest are listed; cutters become mattes; camera, sound and backdrops where present.
- **Report.** The importer returns the same kind of `Notes` as the others ("2 deformers baked", "1 effect node left out") and never panics on a damaged file.
- **Corpus and "opens".** A corpus of at least 50 Harmony projects with consent, from simple to complex, with the same precise definition of "opens" as the Moho corpus (loads, layers and timing present, a short render within a stated tolerance). The first release targets 75% of the corpus; 90% is the goal by the second.

**Community importers.** OpenToonz, Synfig and Spine are community plugins, as the vision says. Fanimate provides what makes them easy: a documented importer API, the report type, a test kit that runs a folder of sample files and prints the pass rate, a template repository, and a bounty for each importer that reaches a stated pass rate on a donated corpus. Each importer's reading of a format follows that format's licence and documentation; Spine's runtime licence is a legal question for counsel (D2-7). Lottie import (Phase 2 stretch) fits here if not done.

**Tests.** The corpus pass rate; damaged-file fuzzing for the new importer in `damaged_projects.rs`; a golden render per sample; a legal checklist signed before release.

**Done when.** A tester opens ten of their own Harmony scenes, 75% of the corpus passes, and each import tells them what changed.

### WS4. Education and certification

**Goal.** Make Fanimate the tool schools and training centres choose because it is free, runs on modest computers and comes with a way to learn and be recognised (P20, P21).

**Programme.**

- **Who it serves, in order:** secondary schools and vocational colleges, universities, training centres and bootcamps, and self-learners. The vision names emerging markets, Nigeria among them, as places where software cost is a real barrier, so offline use, low-end hardware and low-bandwidth delivery are design requirements.
- **Classroom kit.** A curriculum of lessons with sample projects and rubrics (drawing and timing, rigging, lip sync, text, export), a teacher guide, a deployment guide (managed install on shared computers, no internet, a lab image, USB distribution, group policy settings that turn off cloud features), and an offline installer with templates and the documentation.
- **Classroom features** built on the Hub's organisation accounts: a teacher creates a class, hands out a template, students submit scenes, the teacher reviews with notes on frames (Phase 3), and grades with a rubric. Students under 18 have organisation accounts only and no public profile. Cloud AI can be forbidden by a policy file.
- **Simple mode** is the default for beginners, with a way to unlock the full interface.
- **Teacher training.** A short online course and a train-the-trainer day, delivered with partners.
- **Community.** A gallery for student work with consent, a forum category for teachers, and translated materials (WS5).

**Certification.**

- **Levels (a proposal):** Foundation (draw, key and export), Practitioner (frame-by-frame and rigging basics, lip sync), Technical (advanced rigs, the graph, scripting), and Educator (teaching with Fanimate).
- **Assessment is practical.** A candidate completes tasks in Fanimate, and the deliverable is checked by scripts on the Phase 2 API (the rig has IK, passes the health check, the timing is within a range) plus a reviewer against a rubric. Multiple-choice questions are a minor part.
- **Delivery.** Online with optional supervision, and in person at partner centres, because connectivity and trust in remote exams differ by region. An exam needs no more than the installer and a USB drive, so a centre with poor internet can run it offline and upload later.
- **Credentials.** Issued as open, verifiable digital credentials (the Open Badges and W3C Verifiable Credentials standards), so an employer can verify without contacting Fanimate.
- **Pricing.** A fee for certification and course content (as the vision says), regional pricing, fee waivers and scholarships, and free for the school programme itself.
- **Quality.** Item and task analysis, inter-rater agreement checks between reviewers, a process for appeals, fraud prevention, and a published description of what each level means.
- **Partners.** Approved training centres and instructors with a simple agreement and an annual review.
- **Claims.** "Certified" claims, accreditation relationships and the use of the name follow the trademark policy and legal advice.

**Measures.** Schools teaching with Fanimate (the vision's target is 200 twelve months after 1.0), number of certified people, completion rates, and employer feedback.

**Done when.** Ten schools run the first unit with the classroom kit including at least three with no internet; the first certification pilot runs with 50 candidates, the reviewers agree on at least the level set in advance, and credentials verify independently.

### WS5. Localisation and accessibility

**Goal.** Fanimate is usable in the languages and by the people who need it, without a second code path.

**Localisation.**

- **Framework.** A message format that handles plurals, gender and variables (Fluent is a candidate, with permissive Rust crates; check licences). All user-visible text comes from message files; Phase 2 moves strings into one place so this step is mechanical.
- **Right-to-left.** Mirrored layout for Arabic and Hebrew is a real project because egui's support is limited; start with a spike. Text animation and typing for these scripts already ship in Phase 2.
- **Fonts.** The interface fonts must cover the supported scripts; large fonts are optional downloads (Phase 2 WS2).
- **Platform.** A community translation platform (a self-hosted or hosted tool), glossary and style guide for animation terms (Moho terms stay unchanged, as the vision says), review by native speakers, and screenshots in context.
- **Fit.** Every translated screen is checked at 1366 by 768 with the longest likely strings; a test renders every panel in a pseudo-language of long strings and fails on clipping.
- **Order.** By demand and by the programme's partners; a starting list is Spanish, Portuguese, French, Chinese, Japanese, Arabic, Hindi, and community-led Nigerian languages (Yoruba, Igbo, Hausa) beside English, which is Nigeria's working language. Documentation and tutorials follow the interface.

**Accessibility.**

- **Screen readers.** egui can expose an accessibility tree through AccessKit; wire it to the panels, menus and dialogs. Canvas content is described by layer names and selection announcements rather than pixels.
- **Keyboard.** Every command reachable without a mouse, including the node editor and timeline.
- **Themes and motion.** The high-contrast theme from Phase 1, colour-blind-safe label and state colours, reduced-motion setting, adjustable font size and a dyslexia-friendly font option.
- **External audit** of the editor and the web apps by an accessibility specialist, with the findings published.

**Tests.** A pseudo-localisation test; every string has a source; a screen-reader walk through the main workflows on each platform; contrast checks continue.

**Done when.** The interface is complete in three languages beyond English including one right-to-left, a blind or low-vision tester completes the draw, key and export task, and the audit has no open high findings.

### WS6. The browser editor

**Goal.** Learning, review and light work without installing anything (the vision's description of the web build), with real sync to the desktop.

**Design.**

- **Scope.** Open and save a project (through the Hub or the browser's file access), review, play, scrub, change text, retime, move layers, set and edit keys, draw with the vector brush, and send a job to cloud render. No FFmpeg routes, no local AI, no WASM plugin installs; data packages and templates work.
- **Engine.** The same crates compiled to WebAssembly; the editor crate already builds for the web, so the work is the interface (the egui web target is mature) and storage (browser storage limits, the file system access API, and the Hub).
- **Limits stated in the product.** A page says what the browser version cannot do. WebGPU support varies by browser and operating system; a fallback message is shown rather than a broken canvas.
- **Classrooms.** A school with locked-down computers can use it with no install, which makes it a strong education tool.
- **Sync.** Closes the Phase 1 gap "project sync between the web build and the desktop is not built" through the Hub.

**Tests.** The same document operations produce the same files as the desktop (a shared test suite); memory limits (a large project opens or says why not); every limit is documented; accessibility checks.

**Done when.** A student opens a template in a browser on a school computer, edits it, saves to the Hub, and the teacher opens it on the desktop.

### WS7. Depth, game and publishing (carried over, optional)

These are carried over from Phase 2 and are done only if the pilots and the animators ask, since each is expensive.

- **Multiplane depth.** A Z value for every layer, a perspective camera with parallax, and depth of field as a node. This changes the transform pipeline, so it is a format bump with a migration, and it keeps flat scenes unchanged. The keying rule applies to Z and to the camera's depth channels.
- **glTF models.** Import of 3D models as layers rendered into the scene. A separate render path with lighting limits; scoped to props and backgrounds, not characters.
- **Unreal plugin**, if no community plugin exists. Unity and Godot runtimes (Phase 2) are maintained with every format change.
- **Spine-compatible export**, if counsel agreed (D2-7).
- **Interactive HTML5 publishing**, if not done in Phase 2: the player, a project and TypeScript actions.

### WS8. Ecosystem, long-term support and sustainability

- **Long-term support releases.** One feature release a year is designated LTS: security and data-loss fixes for 24 months, no behaviour changes. Studios and schools pin to it. The feature train and the monthly stability releases continue.
- **Platform lifecycle policy.** Which operating system versions are supported and for how long, published, with notice before dropping one.
- **Coupled upgrades.** The wgpu, winit and egui versions are coupled (project rule). Plan a coordinated upgrade each year with the full test suite, the damaged-project tests and the render-parity check.
- **Marketplace growth.** A creator programme, grants and bounties for needed tools and importers, featured listings, and documentation; the vision's target is 500 extensions twelve months after 1.0.
- **Studio programme.** Case studies from the Phase 3 pilots, a partner list, and paid support contracts and training (revenue lines in the vision).
- **Community health.** A code of conduct with real moderation, a maintainer ladder, "good first issue" labels, mentoring through programmes such as Google Summer of Code and Outreachy, and release notes that credit contributors.
- **Sustainability.** Revenue from the vision's lines, grants from open-source and arts foundations, and the annual transparency report started in Phase 3.
- **Hosted regions.** Add regions when pilots show demand, starting with the regions the education programme serves.

### WS9. Measurement without telemetry

The vision's success metrics need numbers, and the project forbids default-on telemetry. This is the plan, to settle in D3-9 and D4-6.

| Metric (vision target, 12 months after 1.0) | How to measure without default telemetry |
|---|---|
| 100,000 monthly active users | Download counts, update-check counts from the release host (aggregate, no identifiers), and an opt-in anonymous ping that shows its exact content before it is enabled |
| 50,000 Moho and Animate files imported | An opt-in counter in the import report that the user can read and choose to share; the number of importer downloads |
| 50 studios using the Hub in production | Hub organisations with active projects (the services know this; the desktop does not report it) |
| 500 marketplace extensions | The registry's count |
| 99.5% crash-free sessions | Session markers and crash counts already kept on the machine, shared through the user-sent report or the opt-in ping |
| Median under 30 minutes to rig a character | Moderated usability studies each half year, and certification task timings with consent |
| 200 schools teaching with Fanimate | Registered school organisations and the partner list |

A yearly user survey supplements these.

---

## 5. Milestones and waves

Numbering continues from Phase 3, which ends at M22. Phase 4 has no end date, so milestones are grouped in waves. Wave A is planned in detail, wave B in outline, wave C as a direction.

### Wave A: months 1 to 6

| Milestone | Theme | Main deliverables | Exit check |
|---|---|---|---|
| **M23: Reach foundations** | Preparation | Shell spike for tablets; translation framework and extraction; AccessKit wiring; web to Hub sync; LTS policy; measurement plan; Harmony legal review and research spike; education advisers recruited | The spikes have written results; the framework translates one panel; web and desktop exchange a project through the Hub |
| **M24: iPad alpha** | WS1 | iPad app with drawing, timeline, pose and keys; web on tablets as review | An animator draws and poses on an iPad with a pencil and syncs through the Hub |
| **M25: Harmony import** | WS3 | Importer built from the clean-room specification; corpus collected; report | 75% of the corpus passes; the legal checklist is signed |

### Wave B: months 7 to 12

| Milestone | Theme | Main deliverables | Exit check |
|---|---|---|---|
| **M26: Together** | WS2 | Co-editing levels 1 and 2 | Section 4 WS2 "done when" |
| **M27: Classroom** | WS4 | Classroom kit; class features in the Hub; the first translations; teacher course; school pilots | Ten schools complete the first unit, three offline |
| **M28: Android and tablet beta** | WS1 | Android app; iPad beta with store review; in-app purchase compliance; accessibility audit | Both tablets pass the baseline performance tests; store reviews pass |

### Wave C: months 13 to 18, a direction not a commitment

| Milestone | Theme | Main deliverables |
|---|---|---|
| **M29: Browser editor** | WS6 | The light editor, using what M23 and M27 learned about classrooms |
| **M30: Together, deeper** | WS2 | Level 3 co-editing if levels 1 and 2 show the need; depth and 3D camera if the animators asked |
| **M31: Certification** | WS4 | Certification launch, credentials, partner programme; the first LTS release |

**Dependencies.** M23 first. The iPad app (M24) needs sync from Phase 3 and the Phase 2 raster tile compression to fit in memory. Harmony import (M25) needs Phase 2 palettes and, ideally, curve deformers, otherwise deformers are baked. Co-editing (M26) needs the Hub's real-time channel. Education (M27) needs Simple mode, organisation accounts and the review tools from Phase 3, and a first set of translations. Certification (M31) follows the classroom year, since it needs the curriculum and enough teachers.

**Cut order if time runs short:** level 3 co-editing; depth and glTF; the Unreal plugin; the browser editor's drawing; the right-to-left interface (keep text animation, which already works); Android (keep iPad), in that order. Not cuttable: the store-rule review, the clean-room procedure for Harmony, children's data rules, the data integrity tests.

### 5.1 Schedule

| Constraint | Why it takes calendar time | Gates |
|---|---|---|
| Store review | Apple and Google reviews, rejections and policy changes take weeks | M24, M28 |
| Tablet hardware | Testing needs real devices, pencils and styluses | M24, M28 |
| Legal review of Harmony | External, and the answer shapes the work | M25 |
| Harmony corpus | Needs donated files with consent | M25 |
| Schools and teachers | Terms and school years set the pilot calendar | M27 |
| Translators | Volunteers and paid translators, with review by native speakers | M27 and later |
| Certification design | Task design, rater training and piloting take a school term at least | M31 |
| Public beta of each release | The stability trains continue | All |

---

## 6. Reach checkpoints

The vision gives Phase 4 no gate. A single pass or fail test would be arbitrary for something ongoing, so these checkpoints ask each half year whether the work is reaching people and is safe. A checkpoint is a review with written results, not a release block, except where marked.

### 6.1 Checkpoints

| Checkpoint | When | Question |
|---|---|---|
| **R1** | End of wave A | Do the spikes and the first tablet and Harmony results justify the rest of the plan? |
| **R2** | End of wave B | Are tablets, co-editing and classrooms working in the hands of real users? |
| **R3** | End of wave C | Is the programme sustainable, and which direction (more platforms, more education, more studios) gets the next year? |
| **Yearly** | Each year after 1.0 | Do the success metrics in WS9 show growth, and does the steering group agree with the roadmap? |

### 6.2 Tests

| Test | Pass condition |
|---|---|
| Tablet performance | The published budgets on the baseline devices are met, including a 30-minute session without thermal throttling below the floor |
| Pen latency | Under 30 ms from touch to ink for the raster brush on the baseline devices |
| Cross-platform files | A file saved on any platform opens on every other with nothing lost; the `requires` list refuses what a platform cannot do |
| Store compliance | Each store's review passes; account deletion, privacy labels and payment rules are met |
| Background safety | Killing or backgrounding the app during a save loses nothing |
| Co-editing convergence | The simulation finds no divergent replica or lost committed edit over 1,000,000 randomised steps |
| Co-editing latency | A change reaches a collaborator on the same continent in under 300 ms (median) |
| Undo isolation | One person's undo never reverts another person's change |
| Harmony corpus | 75% of the corpus passes at first release, 90% later, under the corpus definition of "opens" |
| Clean room | The written procedure was followed and recorded; counsel's checklist is signed |
| Class safety | A student account has no public profile; cloud AI can be disabled by policy; no student data appears in logs beyond the stated minimum |
| Classroom offline | The classroom kit installs and runs with no internet on the floor machine |
| Certification validity | Reviewers agree at the level set in advance; credentials verify independently; the appeal path works |
| Localisation | Every string is translated or marked untranslated; the pseudo-localisation test passes; no clipped text at 1366 by 768 |
| Accessibility | An external audit has no open high findings; a screen-reader walk passes on every platform |
| LTS | A security fix applies cleanly to the LTS branch with the full suite passing |
| Metrics | Each metric in WS9 has a measured value or a stated reason it does not yet |

---

## 7. Phase 4 risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| The tablet shell never reaches acceptable touch quality on egui | Medium | High | Spike early (M23); native shell as the fallback for iPad; web on tablets as an interim product |
| App store rules or fees change the model (downloaded code, payments) | High | Medium | Read the rules first; data-only packages on iPad; credits bought outside the app where allowed; keep paid features off tablets if the fees do not work |
| Harmony import is not legally or technically feasible | Medium | Medium | The research spike and legal review decide before building; ship a partial import with an honest report, or none; Moho and Animate import already cover many users |
| A faithful Harmony import is impossible without material we may not use | Medium | High | Clean-room rule has no exception; accept lower fidelity |
| Co-editing causes data loss or confusing states | Medium | Very high | Layer-level leases first; simulation tests; no merge without user choice; levels shipped one at a time |
| Real-time infrastructure costs more than expected | Medium | Medium | Rooms only for scenes with active collaborators; limits; self-hostable |
| Children's data is mishandled | Low | Very high | Organisation accounts only; minimum data; counsel review; no public profiles |
| Certification loses credibility (cheating, weak assessment) | Medium | High | Practical tasks; reviewer training; appeals; start as a pilot; published level descriptions |
| Translation quality and coverage vary | High | Medium | Native review; glossary; mark untranslated text; fewer languages done well |
| Right-to-left interface is much harder than expected | High | Medium | Spike first; cut order allows deferral |
| The team spreads across platforms and the desktop quality slips | High | High | The stability train continues; desktop bugs outrank new platform features; named owners per platform |
| Low adoption in schools despite free software | Medium | Medium | Teacher training; offline kit; partner centres; local champions; measure and adjust |
| Maintaining LTS and the coupled graphics libraries strains a small team | Medium | Medium | One LTS a year; a yearly coordinated upgrade; automated test suites |
| Metrics cannot be measured without telemetry | High | Low | Proxies in WS9; honesty in the annual report about what is known |

---

## 8. Decisions needed

Each has a recommendation, so the owner can answer yes or name a change.

| ID | Decision | Recommendation |
|---|---|---|
| D4-1 | Harmony import timing: the vision's migration table says "Version 1.x"; its roadmap table says Phase 4 | Do the legal review and the research spike during Phase 2 or 3 (cheap, and the answer shapes everything); build in Phase 4 unless the Phase 3 pilot studios are Harmony shops, in which case bring it forward |
| D4-2 | Tablet shell | Spike eframe on mobile and a custom winit and egui shell on both platforms in M23; choose by touch quality; keep a native iPad shell as the fallback |
| D4-3 | First tablet platform | iPad first (one hardware family, strong pen support, more animators), then Android |
| D4-4 | Co-editing depth | Levels 1 and 2 first; level 3 only if users need it |
| D4-5 | Certification levels and pricing | Four levels as in WS4; regional pricing and scholarships; run as a pilot before launch |
| D4-6 | Measurement | The plan in WS9, with the opt-in ping reviewed by the steering group before it ships |
| D4-7 | Languages | Start with three beyond English, one of them right-to-left, chosen with the education partners |
| D4-8 | LTS cadence | One a year, supported for 24 months |
| D4-9 | Browser editor | Build a light version after M27; keep drawing and heavy rigging out of the first release |
| D4-10 | Depth, 3D camera and glTF | Only on clear demand from the Phase 3 pilots or the animators |
| D4-11 | Who may issue certificates and use the name | Fanimate and approved partners only, under a written agreement |

---

## 9. First 30 days

In priority order, starting when the owner decides Phase 4 begins (some items can start in Phase 2 or 3).

1. **Commission the Harmony legal review** and write the clean-room procedure (WS3); decide D4-1.
2. **Run the shell spikes** (D4-2) on one iPad and one Android tablet with a pen: ink a stroke, pinch and rotate the canvas, open a template, save and background the app; record latency and problems.
3. **Collect the devices**: a baseline iPad, an Apple Pencil, a mid-range Android tablet and stylus, and record the performance baselines.
4. **Read each store's current rules** on downloaded code, payments, privacy labels and account deletion, and write a one-page summary of what each means for the design.
5. **Recruit the education advisers**: two or three teachers, one training-centre lead and one student group, including in Nigeria, and ask what they need before drafting the kit.
6. **Choose the translation framework and extract strings** from one panel as a trial; start the glossary.
7. **Wire AccessKit** to the menu bar and one panel and run a screen reader over it.
8. **Spike the real-time channel**: two clients on a Hub room exchanging presence, with a killed connection; measure latency.
9. **Start the Harmony corpus** (consent form, folder layout like `corpus/`), and publish the call for files.
10. **Update `CLAUDE.md`** with the new rules (one engine and one file format across platforms, store rules, children's data) and `GOVERNANCE.md` with the LTS policy once D4-8 is answered.
