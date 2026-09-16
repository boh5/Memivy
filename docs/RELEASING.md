# Releasing Memivy

Pushing a version tag starts the release workflow. It builds a DMG and creates a
GitHub Release draft. Test the downloaded app before publishing the draft.

Packages target Apple Silicon and macOS 26+. They are ad-hoc signed, without Apple
notarization. Building them does not require an Apple Developer account.

## Repository setup

Before the first release:

- Enable GitHub Actions and private vulnerability reporting in `boh5/memivy`.
- Use read-only workflow permissions by default. The draft job requests the write access it needs.
- Protect `main` with the CI check, using its name from a completed run, and restrict release tags to maintainers.

## 1. Prepare the version

Update the version in `package.json`, the root entries in `package-lock.json`,
`Cargo.toml`, the workspace packages' entries in `Cargo.lock`, and
`src-tauri/tauri.conf.json`. Add a `## <version>` entry to `CHANGELOG.md` describing
what changed for users. The release workflow uses that entry as its release notes.

With the [build prerequisites](../CONTRIBUTING.md#run-the-app-locally) installed, run:

```sh
npm ci
cargo fetch --locked
npm run release:check
npm run i18n:check
npm run test:core-assets
python3 scripts/verify_restore.py
npm run build:release
```

`release:check` validates version and license metadata. The core suite runs the
Rust checks, UI tests, frontend build, and data regression checks. The release
build writes the DMG and its `.sha256` file to `target/release/bundle/dmg/`.

Review dependency vulnerabilities and check for accidentally included secrets.
Commit the release changes, merge them into `main`, and wait for CI to pass.

## 2. Create the draft

Create an annotated tag named `v<version>` on the checked commit on `main`, then
push that tag. This starts the **Release draft** workflow.

The workflow verifies the source and builds the app in parallel. Both jobs must
pass before the DMG, checksum, changelog entry, and source commit appear in the
draft.

A failed build or upload can be rerun against the same tag if no source change is
needed. If a fix changes the source, prepare and tag a new version instead.
The workflow stops when a release already exists for the tag; inspect that draft
before retrying, since it will not be overwritten.

## 3. Test the downloaded app

Download the draft DMG through a browser and follow [Install Memivy (Chinese)](../README.zh-CN.md#安装),
including checksum verification. Use another supported Mac or a separate macOS
test account without development tools or cached models.

Use sample notes and check the following:

- Install and open the app, including the macOS first-launch exception.
- Save notes without configuring a model. Restart and confirm they are still there.
- Type Chinese text, open the quick window with the global shortcut, and continue in the main window.
- Try voice input with microphone permission allowed and denied. Download the local models, load them, and retry an interrupted download.
- Ask AI about saved notes. Follow its sources, inspect a memory change, and undo it.
- Confirm MCP starts disabled. Enable it and use another AI app to save and search a memory.
- Back up and restore a library. Replace an existing installation with the new app and check that its notes, drafts, and settings are preserved.

Record results and any failures in `DEVELOPMENT_PLAN.md`. Publish after these
checks pass.

## 4. Publish

Publish the tested draft with **Pre-release** unchecked and **Latest** selected.
Download the public DMG and verify its checksum again.

Keep published tags and binaries unchanged. If a fix is needed, release a new
version. Users install updates manually; Memivy has no automatic updater.

## Database compatibility

The first public release, 0.1.0, uses schema 1. Pre-release development databases
with other schema numbers are unsupported.

Leave the shipped `migrations/memory/001_initial.sql` unchanged. Add later database
changes as migrations starting at schema 2, and test backup and recovery with each
change. Downgrading the app may require restoring a backup compatible with the
older version.
