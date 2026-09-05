# AGENTS.md

## Repository status

Memivy is a local-first, open-source, AI-native memo app. This repository currently contains planning and design documents, not an application scaffold. Do not create application code as a side effect of a documentation or research task.

## Reference documents

- [PRD.md](PRD.md): product requirements, current platform scope, interaction rules, and acceptance criteria.
- [TECH_STACK.md](TECH_STACK.md): proposed architecture and technology choices; read before implementation or dependency changes.
- [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md): demo-first implementation sequence, approval record, and milestone checks; use the relevant stage for development work.
- [DESIGN.md](DESIGN.md): the selected Miro visual reference; read before UI work. Keep the official imported file unchanged unless the user explicitly requests an update. Do not insert translations, project instructions, or a replacement design system into it.

Use the relevant documents rather than duplicating them here. If a requested change conflicts with a documented decision, clarify the decision instead of silently changing product scope. Visual examples do not introduce new product features.

Build and iterate the clickable visual Demo first; do not start the production scaffold or functional development until the user explicitly approves it. After approval, use the recorded Demo version as the UI implementation and visual acceptance baseline. The planned entry is `design-demo/index.html`; check its approval status and version in `DEVELOPMENT_PLAN.md` rather than assuming an existing Demo is approved.

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

## Setup and validation

There is currently no `package.json`, `Cargo.toml`, application build, or test suite. No setup, development, lint, build, or test commands are established yet. Do not invent commands or claim planned tooling has run. When adding the actual scaffold, replace this paragraph with verified commands and their working directories.

- For documentation changes, check local links, consistency with the authoritative documents, and that unrelated files remain unchanged.
- For implementation changes, run the relevant available checks and add regression coverage for changed behavior. Prioritize raw-input preservation, failed AI calls, undo, search, and credential leakage.
- Verify UI changes visually. For Tauri-specific behavior, test in the target app as well as the browser, especially Chinese input, focus, and window behavior.
- In the handoff, state what changed, which checks actually ran, and anything not verified. Do not treat a mockup, passing build, or published artifact as proof of a working release.
