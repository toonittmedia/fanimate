# Fanimate Phase 3: Studio

Companion to the [product vision](../product-vision.md), the [Phase 1 plan](phase-1.md) and the [Phase 2 plan](phase-2.md). Phase 1 makes Fanimate a tool for one rigging animator. Phase 2 makes it a tool a small team can finish a production in. Phase 3 makes it a tool a studio can run a pipeline on: a project server, shared assets, review and approval, cloud and network rendering, a marketplace of extensions, and paid cloud AI. It ends at **Gate 3: studio-ready**.

> **Status: planning only. Nothing in this document is built.** Phase 3 depends on Gate 1 (version 1.0 with the frozen scripting API, the flagship film and the handmade templates), which now sits at the end of Phase 2 and has not happened. The old Gate 2 was merged into it on 2026-10-30 ([release-1.0.md](release-1.0.md)). Treat it as a design to challenge and to revise once Phase 2 shows what studios actually asked for. Numbers in tests and schedules are proposals.

> **What comes from the vision and what is new.** The scope (Fanimate Hub, review and approvals, cloud render, marketplace, cloud AI credits), the revenue table, the governance commitments and the Gate 3 criteria (studio pilots on Hub; plugin sandbox audited) are taken from the vision. The architecture, the protocols, the milestone order, the security design, the acceptance tests and the decisions are new proposals.

## Contents

1. [Where Phase 3 starts](#1-where-phase-3-starts)
2. [Phase 3 scope](#2-phase-3-scope)
3. [Rules that apply to every workstream](#3-rules-that-apply-to-every-workstream)
4. [Workstreams](#4-workstreams)
5. [Milestones](#5-milestones)
6. [Gate 3 acceptance tests](#6-gate-3-acceptance-tests)
7. [Phase 3 risks](#7-phase-3-risks)
8. [Decisions needed](#8-decisions-needed)
9. [First 30 days](#9-first-30-days)

---

## 1. Where Phase 3 starts

### 1.1 Entry conditions

- **Gate 1 is passed (version 1.0 is out)**, or the owner decides in writing to start earlier. The API must be frozen before a marketplace sells plugins against it, and the headless mode must exist before a render service can run it. Both are part of 1.0 now. Phase 3 is the first phase that starts after a public release, so the public beta's crash and feedback numbers are available to it.
- **The long-lead work starts earlier than the code.** A legal entity, payment accounts, tax advice, a security auditor, a hosting provider and the studio pilots each take weeks to months of calendar time. They are listed in the first 30 days (section 9) and can begin during Phase 2.
- **The desktop app stays free and offline.** Nothing in Phase 3 may make the editor depend on a network, an account or a server. This is a project rule, repeated in section 3.

### 1.2 What exists today that Phase 3 builds on

These are in the repository as of 0.3.2, so the plan does not re-invent them.

| Existing piece | Where | How Phase 3 uses it |
|---|---|---|
| The `.fnm` folder form: small JSON files per layer, media by content-derived name | [`fnm.md`](../spec/fnm.md), `fanimate-fnm` | The Hub stores and merges projects at this granularity |
| Atomic save, validator and repair, crash journal | `fanimate-fnm` | Sync never writes a half-pulled project; a pulled project is validated like any file |
| Per-layer shared history (`HistoryState`) | `fanimate-fnm` | A natural unit for deltas in live co-editing (Phase 4) |
| Headless render and export, the same renderer as the canvas | `fanimate-cli`, `fanimate-export` | The cloud render worker and the farm integrations call it |
| The render queue as a separate process with a job folder, `.part` files and rename on completion | `fanimate-export`, `fanimate --render-queue` | The local queue grows into the LAN pool and the cloud client |
| Ed25519 signing of updates, verified in the app, with a key that rotates only through a signed update | `scripts/sign-update.py`, `update::PUBLIC_KEY` | The same scheme signs packages, models and the revocation list |
| The browser player on WebGPU | `fanimate-web` | Review in the browser plays the live project |
| Opt-in crash reports sent only by the user | `crash.rs` | Studio support builds on this; no telemetry is added |
| Importers that report what they dropped | `fanimate-import` | The review and render pre-flight checks use the same style of report |
| The scripting API and plugin hosts | [Phase 2](phase-2.md), WS6 | Integrations (Kitsu, ShotGrid, Deadline), the command assistant and marketplace plugins run on them |
| Provenance records on layers and media | Phase 2, WS1 | Studio reports list AI-generated assets |

### 1.3 What Phase 2 and Phase 1 leave behind

| Item | Source | Proposed home |
|---|---|---|
| Project sync between the web build and the desktop | Phase 1 M5 note | WS1 (Hub sync), M17 |
| Marketplace sharing of text and title templates and node setups | Phase 2 WS2, WS5 | WS6 (marketplace) |
| Cloud AI tools (background and prop generation, script to animatic, command assistant) | Vision AI table; Phase 2 section 2.4 | WS8 |
| Bring-your-own-farm submission scripts | Vision | WS4 |
| Python scripts cannot be sandboxed by Fanimate | Phase 2 WS6 | Decision D3-3, WS7 |
| Linked assets with a rig updated in every scene that uses it | Vision | WS2 |
| The Moho Pro corpus measurement and import quality | Phase 1 | Studios on Moho and Animate are the Phase 3 pilots, so their files extend the corpus |

---

## 2. Phase 3 scope

### 2.1 In scope (from the vision)

| Area | Deliverable | Pain points |
|---|---|---|
| Collaboration | Fanimate Hub: open source, self-hostable project server with version history, locks and permissions; shared asset library; hosted version | P17 |
| Review | Reviewers watch renders or the live scene in the browser, draw notes on frames, approve or request changes; notes appear on the animator's timeline | P17 |
| Tracking | Shot status and owner; connection to Kitsu and ShotGrid through plugins | P17 |
| Cloud render | One-click cloud render paid by credits; bring your own farm; free LAN render pool; identical results to the desktop | P5 |
| Marketplace | Signed, sandboxed extensions; free and paid listings; ratings; updates with version pinning; self-hosted private marketplaces | P2 |
| Cloud AI | Background and prop generation, script to animatic, command assistant, through credits or the user's own keys | P8 |
| Business | The revenue streams in the vision, a legal entity, support contracts | P21 |
| Security | The plugin sandbox is audited | P2 |
| Industry | Studio pilots; the beginnings of the education programme | P20 |
| Governance | A steering group, a bug bounty, the release trains in writing | P22 |

### 2.2 Proposed additions

| Added | Why | Cost |
|---|---|---|
| Accounts, identity and billing as a shared service | Everything above needs them, and the vision does not name them | Medium; mostly integration, but with legal weight |
| A minimal built-in task tracker in the Hub | Small studios will not run a second tool; Kitsu and ShotGrid sync remain plugins | Low to medium |
| A pre-flight check before upload and before a cloud render | Missing fonts, media and plugins are the commonest cause of a failed render and a wasted credit | Low |
| Data protection and deletion tooling | Required by law in many places and by studios' contracts | Medium |

### 2.3 Out of scope for Phase 3

| Deferred feature | Moved to |
|---|---|
| Tablet apps | Phase 4 |
| Live co-editing (two people in one scene in real time) | Phase 4; Phase 3 delivers locks and merging |
| Harmony import and further importers | Phase 4 (OpenToonz, Synfig, Spine as community plugins) |
| Education certification and the school programme at scale | Phase 4; Phase 3 only pilots |
| SOC 2 or ISO 27001 certification | Not planned; studios that ask get the audit reports and a security whitepaper |
| A managed Hub for each customer on dedicated hardware | Support contract work, not product |
| 3D, multiplane depth, glTF | Phase 4 |
| In-app purchases on iOS and Android | Phase 4, with the tablet apps |

---

## 3. Rules that apply to every workstream

The Phase 1 and Phase 2 rules continue. These are the ones Phase 3 puts under most strain.

1. **Offline first.** The desktop app works fully with no account and no network. Sync, review, the marketplace, cloud render and cloud AI are optional and fail soft: when a service is down the editor says so and carries on. A project that links a Hub asset embeds a cached copy and opens without the Hub.
2. **No telemetry on by default.** Crash reports remain the user's to send. Service-side logs hold the minimum needed to run and bill, with stated retention. Anything measured for the success metrics comes from opt-in surveys, downloads and accounts, not from the editor phoning home (D3-9).
3. **Nothing leaves the machine without a preview.** Any upload (a project, a layer for an AI job, a render) shows what is sent, where, and what it costs before it starts.
4. **The file format stays the contract.** Linked assets, notes and review state either use the format's rules (spec, version, migration, validator, round trip, damaged-file coverage) or stay out of the project file. Review notes stay out of the file (section 4, WS3).
5. **Everything the vision calls open is published openly:** the Hub server, its sync protocol, the package format, the registry protocol and the render job format, each with a specification under `docs/spec/`. Licence of the server code is decision D3-1.
6. **Security is designed before it is built.** Each service has a written threat model before its first line of code, reviewed by someone who did not write it. Untrusted input includes project files, packages, notes and job descriptions.
7. **Money is boring.** Credits live in a double-entry ledger; every payment webhook is idempotent; every refund is traceable; nothing is charged for a failure that was the platform's.
8. **Self-hosting is a first-class path.** Anything the hosted service does for money, a self-hoster can do for free on their own machines, except the use of Fanimate's own compute and third-party AI providers.
9. **Accessibility and plain language** apply to the web apps as much as the editor: keyboard navigation, contrast, no em dashes in text, short sentences.
10. **Every service is in the pain-point matrix.** A service with no pain point behind it is cut.

---

## 4. Workstreams

Ten workstreams. WS1 is the platform; WS2 to WS8 are the vision's headline items; WS9 is accounts, privacy and operations; WS10 is the pilots and governance.

### WS1. Platform, accounts and the sync foundation

**Goal.** The shared base the hosted services and the self-hosted Hub stand on.

**Design.**

- **One server, modules inside.** A single Rust service, `fanimate-hub`, with modules for projects, storage, review, tasks, the render coordinator and the registry, plus a separate worker binary for rendering. One binary is simplest to self-host. Hosted-only concerns (billing, the AI gateway, the credits ledger) are separate services that talk to it through documented interfaces. Candidate crates are the usual ones (an async web framework such as `axum`, `sqlx`, `tokio`); check licences with `scripts/licences.py`.
- **Storage.** SQLite for a single-machine self-host, PostgreSQL for the hosted service, behind one repository layer with the same migrations tested on both. Blobs go to the local disk or to an S3-compatible store. Content-addressed, so identical media is stored once.
- **Identity.** Accounts are optional and only for services. Sign in from the desktop app with an OAuth device flow; tokens are kept in the operating system's keychain; scripts and farms use personal access tokens with scopes. Studios get OIDC single sign-on (SAML if a pilot needs it). Passkeys and multi-factor sign-in for the hosted service.
- **The sync crate, `fanimate-sync`.** New, above `fanimate-fnm`, outside the core and the editor. It pulls and pushes the folder form of a project: a manifest of (path, content hash) pairs and the blobs behind it. Uploads are chunked with content-defined chunking and resumable. A local sidecar remembers the last synced manifest so a three-way comparison is possible. It never touches the live document: it works on a copy and hands the editor a validated project to open, so crash safety and the "no panics on user data" rule hold.
- **Specs.** `docs/spec/hub.md` (sync and API), `docs/spec/package.md` (extension packages), `docs/spec/registry.md` (marketplace index), `docs/spec/render-job.md` (job description). Versioned, with a compatibility policy.
- **Web app.** A browser dashboard for projects, shots, review and administration, built on the existing `fanimate-web` player for anything that shows a scene. The framework for the surrounding pages is a small decision (D3-8); the constraint is that it works without a heavy toolchain for self-hosters and is keyboard accessible.

**Tasks.**

| Task | Detail |
|---|---|
| Threat models | One per service: Hub, render, registry, AI gateway, billing |
| Hosting | Choose a provider-neutral setup: containers, infrastructure as code, two regions at most to start |
| Skeleton | Service with health checks, migrations, configuration, structured logs with redaction, the OpenAPI description |
| Auth | Device flow, tokens, scopes, SSO |
| `fanimate-sync` | Manifest, chunking, resumable transfer, sidecar, offline queue |
| Specs | The four documents above, with examples and a conformance test each |

**Done when.** A desktop app on one machine pushes a project to a Hub run from a clean container, another machine pulls it, and both validate; killing the network half-way leaves both sides consistent.

### WS2. Fanimate Hub: projects, versions, locks, linked assets

**Goal.** Many artists on one production database (P17), with no lost work and no silent overwrites.

**Design.**

- **Model.** Organisation, then project (a series or a film), then optional episode and sequence, then shot. A shot has a working scene, versions, and links to assets. Roles per project: owner, supervisor, artist, reviewer (read and comment), client (review links only, no download). An audit log records who did what.
- **Versions.** A version is a manifest of (path, hash) with a parent, an author and a message. Linear history with named published versions; a working draft per user. Not Git, but the folder form is readable by Git and a project can be exported to a Git repository for studios that want it.
- **Locks and merging.** Because a scene is a set of small files, two artists can work on different layers of one scene. A lock is a lease on a path (a layer, or the whole scene) with a timeout; an administrator can break one. When both sides changed different files the merge is automatic. When both changed the same file, the user chooses mine or theirs **per layer** in a merge panel; nothing is silently overwritten. Document-wide data (camera, guides, palettes, styles) has a stated owner role.
- **Partial sync.** Only the shots a user opens are pulled; thumbnails and low-resolution proxies are served for browsing. A 100 GB production must not be a 100 GB download.
- **Linked assets.** A rig, character, prop, palette or symbol library is an **asset** with versions in the Hub. A scene links `asset@version` (pinned) or `asset@latest approved`. The scene embeds a cached copy so it opens offline and with the Hub down. Updating shows a summary of what changed. A scene may override animation, transforms and a palette variant, but not the rig's structure; "Make local copy" breaks the link. This needs a format bump (a `links` table and a read-only-structure flag, with a validator rule), which follows the usual rules. Pinning protects a finished scene from a later change to a rig.
- **Self-hosting.** A static binary and a container image; a compose file; documented backup and restore; upgrades with tested migrations; a minimal admin page. Target: a studio's technical director has a Hub running on a clean Linux machine in under 30 minutes from the docs.
- **Integrity.** Writes are transactional. A deterministic simulation test drives the protocol under crashes, retries, duplicate requests and partitions and checks no committed version is lost and no two clients see different content for the same hash.

**Interface.** A Project panel in the editor (browse shots, check out, check in, see locks and who holds them); a lock badge on layers that someone else holds; check-in with a message; the merge panel; a linked-asset indicator and an Update command, all with right-click menus, both keymaps, and the command palette.

**Tests.** Simulation of the sync protocol; fuzz of the manifest and chunk formats; a project with 10,000 shots lists and syncs a shot in under a stated time; the validator runs on every pulled project; backup then restore on an empty server gives a byte-identical store; upgrade from each earlier Hub version.

**Done when.** Three artists work on one shot's layers, one pushes a rig update, another's scene shows the update summary and takes it, a conflict on one layer is resolved in the merge panel, and the server is restored from backup with nothing lost.

### WS3. Review and approval

**Goal.** Reviewers can watch, mark up and approve in the browser, and the animator sees the notes on their timeline.

**Design.**

- **Two ways to review.** *Live scene*: the browser plays the project itself with the WebGPU player (sharp, small, frame-accurate, scrubbable). *Render*: a rendered video or image sequence of a version, for clients and for scenes the web player cannot run. The reviewer picks one; the animator's version history says which exist.
- **Notes.** A note has a version, a frame, an author, a thread, a status (open, resolved) and an optional drawing (vector strokes over the frame, stored as note data). Comparison of two versions side by side, as a wipe, or as an overlay.
- **Notes stay out of the project file.** They are stored in the Hub and cached in a local sidecar. This keeps private feedback out of deliverables, avoids a format bump, and lets reviewers work without editing rights. The editor shows them as markers on the timeline and a Notes panel; resolving one from the editor updates the Hub.
- **Approvals.** A shot has a status flow that a studio can rename (layout, animation, clean-up, colour, comp, approved by default). A reviewer approves or requests changes; the status changes and the owner is notified.
- **Review links.** For clients: an expiring link, an optional password, an optional watermark, view-only with downloads off. No account is needed to view.
- **Notifications.** Email and webhooks (generic JSON), so Slack, Matrix and others work without special code.

**Tests.** Frame-accurate stepping: the frame shown matches the requested frame in the live scene and in the render; a note placed on frame 87 appears on frame 87 in the editor; link expiry and password; a reviewer without edit rights cannot change anything; the sidecar survives the Hub being down.

**Done when.** A client opens a link on a phone, draws on frame 87, requests changes; the animator sees the marker on their timeline, fixes it, resolves the note, and the client approves.

### WS4. Cloud render, farms and the LAN pool

**Goal.** Renders stop tying up the artist's machine (P5), with identical results to the desktop.

**Design.**

- **Job.** A job is (project version or uploaded snapshot, frame range, size, format, settings). The description is the open render-job format, so a farm, the cloud and the local queue share it.
- **Pre-flight.** Before upload or billing: open the project headless, list missing fonts, media and unsupported features, estimate cost from a short sample render, and refuse with a clear message rather than fail after charging. The check reports in the same style as importers.
- **Workers.** A container running `fanimate-cli` headless. Linux with a GPU where available; a software Vulkan fallback (Mesa's lavapipe) for CPU-only tiers. Parity with the desktop is measured against the Gate 1 test (within 1/255 per channel) on both. The output carries the renderer build id, so a result can be traced to a build.
- **Untrusted projects.** A project is untrusted input from any user. Each job runs in an isolated sandbox (a microVM or a user-space kernel such as gVisor), with no network, limits on CPU, memory, disk and time, read-only inputs, a fresh disk, and plugin code disabled (graph expressions are ordinary deterministic nodes and are allowed). Output is scanned for size abuse. The damaged-project fuzzing from Phase 1 and 2 is part of why this is acceptable.
- **Pricing and credits.** Priced per render-minute of worker time by tier, with the estimate shown first and a hard cap per job. Cancelling stops billing. A platform failure is retried on another worker and is not charged. Outputs are kept 14 days by default (configurable), encrypted at rest, delivered by expiring link, and deleted afterwards.
- **Bring your own farm.** `fanimate-cli render` gains frame ranges, tiling, machine-readable progress (JSON lines) and stable exit codes. An `integrations/` repository holds submitters for Deadline, Flamenco and OpenCue written against the Phase 2 API, each driving the CLI as a separate program (no linking, so licences do not interact).
- **LAN pool.** Spare office computers join a free pool. A coordinator runs on any machine; workers find it by local-network discovery (opt in) or by address; jobs are signed with a shared secret; frames are leased in chunks, so a worker that disappears loses only its lease. The queue process that already exists is the base, extended from a folder to a protocol.

**Tests.** Parity against desktop renders on the benchmark and template projects; a job killed at a random moment is retried and charged once; a hostile project (huge canvas, deep nesting, a decompression bomb in media) hits the limits and fails cleanly; a worker with no network still renders; the estimate is within a stated percentage of the real cost on the template set.

**Done when.** An artist sends a 22-second template scene to the cloud, keeps working, gets a notification and a link, and the frames match a local export; a studio renders the same scene on its own Flamenco farm and on a LAN pool with matching output.

### WS5. Cloud AI and credits

**Goal.** Heavy generation (P8) for people who want it, off by default, with every output editable and labelled, and a free path with the user's own keys.

**Design.**

- **Tools.** Background and prop generation, script to animatic, command assistant. (The local tools are Phase 2.)
  - *Background and prop generation*: a prompt, and optionally a reference, produces a raster that is split into layers and, where the user asks, vectorised; the result is ordinary layers. Honest limit: generation gives raster art; "layered, editable" means a layered raster plus tracing, not native vector authorship.
  - *Script to animatic*: a script (pasted or imported) becomes scenes, a shot list, timing, rough boards and placeholder text and audio layers, as a new project.
  - *Command assistant*: a natural-language request ("make the walk heavier") is turned into calls on the scripting API, shown as a preview of what would change, and applied as one undoable transaction. It can only do what the API can do, and only after the user accepts.
- **Gateway.** A service that routes jobs to model providers behind one interface, enforces credits and rate limits, applies content rules, and keeps no prompts or outputs beyond what is needed to deliver and bill (stated retention, default none). Providers are contracted for no training on Fanimate users' data.
- **Consent and preview.** Each job shows exactly what is sent (prompt, selected layers, frames) and the credit cost before it starts. There is no background upload.
- **Bring your own keys.** The user supplies a provider key (kept in the keychain, used from the user's machine directly, never sent to Fanimate) or an address for their own or a local server speaking a common API. This is the free route and needs no account.
- **Credits.** A ledger service: purchase, spend, refund, expiry rules stated in the terms, organisation pools, spend limits per user. Prices come from a published list that follows provider costs.
- **Labels.** Everything generated carries `provenance`; the studio can export a report of generated assets; an organisation can forbid cloud AI entirely.
- **Policy.** Terms say the user owns their output as far as the law and the providers' terms allow, and say where they do not. Abuse handling and refusals are documented.

**Tests.** A job with the network cut mid-way is retried or refunded; no content is stored after delivery in the default configuration; a provider error does not charge; a command-assistant preview matches what is applied; with cloud AI disabled by policy no request is possible.

**Done when.** A user with their own key generates a background and gets layers; another with credits turns a script into an animatic; each sees the preview first, and each result is labelled.

### WS6. Marketplace and the extension registry

**Goal.** Replace forum-shared loose files with discovery, ratings, updates and safe installation (P2), with free and paid listings.

**Design.**

- **Package.** A Fanimate package (`.fnp`) is a zip with a manifest (id, version, publisher, kind, API version range, permissions, licence, a hash list of every file, screenshots), the payload, and signatures. Kinds, from the vision: scripts and tools, brushes, rigs and character templates, text and title templates, effect presets, node setups, keymaps, tutorials; also workspace layouts, themes, palettes, styles and symbol libraries. Data kinds carry no code and need no permissions.
- **Signing.** Publishers register an Ed25519 key. The store countersigns after review. The app verifies both and refuses a package that fails either, using the same scheme as updates. A signed revocation list is refreshed when online and honoured offline up to a maximum age; a revoked package is disabled on the next start. Keys can be rotated; the store key rotates only through a signed app update, as the update key does.
- **Review pipeline.** Automated: manifest validation; permission analysis (the declared permissions against what the code can reach: for WASM, its imports); static scanning; malware scanning; licence checks of bundled assets; size limits; a test run in the sandbox. Human review for new publishers and for dangerous permissions (`net`, `process.spawn`, writes outside the project). Updates are sampled. Rejections come with reasons and an appeal route. A seeded corpus of malicious test packages checks the pipeline catches what it should.
- **What may be installed by the app (D3-3).** Recommendation: the app installs **only** packages it can sandbox: data packages, WASM plugins and TypeScript plugins. Python tools are listed in a directory with a clear "runs outside the sandbox; trust the author" marker and are installed by the user with their own tools, not by the app. Revisit if an operating-system sandbox for Python proves practical on all three platforms.
- **Commerce.** Free and paid listings; the creator sets the price; the proposed fee is 15%. Payments through a merchant of record, which handles sales tax and chargebacks, or a marketplace payments provider (D3-6). Creator payouts must work for people in Nigeria and other emerging markets, so payout provider coverage is a requirement, and provider availability changes, so check it at decision time. Refund rules, a payout hold period, tax forms for creators, and fraud checks are written down.
- **Ratings and reviews.** From verified installs or purchases; reporting and moderation; the code of conduct applies.
- **Updates and pinning.** One-click install and automatic updates; a production pins exact versions in a lock file in the project (`plugins.lock`: id, version, hash) and the app warns when a project's plugins differ; rollback keeps the previous version.
- **Self-hosting.** The registry protocol is simple enough to serve from static files (an index and the packages), with optional authentication. A studio runs a private marketplace on its own Hub or a storage bucket and sets an allowlist of what its artists can install; a mirror of public listings is optional.
- **Trust and safety.** Terms for publishers; a takedown process; no proprietary content from other products; reports; a status for each listing (public, hidden, revoked).
- **In the app.** A Marketplace panel (browse, search, install, update, see permissions and reviews). Offline it shows what is installed.

**Tests.** Signature and countersignature verification, tampering, expiry, revocation offline and online; the pipeline against the malicious corpus; install, update, pin and roll back; a package that asks for more permissions in an update is blocked until the user agrees; a compromised publisher key is revoked and the packages disabled.

**Done when.** A publisher lists a free WASM brush and a paid title template; a user finds, buys and installs them; the studio's private registry serves an internal tool; a deliberately malicious package is caught by the pipeline; a revoked package is disabled on the next start.

### WS7. Plugin sandbox and the security audit

**Goal.** Gate 3 requires the plugin sandbox to be audited. This workstream produces a sandbox worth auditing, then the audit.

**Threat model** (written first, in `docs/security/`): the assets are the user's files, credentials and machine; the adversaries are a malicious publisher, a compromised update, a malicious project file or Hub, and a malicious dependency inside a plugin.

**Design.**

- **WASM**: no ambient authority; capabilities as host functions that check the manifest's permissions; fuel and memory limits; no operating-system interface unless a permission opens a specific scope; every file path is canonicalised and checked against the scope, with symbolic links and time-of-check issues considered; network access only to listed hosts, with protection against DNS rebinding.
- **TypeScript**: an embedded engine with no Node-style APIs, only the same host functions, time and memory limits, no dynamic code loading from the network.
- **UI contributions** are declarative data, so a plugin never runs code in the UI thread or draws arbitrary pixels.
- **RPC** is local only, token protected, off by default.
- **Supply chain**: signed packages; revocation; reproducible builds for first-party plugins; dependencies of a plugin are inside its package and listed in the manifest.
- **Resource exhaustion** (CPU, memory, disk, handles) is bounded and the app stays responsive; a stuck plugin is killed.
- **Accepted risks** are written down (for example timing side channels).
- **Fuzzing** of every host function and of the package and manifest parsers.

**Audit.** Choose an independent firm; write the scope (the sandbox, the package and signature handling, the Hub's authentication and tenant isolation, the render sandbox); allow a fix window; publish a summary. Run a penetration test of the hosted services. Open a bug bounty with a safe-harbour statement (the existing `SECURITY.md` is the base). **Gate 3 needs no unresolved critical or high findings in scope.**

**Done when.** The audit report is in, findings are fixed and re-tested, and a summary is public.

### WS8. Accounts, billing, privacy and operations

**Goal.** Run the services responsibly.

- **Accounts and organisations.** Optional; deletion and export tools; roles; invoices; spending limits.
- **Billing.** A payment provider and a merchant of record; a credits ledger with double-entry records and daily reconciliation; idempotent webhook handling; refund workflow; receipts; a clear price list; no dark patterns.
- **Privacy and law.** A privacy policy and terms in plain language; a subprocessor list; data export and deletion within stated days; data residency options (at least a choice between regions); retention schedules per data type. Applicable laws include the GDPR, the Nigeria Data Protection Act, and US state laws; students may be under 18, so the education route uses organisation accounts and the rules on minors (D3-10). Legal review of all of it.
- **Operations.** Environments (development, staging, production) from infrastructure as code; secrets management; monitoring and alerts; a public status page; incident response with public post-mortems; backups with restore drills each quarter; service objectives that are honest for a small team (propose 99.5% monthly availability for the first year, not more); a cost model in which render compute is the main cost and prices cover it; abuse handling (render jobs are a target for misuse, hence the sandbox and limits).
- **Support.** Business-hours support for pilot studios, a public issue tracker for everyone, and paid support contracts (a revenue stream in the vision).

### WS9. Studio pilots, education seeds and the ecosystem

- **Studio pilots.** At least three studios of 5 to 30 people run a real shot pipeline on the Hub for at least 8 weeks, free, under a written pilot agreement (feedback, permission for a case study, no exclusivity). Choose studios that use Moho or Animate today, since those imports work first. Weekly check-ins; every blocker logged and triaged; an exit interview.
- **Integrations.** Kitsu and ShotGrid plugins on the Phase 2 API: pull shots and statuses, push status changes and renders. Webhooks for anything else.
- **Education seeds.** A classroom pack (Simple mode on by default, a teacher guide, five lesson plans, sample projects, an offline installer), piloted in a few schools and training centres, including some in Nigeria and other emerging markets, where bandwidth and hardware set the design: the 2015 floor machine is the target and nothing requires a constant connection. Certification waits for Phase 4.
- **Showcase.** Case studies, plus the films and examples published at the 1.0 launch, as the proof that studios and schools can point to (P20).

### WS10. Governance and business structure

- **Legal entity.** A foundation or company to hold the trademark, receive payments, sign contracts and employ people; chosen with advice (D3-2).
- **Steering group.** A charter, with seats for working animators, studio technical directors and core developers, terms, how decisions are recorded and how conflicts of interest are handled. This answers the old forum complaint that Moho's developers had little animation experience.
- **Contributions.** A developer certificate of origin is enough for a project under permissive licences; a contributor licence agreement is needed only if the entity wants to relicense (D3-1).
- **Trademark policy.** What others may call their services and products ("Fanimate Hub" hosting, plugins "for Fanimate").
- **Releases.** The two trains from the vision (a feature release about every four months; a stability release about every month) written into `GOVERNANCE.md`, with the rule that polish never waits for features.
- **Public roadmap and voting.** The issue tracker's voting is used to rank requests; the steering group publishes why it accepts or declines.
- **Bug bounty.** Opens with the audit (WS7), with a stated range of rewards and a safe harbour.
- **Transparency.** An annual report of revenue by stream and spending by category, so "free forever" is credible.

---

## 5. Milestones

Numbering continues from Phase 2, which ends at M15.

| Milestone | Theme | Main deliverables | Exit check |
|---|---|---|---|
| **M16: Platform** | Foundations | WS1 complete; threat models; hosting; specs drafted; entity and payment groundwork under way | A project pushed from one machine is pulled on another; a service skeleton passes the threat-model review |
| **M17: Hub core** | Versions and locks | WS2 except linked assets; desktop Project panel; self-host package | Three artists work on one shot; a restore from backup loses nothing |
| **M18: Shared work** | Assets, review, tasks | Linked assets; WS3 review; the built-in tracker; Kitsu and ShotGrid plugins | Section 4 WS3 "done when"; a rig update reaches a pinned and an unpinned scene correctly |
| **M19: Render** | P5 | WS4: cloud, farms, LAN pool | Section 4 WS4 "done when" |
| **M20: Marketplace** | P2 | WS6 with free listings and the private registry; sandbox hardening from WS7 | Section 4 WS6 "done when", free path |
| **M21: Commerce and cloud AI** | Revenue | Paid listings; credits ledger; WS5; billing and privacy tooling from WS8 | Section 4 WS5 "done when"; a purchase, a refund and a payout reconcile |
| **M22: Prove it** | Quality gate | The audit and penetration test; studio pilots complete; load and disaster-recovery drills; documentation; governance in place | Every Gate 3 criterion in section 6 passes |

**Dependencies.** M16 before everything. M18 needs M17. M19 needs the Phase 2 headless mode and the open render-job format from M16. M20 needs the frozen API and the permission model from Phase 2, and the audit firm booked at the start of M20 because audit calendars are long. M21 needs M19 (credits are spent on renders) and M20 (paid listings). Legal, tax and payment-provider work runs from the first 30 days and gates M21. The pilots start as soon as M17 and M18 are usable (they need locks and review), so the 8 weeks end before M22.

**Cut order if time runs short:** managed regions beyond one; the LAN pool's discovery (keep addresses); paid listings (free listings and the private registry already deliver the main value); the command assistant; script to animatic; prop generation; SAML; the built-in tracker (keep Kitsu); then, only with the owner's approval, the farm integrations other than one. Not cuttable: the sandbox audit, the offline-first rule, data integrity tests, the privacy and deletion tooling, the threat models.

### 5.1 Schedule

The vision plans about six months. Almost nothing here is limited by coding speed. The constraints are:

| Constraint | Why it takes calendar time | Gates |
|---|---|---|
| Legal entity, bank, payment provider, tax | Setup, identity checks and advice take weeks to months | M21 |
| Security audit | Firms book months ahead and need a stable target; the fix window follows | M22 |
| Studio pilots | 8 weeks of real production, with recruitment before | M22 |
| Review of every change | Servers and billing raise the cost of a mistake; every change is read by a person | All |
| Hosting and operations learning | A first-time team learns what breaks | M16 to M22 |
| Legal review of terms and privacy | External counsel | M21 |
| Provider contracts for AI | No-training terms and pricing need negotiation | M21 |

Plan: book the audit and recruit the pilot studios in month 1; M16 and M17 in the first two months; pilots start in month 3; M18 and M19 in months 3 to 4; M20 in months 4 to 5; M21 and the audit in month 5; M22 and the fix window in month 6 and into month 7. Re-plan after M17.

---

## 6. Gate 3 acceptance tests

Gate 3 is a release decision, not a date. It also needs Gate 1 (version 1.0) to have been passed.

### 6.1 Criteria from the vision

| Criterion | How it is measured | Target |
|---|---|---|
| Studio pilots on Hub | Studios of 5 to 30 people run a real shot pipeline on the Hub (proposed: at least 3 studios, at least 8 weeks each, at least one episode, short or full, delivered through it) | **Done**, with pilot reports |
| Plugin sandbox audited | An independent audit of the sandbox, signing and registry | **Done**, no unresolved critical or high findings, summary published |

### 6.2 Supporting tests added for Phase 3

| Test | Pass condition |
|---|---|
| Offline first | With every service unreachable the editor starts, opens, edits and saves; linked assets open from their cached copy |
| No default telemetry | A network capture of a fresh install shows no traffic other than the update check the user can turn off |
| Sync integrity | The simulation test finds no lost or divergent version over 1,000,000 randomised steps |
| Merge safety | No merge overwrites a change without the user choosing; verified by property tests |
| Backup and restore | A restore on an empty server is byte-identical; a drill is done each quarter |
| Self-host install | A technical director installs a Hub on clean Linux in under 30 minutes from the docs |
| Upgrade | Every earlier Hub version upgrades without loss |
| Review accuracy | The frame a reviewer comments on is the frame the animator sees, in both review modes |
| Render parity | Cloud and desktop output agree within 1/255 per channel on the template set, and the LAN pool and the farm integrations agree with them |
| Render safety | Hostile projects fail cleanly inside limits; no job reaches the network |
| Render billing | A killed job is retried and billed once; a platform failure costs nothing; estimates are within a stated percentage |
| Ledger | Double-entry reconciliation balances daily; webhooks are idempotent; refunds trace to payments |
| Package trust | Tampered, unsigned, expired and revoked packages are refused; the pipeline catches every package in the seeded malicious corpus |
| Permission enforcement | A plugin cannot read, write or reach the network outside its granted scope, tested with an adversarial plugin set |
| Cloud AI consent | No request is made without a preview and consent; with cloud AI disabled by policy none is possible; outputs are labelled |
| Privacy | Export and deletion complete within the stated days; subprocessors are listed; retention matches the policy |
| Accessibility | The web apps pass keyboard-only use and contrast checks |
| Load | The hosted service meets its stated objectives under a load test at twice the pilot studios' peak |
| Disaster recovery | A region loss drill restores service within the stated time |
| Penetration test | No unresolved critical or high findings |
| Licence audit | Every dependency of every service and worker image has an allowed licence recorded |
| Documentation | Admin guide, user guide, API reference, security whitepaper and the specifications are complete |

### 6.3 Results so far

None. Nothing in Phase 3 is built.

---

## 7. Phase 3 risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A small team cannot build and run five services plus a marketplace | High | High | One server binary; cut order in section 5; hosted-only parts kept few; operations are budgeted work |
| A security failure (tenant leak, plugin escape, malicious package) destroys trust | Medium | Very high | Threat models first; the audit; the bounty; a kill switch (revocation); least privilege everywhere; a public incident process |
| Studios will not adopt a new server without a track record | High | High | Free pilots; self-hosting; Kitsu and ShotGrid integration; export to Git; every file stays open |
| Render costs exceed credit revenue | Medium | High | Estimates and caps; per-minute pricing from measured cost; CPU and GPU tiers; abuse limits; start small and reprice |
| Cloud and desktop results differ | Medium | High | The same renderer, the parity test as a gate, software and GPU workers both measured, the build id on every output |
| Payment and tax law across countries is harder than expected | High | Medium | A merchant of record; advice early; launch paid listings in a few regions; free listings first |
| Creators in emerging markets cannot be paid | Medium | High | Treat payout coverage as a requirement when choosing providers; offer more than one; accept a slower payout rather than exclude them |
| Provider terms or prices for AI change | High | Medium | A gateway behind one interface; own keys and local servers as the free path; prices published as a list that follows costs |
| Generated content raises copyright or safety complaints | Medium | High | Labels; terms; filters; refusals; a takedown route; opt-in only |
| Python scripts cannot be sandboxed, so users expect safety they do not have | Medium | High | D3-3: the app does not install them; the marketplace says so on the listing; documentation is blunt |
| Data-protection duties (deletion, residency, minors) are underestimated | Medium | High | Design for deletion from the start; counsel review; organisation accounts for schools |
| The linked-asset format change causes migration problems | Medium | High | Follow the format rules; the embedded cache means a scene opens without the link; migration tested from version 1 |
| The review player and the desktop render differ in the browser | Medium | Medium | Offer both live and render review; the frame-accuracy test; state web limits plainly |
| Scope drift into a general production-management product | Medium | Medium | The pain-point rule; Kitsu and ShotGrid do tracking; the built-in tracker stays minimal |
| The pilots reveal editor problems that the Hub work then starves | Medium | High | The stability release train continues; pilot blockers outrank new Phase 3 features |

---

## 8. Decisions needed

Each has a recommendation, so the owner can answer yes or name a change.

| ID | Decision | Recommendation |
|---|---|---|
| D3-1 | Licence of the Hub server and the services | Apache-2.0 (or dual MIT / Apache-2.0) like everything else, with a trademark policy protecting the name; the vision says "open everything" and this keeps dependencies and contributions simple. Use AGPL only if the owner wants to stop unmodified resale by third-party hosts |
| D3-2 | Legal entity and where | A foundation or company chosen with advice, in the jurisdiction that best allows payments, grants and payouts for the team; decide in the first 30 days |
| D3-3 | What the app installs from the marketplace | Only sandboxable packages (data, WASM, TypeScript); Python tools are listed but installed by the user |
| D3-4 | Hosting | A provider-neutral container setup with infrastructure as code; pick one provider for the hosted service; confirm regions that serve Africa, Europe and North America with acceptable latency |
| D3-5 | Credits | Prepaid, expire after 24 months (stated up front), refundable when unused within 14 days of purchase where the law requires |
| D3-6 | Payments | A merchant of record for sales tax and chargebacks, and a second payout route for creators where the first is not available |
| D3-7 | Marketplace fee | 15% as the vision proposes, with a lower rate for the first year to attract creators |
| D3-8 | Web app stack | A small, dependency-light stack with server-rendered pages and the existing web player; chosen to be easy to self-host and audit |
| D3-9 | Measuring success metrics without telemetry | Downloads and release counts, account and Hub counts for the services, opt-in surveys, and an opt-in anonymous ping the user can read before turning on |
| D3-10 | Minors and education | Organisation accounts only for schools; no personal accounts for children; counsel review before any pilot with under-18s |
| D3-11 | Contributor terms | Developer certificate of origin; no contributor licence agreement unless relicensing is planned |
| D3-12 | Which regions for the hosted Hub at launch | One region first, a second when pilots show demand |

---

## 9. First 30 days

In priority order, starting when Phase 2 is far enough along that the owner decides to begin.

1. **Choose and form the legal entity** (D3-2), open a bank account, and begin the payment-provider and tax conversations (D3-6).
2. **Book an independent security auditor** for the window after M20, and agree the scope outline.
3. **Recruit the pilot studios** (at least three, plus a few schools), using the animators and studios already testing; write the pilot agreement.
4. **Write the threat models** for the Hub, the render service, the registry and the AI gateway, and have each reviewed.
5. **Draft the four specifications** (`hub.md`, `package.md`, `registry.md`, `render-job.md`) with the sync manifest and the render job as the first worked examples.
6. **Pick the hosting provider** (D3-4) and set up infrastructure as code for a staging environment.
7. **Spikes, one or two days each:** push and pull a project folder with chunking and a killed connection; render a template project inside a microVM or gVisor sandbox and compare it to the desktop; verify a signed package offline; call a model provider through a gateway with no logging; check payout coverage for Nigeria and two other emerging markets with each candidate provider.
8. **Draft the privacy policy, terms and subprocessor list** and send them to counsel.
9. **Write the charter for the steering group** and invite the first members, including working animators.
10. **Update `CLAUDE.md`, `GOVERNANCE.md` and `SECURITY.md`** with the new rules once D3-1 and D3-2 are answered.
