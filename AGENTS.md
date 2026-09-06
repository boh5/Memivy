# AGENTS.md

## Repository status

Memivy is a local-first, open-source, AI-native personal memory tool built around “记一下、问一问、接着想”: capture ideas, ask about existing memories, continue thinking, and explicitly save useful conclusions. The original Phase 1 technical-risk prototype was accepted on 2026-09-06. The user subsequently authorized Phase 1 v2 with a desktop companion and a minimal real conversation loop; this new version still awaits native interaction testing and user acceptance. Product requirements are not evidence of implemented features. Do not implement later milestones or create application code as a side effect of a documentation or research task.

## Reference documents

- [PRD.md](PRD.md): product requirements, current platform scope, interaction rules, and acceptance criteria.
- [TECH_STACK.md](TECH_STACK.md): proposed architecture and technology choices; read before implementation or dependency changes.
- [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md): demo-first implementation sequence, approval record, and milestone checks; use the relevant stage for development work.
- [DESIGN.md](DESIGN.md): the selected Miro visual reference; read before UI work. Keep the official imported file unchanged unless the user explicitly requests an update. Do not insert translations, project instructions, or a replacement design system into it.

Use the relevant documents rather than duplicating them here. If a requested change conflicts with a documented decision, clarify the decision instead of silently changing product scope. Visual examples do not introduce new product features.

Research the interaction before revising the clickable Demo, then use the explicitly approved Demo version as the UI implementation and acceptance baseline. Do not start later production milestones until the applicable Demo is approved and development is authorized. The existing Phase 1 scaffold does not need reinitialization. The Demo entry is `design-demo/index.html`; check its approval status and version in `DEVELOPMENT_PLAN.md` rather than assuming an existing Demo is approved.

On 2026-09-05 the user explicitly authorized Phase 1 only, using Demo v0.1 as a visual reference while its functional interactions remain unapproved. This exception allows the technical prototype described in `DEVELOPMENT_PLAN.md`, not Phase 2 or production feature development. Keep prototype data separate from future production data and label it clearly.

On 2026-09-06 the user accepted Phase 1 with its recorded evidence limits; do not reopen that acceptance or add further prototype checks merely because the product direction changed. The user then confirmed the personal-memory direction and authorized updating the related documents only: keep Demo v0.1 and application code unchanged in this documentation task. Detailed interaction research and Demo changes are deferred to later work. The new direction does not approve a new layout, window transition, or confirmation flow; consult PRD section 12 for open interaction questions.

## Implementation boundaries

2026-09-06 later authorization: after UI/UX research, the user explicitly asked to adapt the existing Phase 1 code into a usable interaction prototype and approved the proposed plan ("好，那你就做吧，做出来我看看。"). Phase 1 v2 may implement the desktop floating companion, shared capture/question input, topic continuation, grounded model answers, reviewed new-record saving and undo in isolated prototype data. This is a native interaction experiment, not approval of production Phase 2 or a requirement to redo the accepted Phase 1 baseline. Keep Demo v0.1, brand assets and the official DESIGN.md unchanged. Record actual v2 checks in DEVELOPMENT_PLAN.md.

- Keep the MVP focused on capture, AI memory maintenance, grounded memory Q&A, continued discussion, and explicitly confirmed conclusion saving. Retain a browsable, editable memory library; Memivy must be useful without an external Agent. Do not add to-dos, reminders, voice capture, embedding, synchronization, a hosted backend, or additional platform support without an explicit scope change.
- Save raw captures locally before starting AI processing. AI must never rewrite or delete the original input; edits to current memory content retain versions and provenance.
- AI memory mutations must produce a visible receipt and be reversible. Model failures must not prevent capture or keyword search. Do not substitute an answer or summary for the required mutation receipt.
- SQLite is the source of truth; Markdown is an export format. Keyword search and MCP `memory_search` use FTS5 without model calls. In-app Q&A and discussion may use the configured model to derive search terms and answer from bounded retrieved evidence; this does not authorize embedding or vector indexes.
- Keep local conversations separate from durable memories. Questions, hypotheses, AI suggestions, and drafts do not automatically become captures or enter memory search/MCP results. Save a discussion conclusion only after the user can review its text and destination and explicitly confirms it; preserve the confirmed text and provenance, and reuse the core version, receipt, deduplication, correction, and undo rules.
- Ground recollections in actual captures or memory versions; distinguish new suggestions from remembered facts and acknowledge insufficient evidence. Bind citations to the versions actually used, handle deleted sources honestly, and support cancellation, failure, and retry without accidental memory writes.
- Follow the planned shared Rust core boundary: UI and MCP must not bypass it to mutate the database or duplicate memory rules.
- Store BYOM credentials in a separate local app-data configuration file, not Keychain or a hosted key service. Keep keys out of the content database, exports, backup metadata, logs, and repository files.
- MCP exposes only `memory_capture` and `memory_search`; capture requires the user's explicit intent to save, and search returns bounded durable-memory results with provenance. Do not expose unsaved conversations or add Q&A, bulk-read, or mutation tools.

## Change discipline

- Make small, task-scoped changes. Preserve unrelated work and avoid speculative abstractions or dependencies for deferred features.
- Borrow useful, established product patterns when they serve users. Existing competitors are not a reason to reject a feature; do not impose uniqueness as a product requirement or invent extra approval gates.
- Update the existing relevant document when a decision changes; do not create extra reports or parallel specifications unless requested.
- Keep research and temporary evidence under the ignored `research/` directory. Keep the maintained visual Demo in the version-controlled `design-demo/` directory. Do not force-add research or commit local credentials and runtime data.
- Reuse shared UI components and styles when implementing the approved Demo; do not invent a separate theme for each screen. Reconfirm affected Demo screens before changing approved visuals or interactions during feature development.

## Logo design

- The user-selected Memivy logo is the lowercase m with a leaf on a yellow rounded tile (`#FFD02F`). The design assets are in [design-demo/brand/](design-demo/brand/). Reuse this selected logo for subsequent UI design and implementation work.
- Use [memivy-icon.svg](design-demo/brand/memivy-icon.svg) for the standalone icon, [memivy-logo.svg](design-demo/brand/memivy-logo.svg) for the horizontal logo on light backgrounds, and [memivy-logo-dark.svg](design-demo/brand/memivy-logo-dark.svg) on dark backgrounds.
- [memivy-icon-1024.png](design-demo/brand/memivy-icon-1024.png) provides the bitmap icon; [favicon.ico](design-demo/brand/favicon.ico) is the browser favicon.
- The macOS 26 application package uses the full-bleed derivative in `src-tauri/icons/`, generated from the selected vector by `node scripts/generate-app-icon.mjs`. The OS supplies the app-icon corner mask; keep the original Demo assets unchanged. This export adjustment was requested on 2026-09-05 after the user observed a small icon inside the system frame.

## Setup and validation

Run from the repository root. Phase 1 was built with Node 24.12.0, Rust 1.98.1, and macOS 26.6.2 on Apple Silicon with Xcode Command Line Tools. `rust-toolchain.toml`, `Cargo.lock`, and `package-lock.json` pin the tested toolchain and dependency graph.

- Install frontend dependencies: `npm install` (the sandbox run used `npm --cache /private/tmp/memivy-npm-cache install`).
- Check and build frontend: `npm run build`.
- Check Rust: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --offline -- -D warnings`.
- Test core behavior: `cargo test --workspace --offline`.
- Build test harnesses: `cargo build -p memivy-core --example probe --offline` and `cargo build -p memivy-mcp --offline`; then run `python3 scripts/verify_phase1.py` for actual process/stdio tests.
- Build a local native test bundle: `npm run tauri build -- --debug --bundles app`. Ensure `~/.cargo/bin` is in `PATH`; this session used `PATH=/Users/bo/.cargo/bin:$PATH` before the command. The output is `target/debug/bundle/macos/Memivy Phase 1.app` and is not a signed release.
- The default isolated database is `~/Library/Application Support/com.memivy.phase1/phase1.sqlite3`. `MEMIVY_PHASE1_DATA_DIR` overrides its absolute directory for test runs; UI and MCP must use the same value. Current manual QA uses ignored `research/runtime/`. MCP defaults off.
- Model probe: `target/debug/examples/probe model /absolute/path/to/private-config.json`. The file must be outside the repository, mode `0600`, and contain `base_url`, `model`, and optional `api_key`. The base URL includes the API prefix such as `/v1`; the probe appends `/chat/completions`. It sends synthetic text only and performs no memory writes.

Commands above require dependencies to have been fetched before using `--offline`. The Phase 1 capture host now uses `src-tauri/src/capture_panel.rs` and a commit-pinned `tauri-nspanel` dependency; run all native panel operations on the main thread. Its diagnostic delayed entry exercises window behavior, not physical global-hotkey delivery. See the Phase 1 verification record in `DEVELOPMENT_PLAN.md` for actual results and outstanding native/model checks; do not infer full acceptance from a successful build.

- For documentation changes, check local links, consistency with the authoritative documents, and that unrelated files remain unchanged.
- For implementation changes, run the relevant available checks and add regression coverage for changed behavior. Prioritize raw-input preservation, failed AI calls, undo, keyword search, grounded answers, conversation/memory separation, explicit conclusion saving, cancellation, and credential leakage.
- Verify UI changes visually. For Tauri-specific behavior, test in the target app as well as the browser, especially Chinese input, focus, and window behavior.
- In the handoff, state what changed, which checks actually ran, and anything not verified. Do not treat a mockup, passing build, or published artifact as proof of a working release.
