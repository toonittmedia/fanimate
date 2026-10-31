# Roadmap

Fanimate ships in four phases, each ending at a quality gate rather than a date (see the "Roadmap, risks and success metrics" section of the [product vision](../product-vision.md)). This folder holds the plan for each phase and the design notes that sit beside them.

> **What exists and what is planned.** Only Phase 1 is being built. Phases 2 to 4 are plans: nothing in them is built, and they will change as the animators and the pilot studios teach us things. Each plan says which parts come from the vision and which are new proposals.

> **Decision of 2026-10-30: version 1.0 and the public launch come after Phase 2.** Gate 1 moved from the end of Phase 1 to the end of Phase 2, and the old Gate 2 was merged into it, so that a large body of films, examples and handmade templates exists before the public release. Until then Fanimate is a closed alpha for testers and animators. The old Gate 2 number is retired and not reused, so the name Gate 3 is unchanged. See [release-1.0.md](release-1.0.md).

## The phases

| Phase | Document | Theme | Ends at | Status |
|---|---|---|---|---|
| 1. Foundation | [phase-1.md](phase-1.md) | Engine, drawing, frame-by-frame, rigging, lip sync, import, export, installers | **Phase 1 close-out**: the exit checks of M2 to M5 with testers, no known data-loss bug, benchmarks from the other platforms. No public release | M0 to M5 built; close-out under way; closed alpha running |
| 2. Production | [phase-2.md](phase-2.md) | Text, paint, colour marks, node graph, scripting API, local AI, Lottie and game export, plus the showcase films and examples and the handmade templates | **Gate 1: version 1.0 and the public launch** (60 fps budget, 99.5% crash-free public beta, 90% of test Moho files open, frozen scripting API, flagship film, showcase, handmade templates) | Planned |
| 3. Studio | [phase-3.md](phase-3.md) | Fanimate Hub, review, cloud and network render, marketplace, cloud AI | Gate 3: studio pilots run on the Hub and the plugin sandbox is audited | Planned |
| 4. Reach | [phase-4.md](phase-4.md) | Tablets, live co-editing, Harmony import, education and certification, languages | No gate: checkpoints each half year | Planned |

Milestones are numbered across phases: M0 to M6 in Phase 1, M7 to M15 in Phase 2 (M15 is the public beta and the launch), M16 to M22 in Phase 3, M23 onwards in Phase 4.

## Other notes

| Document | What it is |
|---|---|
| [release-1.0.md](release-1.0.md) | The road to 1.0: why Gate 1 moved, the closed alpha, the showcase of films and examples, the handmade templates, the criteria and the path to the launch |
| [ux-pass.md](ux-pass.md) | The interface passes (0.2.x and 0.3.0): decisions, what was built, what was left open |
| [video-import.md](video-import.md) | Design note for video layers; no code. Phase 2 picks it up |

## How the phases depend on each other

```
Phase 1  Foundation        engine, drawing, rigging, import, export, installers
   |                       Phase 1 close-out (exit checks; closed alpha continues)
   v
Phase 2  Production        plumbing (API-first core, required features, provenance)
   |                         -> text, paint, colour marks, node graph
   |                         -> scripting API (beta, then frozen)
   |                         -> local AI, Lottie and game export
   |                       showcase: films, examples, handmade templates (alongside)
   |                       feature freeze -> public beta (8+ weeks) -> launch
   |                       Gate 1 (1.0: frozen API, flagship film, showcase)
   v
Phase 3  Studio            Hub (versions, locks, linked assets) -> review, tasks
   |                         -> cloud and LAN render, farms
   |                         -> marketplace (needs the frozen API), cloud AI and credits
   |                       Gate 3 (studio pilots, audited sandbox)
   v
Phase 4  Reach             tablets, co-editing (needs the Hub), Harmony import,
                           education and certification, languages, browser editor
```

The long-lead work in each phase is not code: testers, labelled data, legal review, accounts and payments, a security audit, store review, schools and translators. Each plan lists these in its schedule table and starts them in its first 30 days.

## Conventions used in the plans

- **Section layout.** Where the phase starts, scope (in, proposed additions, out), rules that apply to every workstream, workstreams, milestones and schedule, acceptance tests, risks, decisions needed, first 30 days.
- **Decision ids.** `D2-n`, `D3-n` and `D4-n` are decisions for the owner. Each has a recommendation.
- **Scope rule.** Any addition to a phase needs a removal. The plans keep a "cut order" so the first things to drop are agreed before they are needed.
- **Cross-cutting rules** come from [CLAUDE.md](../../CLAUDE.md): the file format is a public contract, never lose work, fast by default, clean room, offline first, no telemetry by default.
- **Update the status.** When you finish milestone work, update the Status section of the phase document. When a plan changes, say what changed and why at the top of the document.
