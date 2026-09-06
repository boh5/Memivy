# AGENTS.md

## Repository status

Memivy is a local-first, open-source, AI-native memo app. The current implementation is limited to the Phase 1 technical-risk prototype. Do not implement later milestones or create application code as a side effect of a documentation or research task.

## Reference documents

- [PRD.md](PRD.md): product requirements, current platform scope, interaction rules, and acceptance criteria.
- [TECH_STACK.md](TECH_STACK.md): proposed architecture and technology choices; read before implementation or dependency changes.
- [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md): demo-first implementation sequence, approval record, and milestone checks; use the relevant stage for development work.
- [DESIGN.md](DESIGN.md): the selected Miro visual reference; read before UI work. Keep the official imported file unchanged unless the user explicitly requests an update. Do not insert translations, project instructions, or a replacement design system into it.

Use the relevant documents rather than duplicating them here. If a requested change conflicts with a documented decision, clarify the decision instead of silently changing product scope. Visual examples do not introduce new product features.

Build and iterate the clickable visual Demo first; do not start the production scaffold or functional development until the user explicitly approves it. After approval, use the recorded Demo version as the UI implementation and visual acceptance baseline. The planned entry is `design-demo/index.html`; check its approval status and version in `DEVELOPMENT_PLAN.md` rather than assuming an existing Demo is approved.

On 2026-09-05 the user explicitly authorized Phase 1 only, using Demo v0.1 as a visual reference while its functional interactions remain unapproved. This exception allows the technical prototype described in `DEVELOPMENT_PLAN.md`, not Phase 2 or production feature development. Keep prototype data separate from future production data and label it clearly.

## Implementation boundaries

- Keep the MVP focused on memory capture, organization, and retrieval. Do not add to-dos, reminders, embedding, synchronization, a hosted backend, or additional platform support without an explicit scope change.
- Save raw captures locally before starting AI processing. AI must never rewrite or delete the original input; edits to current memory content retain versions and provenance.
- AI actions must produce a visible receipt and be reversible. Model failures must not prevent capture or keyword search. Do not substitute a summary for the required action receipt.
- SQLite is the source of truth; Markdown is an export format. Search uses FTS5 without model calls or vector indexes in the MVP.
- Follow the planned shared Rust core boundary: UI and MCP must not bypass it to mutate the database or duplicate memory rules.
- Store BYOM credentials in a separate local app-data configuration file, not Keychain or a hosted key service. Keep keys out of the content database, exports, backup metadata, logs, and repository files.
- MCP exposes only `memory_capture` and `memory_search`; capture requires the user's explicit intent to save, and search returns bounded results with provenance. Do not add bulk-read or mutation tools.

## Change discipline

- Make small, task-scoped changes. Preserve unrelated work and avoid speculative abstractions or dependencies for deferred features.
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
- For implementation changes, run the relevant available checks and add regression coverage for changed behavior. Prioritize raw-input preservation, failed AI calls, undo, search, and credential leakage.
- Verify UI changes visually. For Tauri-specific behavior, test in the target app as well as the browser, especially Chinese input, focus, and window behavior.
- In the handoff, state what changed, which checks actually ran, and anything not verified. Do not treat a mockup, passing build, or published artifact as proof of a working release.
