# The road to 1.0 (Gate 1)

Decision recorded 2026-10-30. **Gate 1, version 1.0 and the public launch, now sit at the end of Phase 2, not at the end of Phase 1.** Before the public release Fanimate must show what it can do: a large body of films and examples, and templates made by hand. This document records the decision and its consequences, and plans the showcase and the handmade templates. [Phase 1](phase-1.md), [Phase 2](phase-2.md), [Phase 3](phase-3.md) and the [roadmap index](README.md) point here.

> **Status: decision made; the plan below is a proposal.** The decision (move Gate 1; handmade templates before release) is the owner's. Everything else here, including the numbers, the process and the inventory, is a proposal to challenge, in the same way as the phase plans.

## Contents

1. [The decision and why](#1-the-decision-and-why)
2. [What each phase ends with now](#2-what-each-phase-ends-with-now)
3. [Until 1.0: the closed alpha](#3-until-10-the-closed-alpha)
4. [The showcase: films and examples](#4-the-showcase-films-and-examples)
5. [Handmade templates](#5-handmade-templates)
6. [Gate 1 criteria now](#6-gate-1-criteria-now)
7. [The path to the gate](#7-the-path-to-the-gate)
8. [Consequences and risks](#8-consequences-and-risks)
9. [Decisions needed](#9-decisions-needed)
10. [First 30 days](#10-first-30-days)

---

## 1. The decision and why

**What changes.**

1. Gate 1 (version 1.0, the public beta and the public launch) moves from the end of Phase 1 to the end of Phase 2.
2. Gate 2 (a pilot film ships; the scripting API is frozen) is **merged into Gate 1**. A public 1.0 that waits for Phase 2 cannot also wait for a separate Gate 2 afterwards, and the pilot film is exactly the kind of work the owner wants made before launch. The number 2 is retired, not reused, so the existing references to Gate 3 stay valid.
3. Phase 1 closes without a public release. It ends at the **Phase 1 close-out** (section 2): the exit checks of M2 to M5 with testers, no known data-loss bugs, and the benchmark numbers from the other platforms.
4. Before the release, **every template that ships is made by hand**. The generated templates in the app today stay as development placeholders and do not ship in 1.0 (section 5).

**Why (the owner's reasoning).** People will not understand what Fanimate can do from a feature list. A launch needs a body of finished films and worked examples that show rigging, frame-by-frame, lip sync, text, painting, effects and automation in real use. The Phase 2 features are what make most of those films possible, so the film programme needs them. And the first things a new user opens are the templates, so they must be good enough to teach by, which the generated ones are not.

**What does not change.** The 1.0 criteria themselves (section 6) are still the vision's: the performance budget, a 99.5% crash-free beta, 90% of test Moho files opening. The standing rules also hold: never lose work, fast by default, the file format is a public contract, no telemetry on by default. A later release is not an excuse for lower quality; it adds a bar on top (films, examples, templates).

---

## 2. What each phase ends with now

| Phase | Ends at | What it means |
|---|---|---|
| 1. Foundation | **Phase 1 close-out** (no public release) | Exit checks of M2 to M5 passed with testers; no known data-loss bug; the M1 benchmark numbers from macOS and Linux recorded; the import corpus above 75% (M5's exit check); the closed alpha keeps running |
| 2. Production | **Gate 1: version 1.0** | Every Gate 1 criterion in section 6, including the public beta, the frozen scripting API, the showcase and the handmade templates |
| 3. Studio | Gate 3: studio-ready | Unchanged. It starts after 1.0 |
| 4. Reach | Checkpoints | Unchanged |

Phase 1's M6 (Harden) is split. What can be done without the public stays at the end of Phase 1: bug fixing, the exit checks, the reference-hardware benchmarks and the corpus. The public beta, the legal sign-off, the show-reel and the release candidate move into Phase 2's last milestone, M15 (which becomes "Showcase and 1.0" in the [Phase 2 plan](phase-2.md#5-milestones)).

---

## 3. Until 1.0: the closed alpha

Releases 0.1.0 to 0.3.2 are already on GitHub and the self-updater serves them, so Fanimate is not invisible today. What changes is how it is presented and used until 1.0.

**Recommendation (D1-1).**

- Keep publishing alpha builds to testers through the existing release process and updater. Do not take the old releases down.
- **No launch activity before 1.0:** no announcement post, no press, no store listings, no social media campaign, no "1.0" or "release" language. The README status line and the release notes say "closed alpha for testers and animators".
- **Widen the circle on purpose:** a list of animators and students who test (aim for at least 20 active testers by the end of M9, more by M12), with a feedback channel and a short survey each month. They are also the people who make the first films.
- **Closed alpha measures what it can:** crash reports and session counts from testers give early stability numbers. They cannot prove 99.5% crash-free, which needs thousands of sessions, so the public beta (section 7) is still required.

**Alternative.** Hide the alpha releases (mark them draft or private and distribute by invitation). That protects first impressions more strongly but loses the open-source transparency the project promises, makes the updater harder and cuts off drive-by testers and contributors. Not recommended unless the owner wants it.

**Update to existing text.** The Phase 1 rule "a public alpha at M5, not at M6" is replaced by "a closed alpha until Gate 1" (the Phase 1 plan is edited to say so).

---

## 4. The showcase: films and examples

### 4.1 Goal

Someone who lands on the Fanimate site should, in five minutes, see films that make them want to try it, find the project file of any film to open in the app, and find a small worked example for whatever they want to learn first. Every headline capability appears in at least one film and at least one example, and every film and example is **made in Fanimate, by people, in release-candidate builds**.

### 4.2 Targets (proposals)

| Item | Minimum at 1.0 | Notes |
|---|---|---|
| Short films | **10**, each 15 seconds to 3 minutes | Different artists and styles; at least 6 different makers outside the core team |
| The flagship film | **1**, at least 3 minutes | The old "pilot short film" from Gate 2. At least four Phase 2 features, made by people outside the core team |
| Show-reel | **1**, 60 to 90 seconds | The "show-reel for 1.0" already listed in the Phase 1 brand assets; cut from the films |
| Worked examples ("recipes") | **40** | Each teaches one thing, with a notes layer in the project, a one-page explanation and a short clip |
| Walkthrough videos | **8** | Start from zero: draw, rig, frame-by-frame, lip sync, text, painting and colour, effects and the node graph, scripting |
| Migration stories | **3** | A Moho scene and an Animate scene each brought over, with what the importer reported |

Every number is a proposal. The point is "a lot, and varied", not a quota; fewer, better films beat more, weaker ones, in line with the project's rule of polish before features.

### 4.3 Coverage

Every capability below must appear in at least one film and one example. The matrix is the checklist the showcase is reviewed against.

| Capability | Film types that show it | Phase it comes from |
|---|---|---|
| Bone rigging, IK, Smart Bones, actions | Cut-out character comedy; a walk and run study | 1 |
| Frame-by-frame, onion skin, X-sheet | A hand-drawn cycle; a short traditional piece | 1 |
| Lip sync with a recorded voice | Dialogue scene; a talking-head explainer | 1 |
| Moho and Animate migration | A migrated scene with its import report | 1 |
| Video and web export | Every film, in MP4 and a web build | 1 |
| Text animation and captions | A kinetic-type title sequence; a captioned dialogue | 2 |
| Raster painting and brushes | A painted background; a textured, painterly short | 2 |
| Colour marks and palettes | A frame-by-frame short coloured with marks | 2 |
| Node graph effects and passes | A film with glow, shadow and mattes; a layered EXR handoff | 2 |
| Symbols and the Library | A crowd or a reused extra | 2 |
| Scripting and headless mode | A scripted batch or a procedural piece | 2 |
| Lottie, sprite and game export | A web banner; a game character demo | 2 |
| Local AI tools (off by default) | One example showing line clean-up and inbetween assist, labelled | 2 |

No film depends on an AI tool to be watchable, and AI-assisted work is labelled in the credits, as the file's provenance record says.

### 4.4 Production process

1. **Brief and kit.** Each maker gets a brief (length, capabilities to show, deadline), the current release-candidate build, a template to start from if wanted, and a bug-report route.
2. **Build channel.** Films are made on the alpha channel, updated through the normal updater, so they use what testers use. A film starts only when the features it needs are in a build (text films start after M9, painted films after M10, and so on).
3. **Bugs are the other product.** Every problem a maker hits is logged with the film's name, triaged weekly, and fixed before new features. The count and kinds of bugs per film are published in the post-mortem; they feed the hardening work.
4. **Review.** An animator on the review panel (section 5.7) and one person from the core team watch the final film and the project file: does it look good, is the file tidy, does it play at the budget on the floor machine, is it free of data-loss incidents.
5. **Credits and rights.** Makers keep the copyright of their work. Each contributes a signed licence (D1-3) that lets Fanimate show the film and distribute its project file; voice and music contributors sign consent and licence forms. No real person's likeness, no real club, player or sponsor, and no proprietary characters or music.
6. **Source included.** Every film's `.fnm` is published for download, so a viewer can open it and learn.
7. **Payment (D1-4).** Commissions are paid where the maker is a professional; volunteers get credit and a clear licence. Decide the budget before recruiting.

### 4.5 Publication

Nothing is announced before 1.0. At launch, publish the films on a showcase page with a project download each, on video platforms, in the README and in the release notes; the show-reel is the first thing a visitor sees; the examples are in the app's Home tab and the manual. A staggered "launch fortnight" (one film or walkthrough a day) gives the launch more than one day of attention.

---

## 5. Handmade templates

### 5.1 The problem

The templates in the app today (the eight added in 0.3.2, and the four before them) are generated:

- **Art from scripts.** The eight cartoon characters, the four sets and five mouth styles are SVG written by `scripts/template-art/*.mjs`.
- **Voices from a speech synthesiser.** The dialogue is Microsoft David and Zira (Windows' built-in text-to-speech), and the mouth timing comes from the synthesiser's own events (`scripts/template-speech/`).
- **Motion from code.** Key frames, including the bouncing ball and the character rig's breathing, are written by Rust and script code, not performed by an animator.
- **Procedural basics.** Walk cycle, Lip sync and Frame by frame are built from code in `fanimate-core::samples`.

They were a useful way to test the app end to end, and they exercise the importer, the rigs, the switch layers and the lip sync. But they look and sound synthetic, a new user will learn nothing about animating from them, and two of their assets have unconfirmed licences (the supplied photo-real mouth set, and Microsoft's terms for shipping synthesised speech). The owner's judgement is that they are not good enough to ship.

### 5.2 What "handmade" means

A template is handmade when **all** of these are true:

1. **Art** is drawn by a named artist in a drawing tool (Fanimate, or another tool they import from), not generated by a script, not AI-generated, not traced from someone else's work.
2. **Animation** is keyed and timed by an animator in Fanimate. Mechanical generation of keys by a script is only allowed for things nobody would animate by hand (a regular loop of a clock hand).
3. **Voices** are recorded by people who consented in writing to this use. Synthetic speech does not ship.
4. **Mouth shapes** are drawn by an artist; **lip sync** starts from Rhubarb on the recorded audio and is then corrected by an animator.
5. **Music and sound effects** are composed or recorded for the project, or are licensed in a way that allows redistribution inside an open-source app (CC0, CC BY with credit, or a commissioned licence).
6. **Provenance is recorded** for every asset: who made it, when, with what tool, under what licence, with what consent. The record is `docs/legal/template-assets.md` (new), and a release check fails if a shipping asset has no entry.
7. **It passes the quality review** in section 5.7.

Made-up characters only, as before. Never a real player, club, sponsor, brand or voice.

### 5.3 Inventory and replacement plan

Size is the relative effort: S under a week of one person's work, M a few weeks, L many weeks or several people.

| Template today | What is synthetic | Replacement | Size |
|---|---|---|---|
| Blank | Nothing (an empty document) | Keep; add a hand-drawn welcome note layer that shows the grid and guides | S |
| Walk cycle | Rig and cycle built by code | A hand-drawn character with a hand-keyed walk, a clean rig that passes the health check, and notes on timing | M |
| Lip sync | A procedural face | A hand-drawn face with a recorded line, Rhubarb-synced and then corrected by hand, with the mouth set | M |
| Frame by frame | Procedural drawings | A short hand-drawn cycle (a bouncing character or a flame) with rough, line and colour sub-layers filled in and onion skin set up | M |
| Bouncing ball | Keys per frame by script | A hand-animated ball with squash, stretch and shadow, with the timing chart as a notes layer | S |
| Character rig | Scripted SVG character; scripted breathing, blinking and nodding | A hand-drawn character from an illustrator, rigged by a rigger, with hand-keyed idle, blink and nod actions | L |
| Talking head | Synthetic voice, scripted face | A hand-drawn character, a recorded line, hand-corrected lip sync | M |
| Mouth styles | Five generated SVG styles; the supplied photo-real set of unconfirmed licence | Two or three hand-drawn mouth sets by an artist, with the supplied set dropped unless its author and licence are confirmed and it is handmade | M |
| Four football scenes (Dressing room, Press conference, Penalty shootout, Studio pundits) | Generated characters and sets; synthetic dialogue; scripted flashes | **Decision D1-2:** replace with handmade scenes, or ship fewer. Each is a short film in itself (L each). The recommendation is to ship two or three handmade scenes at 1.0 rather than four synthetic ones, and to hold the rest as showcase films | L each |

Phase 2 adds more templates (titles, lower thirds, captions and kinetic type; rig templates for biped, quadruped, bird and face; symbol libraries). **They are handmade too** and follow the same rules. The benchmark scenes and the `episode` sample are test fixtures, not templates, and stay generated.

### 5.4 Where the generated ones go

- **In alpha builds** the generated templates stay, so testers keep a working set. Their names in the Home tab and the menu carry "(placeholder)" so nobody mistakes them for the final templates (D1-6).
- **In release builds for 1.0** they are removed from the template list.
- **The generator scripts and assets** stay in the repository as test fixtures (the importer, the rig, the lip sync and the render tests use them). They move out of `crates/fanimate-editor/assets/templates/` into a fixtures location so they are not bundled into the installers, which also removes the roughly 3.5 MB they add.
- **The `CLAUDE.md` rule** that templates are generated and must be rebuilt by script describes the placeholders. For shipping templates the rule is the opposite: edit the hand-made source directly, and record it.

### 5.5 How a handmade template is shipped (D1-5)

Two ways, and the choice changes a project rule, so it needs the owner's answer.

| Option | How it works | Pros | Cons |
|---|---|---|---|
| **A. Ship `.fnm` files** (recommended) | The artist makes the template in Fanimate and saves a `.fnm`. The file is bundled as an asset and opened as a new unsaved document. Old files always open (the format rule), so a template survives format changes by migration | The simplest path for artists; exactly what a user would make; the templates are also test projects for the format; no builder code | Reverses the current rule that a template is never a stored project; binary files in the repository (size, diffs); the project must keep every shipped template opening forever |
| **B. Keep assets plus builder code** | Artists supply SVG, PNG and audio; code assembles the document | Fits the present structure | Artists' real work (timing, rigging, keys) is lost in translation to code; rigging by an artist cannot be expressed in assets; this is how the synthetic ones were made |

With option A: a size budget per template and for the whole bundle (a proposal: under 8 MB each, under 40 MB in total, less in the web build), compression of media, a decision on Git LFS for large binaries, and a test that saves and reopens every shipped template through the migration chain.

### 5.6 Provenance, licences and consent

- **The record**, `docs/legal/template-assets.md`: asset, maker, date, tool, licence, consent form id, notes. Each asset is one row.
- **The licence for makers' work** (D1-3). Recommendation: the maker keeps copyright and grants Fanimate a perpetual, worldwide, irrevocable licence to distribute the work inside the app and the repository, and CC BY 4.0 for the public (so others can learn from and reuse a template with credit). This must be compatible with the app's MIT and Apache-2.0 licensing, which counsel confirms.
- **Voice and likeness consent** in writing, naming the use (inside a free, open-source animation app, forever, worldwide), with a right to be credited. Voice actors may not be asked to sign away future AI uses without a separate, explicit clause; and recordings are never used to train a model.
- **Music** from the composer under the same licence, or from CC0 and CC BY sources with the source recorded.
- **Fonts** used inside templates must be OFL or similar, recorded.
- **Counsel review** of the forms before the first commission, added to the [review scope](../legal/review-scope.md).

### 5.7 Quality review

An **animator review panel** (three or more working animators, none of whom made the template) signs off each template against a checklist:

- **Look:** reads clearly at 1080p and at the 1366 by 768 canvas size; the drawing quality matches the showcase films.
- **Motion:** timing and spacing follow the principles it claims to teach; no mechanical or stiff motion; no popping.
- **Sound:** clean recording, balanced levels, no clipping; the lip sync is believable.
- **File hygiene:** layers and bones named in plain words; the rig passes the rig health check; no stray keys on hidden layers; palettes and styles sensible; sub-layers used as the app expects.
- **Teaching:** a notes layer explains what the template shows and how to change it, in plain, short sentences (no em dashes), and a one-page guide exists in the manual.
- **Performance:** plays at 60 fps on the floor machine, opens within the budget.
- **Safety:** opens, edits, saves, reopens and exports with no error; survives the damaged-file tests where applicable.
- **Accessibility:** captions or text alternatives for any dialogue.

A template that fails is fixed or not shipped. **Fewer, better templates** is the rule.

### 5.8 People

A realistic crew for a set of six to eight templates: two illustrators, one or two animators, one rigger (or an animator who rigs), two or three voice actors, one composer or sound designer, and a producer who runs the briefs, licences and review. Recruit from the animators already testing (section 3), from the pilot-film makers (section 4), and by open call. Budget before recruiting (D1-4).

---

## 6. Gate 1 criteria now

All of the following. The first block is the vision's Gate 1; the second is the old Gate 2 plus what this document adds.

### 6.1 The vision's Gate 1 criteria (unchanged)

| Criterion | Target |
|---|---|
| Performance budget, measured on the reference hardware | 60 fps with a 40-bone character and 10 Smart Bones |
| Project open time | Under 3 seconds for a 5-minute scene |
| UI responsiveness | No single action over 100 ms |
| Stability | 99.5% crash-free sessions across the **public** beta |
| Moho compatibility | 90% of the Moho test corpus opens |

The supporting tests in [Phase 1, section 5.2](phase-1.md#52-supporting-tests-added-for-phase-1) still apply (crash recovery, file corruption, parity, canvas against export, SVG conformance, fresh install, self-update, accessibility, usability, licence audit, legal review). The [Phase 2 tests](phase-2.md#6-gate-1-at-the-end-of-phase-2) are added to the same list.

### 6.2 The criteria from the old Gate 2

| Criterion | Target |
|---|---|
| The flagship film | Released, as defined in section 4.2 |
| The scripting API | Frozen at 1.0 after at least 8 weeks of public beta use with outside authors |

### 6.3 New criteria from this decision

| Criterion | How it is measured | Target |
|---|---|---|
| Films | Count, coverage and review | At least the minimums in section 4.2, covering every row of the matrix in section 4.3, each reviewed and each with its project file |
| Examples | Count and coverage | At least 40, each with a notes layer, a one-page explanation and a clip |
| Show-reel | Released | One, 60 to 90 seconds |
| Handmade templates | The inventory in section 5.3 | Every template in a release build is handmade; none is generated; each has a provenance row, a signed licence and, where it has a voice, a signed consent; each passed the review in section 5.7 |
| No synthetic content in the release | A release-script check | Fails if a shipping template or sample asset is missing from `template-assets.md`, or is marked generated |
| Rights | Counsel's sign-off | The licence and consent forms and the template assets are reviewed |
| Code signing and notarisation | A clean-machine install | Windows and macOS installers signed and notarised; no unknown-publisher warning |
| Documentation | Manual, tutorials, API reference | Complete for every headline feature |
| Launch readiness | A checklist | Site, showcase page, downloads and release notes ready; support route and moderation in place |

---

## 7. The path to the gate

```
Phase 1 close-out  (exit checks, benchmarks, corpus; closed alpha continues)
        |
        v
Phase 2, M7 to M14  (features; the showcase films and examples begin when the
        |            features they use reach the alpha channel: text after M9,
        |            painting after M10, effects after M11, scripting after M12)
        v
M15a  Feature freeze and showcase complete  (films, examples, handmade templates)
        |
        v
M15b  Public beta, at least 8 weeks  (scripting API in beta with outside authors;
        |                              crash-free measurement; Moho corpus final)
        v
M15c  Release candidate, legal sign-off, signing, launch   ==  Gate 1: 1.0
```

**Calendar (a proposal).** Phase 2 is planned at about six months. The public beta adds at least eight weeks. So 1.0 is about eight to nine months from the start of Phase 2, and the film programme and the handmade templates are the part most likely to set that date, since they depend on people, not code. A realistic order of work:

- Months 1 to 2: recruit makers, templates crew and review panel; write the licence and consent forms; start the template briefs (they need only Phase 1 features). Phase 2's M7 and M8 proceed.
- Months 3 to 5: films and examples begin as features land; first handmade templates reviewed; the closed alpha widens.
- Months 5 to 6: the flagship film in production; remaining templates and examples finish; feature freeze.
- Months 7 to 8 and a little more: public beta; fixes only; release candidate; launch.

**The feature freeze (M15a) is firm.** If a film or a template is late, the launch waits or the item is dropped, but no new feature is added after the freeze.

---

## 8. Consequences and risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Building for months with little public feedback leads the project the wrong way | Medium | High | A widening closed alpha with at least 20 active testers; monthly surveys; the film makers are testers; an invite-only wider beta at M12 if the owner agrees |
| More features before release means more to harden in one public beta | High | High | The Phase 2 cut order; the stability train continues; the 8-week beta is a minimum, not a maximum; the API stays in beta until the freeze |
| The date slips because the films and templates are people-limited | High | Medium | Recruit in the first 30 days; budget it; fewer, better; the freeze rule; a smaller flagship rather than none |
| Momentum and motivation drop without a public launch | Medium | Medium | Releases to testers continue; each milestone shows a film or example to testers; a public development log without launch language |
| Competitors or the market move first (Animate is in maintenance mode, Harmony and Moho continue) | Medium | Medium | The showcase is the differentiator; the closed alpha keeps the first adopters close; watch the market at each milestone |
| The closed alpha is not really closed (it is already on GitHub) | High | Low | Be honest: it is quiet, not secret; no promotion; the README and notes say so |
| Handmade templates are slow or costly, tempting the team to keep a synthetic one | Medium | High | The release check; the "fewer, better" rule; showcase films double as template sources |
| Rights or consent problems with contributed work | Medium | High | Forms reviewed by counsel before the first commission; a provenance row per asset; written consent for voices; no AI voice use |
| Reversing the "templates are never stored projects" rule creates long-term maintenance | Medium | Medium | Migration chain tested on every shipped template; size budgets; a test that opens every template in every release |
| The public beta reveals data-loss bugs late | Medium | Very high | Data safety stays a standing requirement from the start; the closed alpha tests recovery and migration continuously; the beta does not start before the freeze |

---

## 9. Decisions needed

Each has a recommendation, so the owner can answer yes or name a change.

| ID | Decision | Recommendation |
|---|---|---|
| D1-1 | How to treat the existing public alpha releases until 1.0 | Keep them and keep publishing alphas, with no launch activity and a clear "closed alpha" status line (section 3) |
| D1-2 | What happens to the four football scenes | Replace with handmade scenes, but ship two or three at 1.0, not four; hold the others as showcase films |
| D1-3 | Licence for makers' contributions | Maker keeps copyright; Fanimate gets a perpetual distribution licence; public under CC BY 4.0; counsel to confirm |
| D1-4 | Budget and payment | Decide before recruiting; pay professionals for commissions (illustration, voice, music, rigging); credit and licence for volunteers |
| D1-5 | How templates ship | Option A: `.fnm` files made in Fanimate, with size budgets and a migration test |
| D1-6 | Labelling the generated templates in alpha builds | Add "(placeholder)" to their names now; remove them from release builds |
| D1-7 | Merge Gate 2 into Gate 1 | Yes (section 1); keep the name Gate 3 for the studio gate |
| D1-8 | Size of the showcase | The minimums in section 4.2 as a starting point; revise after the first two films show how long they take |
| D1-9 | An invite-only wider beta at M12 | Yes if the owner agrees: it widens feedback without a public launch |
| D1-10 | Who is on the review panel | Three or more working animators outside the core team, paid or credited |

---

## 10. First 30 days

In priority order.

1. **Answer D1-1 to D1-10.** Several (D1-3, D1-4, D1-5) block the commissioning.
2. **Mark the generated templates "(placeholder)"** in alpha builds and add the closed-alpha wording to the README and release notes (small changes, with the owner's go-ahead).
3. **Write the licence, consent and provenance forms** and add them to the [legal review scope](../legal/review-scope.md); create `docs/legal/template-assets.md`.
4. **Recruit the animator review panel and the first makers** from the testers already using 0.3.x.
5. **Write the template briefs** for the first four handmade templates (suggested: Walk cycle, Lip sync, Frame by frame, Character rig, since they teach the core and need only Phase 1 features) and the film briefs for the first three films.
6. **Settle the supplied mouth set:** find its author and licence, or drop it.
7. **Set a size budget and a test** that opens every shipped template through the migration chain.
8. **Plan the film programme calendar** against the Phase 2 milestones, with each film's start tied to the milestone whose feature it needs.
9. **Start a list of showcase examples (recipes)** from the questions animators ask most.
10. **Update the Phase 1, Phase 2 and Phase 3 plans, `CLAUDE.md`, the vision's roadmap table and the README** to match this decision (done in the same change as this document).
