# AGENTS.md

## Working scope

Memivy is a local-first personal memory app for Apple Silicon and macOS 26+.
Use the formal `MemoryStore` application, including the main window and desktop
quick window. Do not restore the removed prototype, its stores, migrations, MCP
entry points or model-configuration imports. Source cleanup must not delete user data.

Keep changes focused on the user's request. Preserve unrelated work.

If a major problem, conflicting requirement or substantial change to the agreed
approach emerges, stop and explain it to the user before proceeding. Ask for a
decision; do not silently reinterpret the request or perform a broad workaround.

Do not add platforms, cloud sync, a hosted backend, reminders or extra MCP tools without a
scope change. Prefer existing components and direct implementations over new
frameworks or speculative abstractions.

## Language and documentation

- Write all project-authored code in English: identifiers, comments, docstrings,
  prompts, tool/schema descriptions, errors, logs, command output, scripts, tests,
  configuration, SQL, CSS and executable demos. Use English for ordinary examples.
- Localized UI text belongs in translation resources, including native dialogs,
  export dialog labels and language names. Preserve both English and Chinese interfaces.
  English prompts must still follow the user's conversation language.
- Write necessary Chinese, IME, Unicode and multilingual test or diagnostic
  samples directly in the test/probe code. Keep test names, comments and assertion
  explanations in English. Use separate fixtures only for actual reuse, sizable
  datasets or file-loading tests, never solely to move Chinese out of code.
- Language-specific terms used to match user input may remain inline. They are
  input data, not translated UI text; do not create locale files just to hide them.
- Preserve actual user notes, quotations, transcripts and stored history in their
  original language. Language cleanup must not rewrite user data.
- Development documents and README documents may use Chinese. Public documents
  should explain what users can do and the steps they need, without internal
  approval conversations, implementation jargon or repeated warnings.
- Keep this file limited to current rules. Put implementation history and actual
  check results in `DEVELOPMENT_PLAN.md`, not here. Report research in chat unless
  the user requests a document; do not create extra reports or parallel plans.

## Architecture and data

- React/Tauri provides the interface; the shared Rust core owns storage and memory
  rules. The UI and MCP must not bypass `MemoryStore` to mutate SQLite.
- SQLite is the source of truth. Current memory text supports keyword and optional
  semantic search. Original input and older versions remain available as evidence
  and recovery material, not a competing primary search index.
- Save original input locally before AI processing. Preserve originals, version
  history and provenance. AI changes must have visible, reversible receipts.
  Model failure must not block ordinary capture or keyword search.
- Conversations and their compression summaries are separate from durable memories.
  Save meaningful user expressions while distinguishing tentative ideas, decisions
  and completed actions. Questions and AI suggestions must not become user facts.
- Recall relevant global memories before answering and use bounded tools to read
  further evidence. Selected notes or collections focus a discussion without
  hiding relevant global constraints. Cite the actual source versions used.
- Preserve user drafts, concurrent-edit checks, cancellation and retries. Undo must
  not overwrite later edits. Deleting a conversation must preserve already saved
  memories, their source records and grouped undo.
- Deleted memories go to Trash and leave normal search and AI recall. Preserve
  shared sources and independently deleted items. Empty Trash only on user action.
  Removing a collection must not delete its memories. Collection membership is
  manual: AI may recommend collections but must not add memories without confirmation.
- The first public database uses schema 1 in `migrations/memory/001_initial.sql`.
  Do not restore development-era migrations or compatibility conversions. After
  release, keep the initial schema unchanged and add migrations from schema 2.
- Back up the current library before restoring another backup. Keep credentials
  and model weights out of database backups, exports, logs and repository files.
  Credentials live in separate local configuration, not Keychain or a hosted vault.

## Models, voice and MCP

- Use the existing model client and bounded tool loops. Unsupported model
  capabilities must fail clearly; do not silently switch models or revive old flows.
- Local embedding and speech use their existing helper processes. Both share
  Memivy's system cache: `~/Library/Caches/com.memivy.app/models/` on macOS,
  independent of app identifier and test library. Do not use a global HF cache.
- Voice input shares the text draft. Finish transcription before submitting and
  preserve recoverable input on failure. Stopping recording must not send it.
- MCP uses local stdio and exposes only `memory_capture` and `memory_search`.
  It starts disabled. Capture requires explicit intent to save; search returns
  bounded saved-memory results with sources, never unsaved conversations.
- Test actual model requests, raw tool arguments and storage effects when changing
  prompts or agent behavior. Fix controllable defects; report remaining model
  quality observations without claiming every semantic outcome is guaranteed.

## Interface and design

Preserve the current layout, white/yellow branding and shared Markdown editor.
Keep formatting controls attached to text selection, with direct Markdown input.
Do not introduce a persistent toolbar, a source-mode editor or task-management
features as a side effect of editing document formatting.

Preserve focus, selection, scroll position, drafts and Chinese IME composition in
both windows. Command+Enter submits; Enter inserts a newline. Run native panel
operations on the main thread. Verify global shortcuts and window handoff natively.

Reuse the selected leaf-and-m logo in `design-demo/brand/`. Keep official design
assets unchanged unless requested. App icons are generated from those assets by
`node scripts/generate-app-icon.mjs`; the OS supplies the packaged icon's corner mask.
The clickable demos are visual references, not an alternate product runtime.

## Setup and verification

Use the versions in `.node-version` and `rust-toolchain.toml`. Start with `npm ci`
and `cargo fetch --locked`; offline checks require dependencies to be present.
Ensure Cargo and CMake are on PATH.

- Development: `npm run dev:app` belongs to the development identity and must use
  a separate development library, following the rules below. `npm run dev` alone
  is a browser preview. Verify the effective identity and data directory before
  native testing; do not assume existing launchers already enforce these rules.
- Frontend: `npm run build`, `npm run test:ui`, `npm run i18n:check`.
- Rust: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --offline -- -D warnings`,
  and `cargo test --workspace --offline`.
- Full regression: `npm run test:core-assets`, then
  `python3 scripts/verify_restore.py`. Use synthetic isolated data.
- Run focused checks while iterating and the relevant broader checks before
  handoff. Use independent review, fix confirmed findings, and re-review changes
  to agent behavior, prompts or data handling.
- Verify UI changes visually and Tauri behavior in the native app. Build success
  does not prove input, microphone, global-hotkey or installation behavior.
- Never delete shared production models or use personal data to simulate failures.
  Keep real model credentials outside the repository in a private `0600` file.
- Keep temporary evidence under ignored `research/`. State which checks ran and
  what remains unverified; do not claim user acceptance from tests or review.

## Installed app, development and QA isolation

- Keep exactly two stable application identities: the installed `Memivy`
  (`com.memivy.app`) for daily use, and `Memivy Dev` (`com.memivy.app.dev`) for
  development and native QA. Dev commands and QA share the development identity;
  do not create additional names or identifiers per thread, feature, date or test.
- Preserve the installed application, normally `/Applications/Memivy.app`, and
  its production library at `~/Library/Application Support/com.memivy.app/`.
  Development defaults to `~/Library/Application Support/com.memivy.app.dev/`.
  QA must explicitly set an absolute `MEMIVY_DATA_DIR` for synthetic isolated
  test data and verify the opened library before interacting with it. Changing
  the bundle identifier alone is not proof of data isolation. Preserve developer
  notes and drafts in the persistent development library as user data.
- Reuse `~/Applications/Memivy Dev.app` across threads and worktrees. Use
  `npm run dev:app` for persistent development. For synthetic native QA, set an
  absolute `MEMIVY_DATA_DIR` and run `npm run qa:app`. Both use `scripts/dev-runtime.py`;
  do not create alternate bundles or launch raw Cargo binaries for native QA.
  The launcher stops its owned processes on exit and retains the selected library;
  explicitly clean only your disposable QA data after inspecting results.
- Serialize native QA across threads using a shared ownership/lock mechanism.
  The launcher holds `~/Library/Caches/com.memivy.app.dev/native-session.lock`;
  an occupied session fails clearly. Wait and retry; never delete the lock file.
  Check ownership before rebuilding the shared app or starting a session; wait
  while another thread owns it. Do not overwrite or terminate another session.
  Independent coding and isolated non-UI tests may continue in parallel.
- The installed and development apps may coexist. Avoid conflicting global
  shortcuts without changing the installed app's preferences. Verify coexistence
  natively; do not quit the user's installed app merely to make a test pass.
- At test completion, stop only processes owned by the test, release its lock,
  and remove disposable test data and generated artifacts. Preserve user data,
  shared model caches and other threads' work. Retain only necessary failure
  evidence under ignored `research/`, with its purpose recorded. Do not clear
  shared build caches while another thread is building or testing.
- Installation, upgrade and uninstall acceptance tests use the release identity.
  Run them in a separate macOS test account or coordinate explicitly with the
  user before replacing or removing their installed app. Never silently use the
  daily installation or production library as a disposable test environment.

## Distribution and Git

The initial public release is `0.1.1` / `v0.1.1`, MIT, copyright Huang Bo, for `boh5/memivy`.
Use `npm run build:release` and `src-tauri/tauri.release.conf.json`. Packages are
ad-hoc signed and unnotarized; no paid Apple Developer account is assumed.
Follow `docs/RELEASING.md`: CI creates a draft, and the actual download needs
installation checks before publication.

Commit only when requested, using English Conventional Commits. Do not push,
create a remote repository, tag or publish a release without explicit authorization.

## References

- `PRD.md`: product scope and behavior.
- `TECH_STACK.md`: architecture; read before dependency or structural changes.
- `DESIGN.md` and `design-demo/`: visual references; do not rewrite official designs.
- `DEVELOPMENT_PLAN.md`: implementation history and verification evidence.
- `docs/goals/second-memory-agent-plan-goal.md` and its progress document: agent
  behavior and acceptance scenarios. Current rules above supersede older conflicts.
- `README.md` and `README.zh-CN.md`: installation, usage, and data privacy.
- `docs/RELEASING.md`: release preparation, verification, and publication.
