# Legal review: scope

The product vision requires a legal review before version 1.0. This document scopes it, so it can be planned and budgeted now rather than rushed at the end of Phase 1. It is not legal advice; it lists the questions a qualified lawyer should answer.

**Done so far:** trademark and domain check for the name "Fanimate" (passed in M0).

## 1. Interface and trade dress

Fanimate deliberately follows Moho Pro's layout, tool groupings, concepts and default shortcuts so Moho users can switch without relearning.

Questions for counsel:

- How far can the layout, panel arrangement and workflow follow Moho Pro before it risks a trade-dress or copyright claim, in the main markets (US, EU, UK, Nigeria)?
- Are Moho's tool names and concept names (bones, Smart Bones, actions, switch layers) safe to use? Which, if any, are registered trademarks?
- Is shipping a keymap that reproduces Moho's default shortcuts acceptable?
- Is "Coming from Moho / Animate / Harmony" wording in docs and marketing acceptable nominative use?

What we already do: own code, own icons, own visual style (purple brand, rounded UI), no use of competitors' artwork or logos.

## 2. File importers

Phase 1 ships importers for Moho (`.moho`, `.mohoproj`) and Adobe Animate (`.fla`, `.xfl`). Later: Toon Boom Harmony scenes.

Questions for counsel:

- Is writing an importer from public documentation and from inspecting files we are licensed to use lawful in the main markets? Does any end-user licence agreement prohibit it, and is such a term enforceable?
- Does interoperability law (for example the EU Software Directive) protect this work?
- What records should we keep to show clean-room development? (We already require contributors to log sources and forbid decompiled or leaked material; see [CONTRIBUTING.md](../../CONTRIBUTING.md).)

## 3. Patents

- Are there patents in force on techniques Fanimate plans to use: Smart Bones-style corrective shapes, automatic colour fill across frames, lip sync from audio, bone-based mesh deformation, vector stroke rendering?
- What is a proportionate patent-risk process for an open-source project (for example, avoiding wilful-infringement exposure while not ignoring known patents)?
- Does the Apache 2.0 patent grant cover what we need from contributors?

## 4. Licensing and dependencies

- Confirm the dual MIT / Apache 2.0 licence suits the business model (free app; paid cloud rendering, AI credits, hosting and marketplace).
- Review third-party licences: Rust crates, the Inter font (OFL 1.1), Rhubarb Lip Sync (MIT), future AI models and their training-data terms.
- Video codecs: licensing for H.264, H.265, AV1 and ProRes encoding and decoding in a free app. Since 0.2.0 Fanimate ships no H.264 encoder: MP4 uses the operating system's encoder, AV1 uses rav1e (royalty-free), and FFmpeg formats use the user's own FFmpeg. [video-export.md](video-export.md) sets out each route; counsel should confirm before 1.0 that calling the system's encoders and running the user's own FFmpeg place no patent or GPL duty on Fanimate.
- Whether a contributor licence agreement or developer certificate of origin is needed, given the inbound-equals-outbound clause in the README.

## 5. Brand

- Trademark registration for "Fanimate" and the Bone-F logo: which classes and countries.
- A trademark policy for forks and community use (currently a short note in [brand/README.md](../../brand/README.md)).

## 6. Services (before Phase 3)

Not needed for 1.0, but worth scoping early: terms of service and privacy policy for cloud rendering, AI and Fanimate Hub; GDPR and Nigeria's NDPA; marketplace seller terms and payments; age requirements for schools.

## Timing

| When | What |
|---|---|
| Now | Sections 1 to 3: the Moho and Animate importers are built (M5) and in the public alpha; and the video codec question from section 4, before 1.0 |
| Milestone M5 | The rest of section 4, once the dependency list is stable |
| Before the first template commission | The licence, consent and provenance forms for makers, voice actors and composers ([release-1.0.md](../roadmap/release-1.0.md#56-provenance-licences-and-consent)); font and music licences |
| End of Phase 2 (was M6; Gate 1 moved on 2026-10-30) | Final review of the 1.0 release, including the shipped template assets; section 5 |
| Before Phase 3 | Section 6 |

Timing follows the [Phase 1 plan](../roadmap/phase-1.md#4-milestones).
