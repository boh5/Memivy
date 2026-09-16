# Contributing to Memivy

Bug reports, documentation fixes, and pull requests are welcome. For a larger
feature or a change to how memories are stored, open an issue first so we can
discuss the approach before you spend time implementing it.

## Report a bug or suggest a feature

[Open an issue](https://github.com/boh5/memivy/issues/new/choose). For a bug, include
steps we can follow, what you expected, and what happened instead. For a feature,
describe what you were trying to do and where the app fell short.

Use sample notes instead of personal data. Report vulnerabilities through
[Security](SECURITY.md), rather than a public issue.

## Run the app locally

Memivy uses React and Tauri with a shared Rust core. To build it, you need an
Apple Silicon Mac running macOS 26+, Xcode command-line tools with the macOS 26+
SDK, Node **24.12.0**, Rust **1.98.1**, Python 3.11+, and CMake.
The Node and Rust versions are pinned in `.node-version` and `rust-toolchain.toml`.
Make sure `cargo` and `cmake` are on your PATH.

```sh
git clone https://github.com/boh5/memivy.git
cd memivy
npm ci
cargo fetch --locked
npm run dev:app
```

`npm run dev:app` runs `~/Applications/Memivy Dev.app` with frontend hot reload and
Rust rebuilds. Its default library is
`~/Library/Application Support/com.memivy.app.dev/`, separate from the installed
app's `~/Library/Application Support/com.memivy.app/`.

Quit the development app or press Ctrl+C in its terminal to end the session.
`npm run dev` starts only the browser preview.

Fresh Dev libraries use **Option+Shift+M** for quick capture, leaving the installed
app's **Option+M** shortcut available. Dev does not enable launch at login
automatically.

## Test a change

For code changes, run:

```sh
npm run i18n:check
npm run test:core-assets
python3 scripts/verify_restore.py
```

The core suite runs formatting, lint, Rust and UI tests, the frontend build, and
data regression checks. It also builds the examples used by the restore check.
These checks use isolated data and do not require model credentials.

For model or prompt changes, also test real model requests with made-up examples.
For input, window, or packaging changes, test the desktop app. For documentation
changes, check the instructions and links.

### Native tests with a disposable library

Set `MEMIVY_DATA_DIR` to an absolute path for synthetic test data and run
`npm run qa:app`. Check the selected library before testing edits, deletion, or
restore. The launcher refuses the production and persistent Dev libraries in QA mode.

QA uses the same Memivy Dev app as ordinary development. Only one native session
can run across all checkouts at a time. If another session owns the lock, wait for
it to finish; do not remove the lock or stop someone else's process.

The launcher stops its processes on exit but keeps the test library for inspection.
Remove your disposable data when finished. Keep the persistent Dev library and
shared model cache intact. Installation and upgrade tests belong on a separate
macOS test account, where replacing the release app will not affect daily use.

To build a DMG, run `npm run build:release`. Packaging and publication are covered
in [Releasing](docs/RELEASING.md).

## Send a pull request

Explain the problem, what your change does, and how you tested it. Include a
screenshot for visible changes and mention any checks you could not run.

Keep each PR focused. Preserve original notes, version history, and undo behavior.
Keep private data and credentials out of commits and attachments.

Write code and comments in English. Keep UI text in translation resources, with
both English and Chinese translations. The Chinese README and development
documents may be written in Chinese.

Use Conventional Commits, such as `fix: preserve the draft when closing a window`.
Code contributions are covered by the project's [MIT license](LICENSE).
