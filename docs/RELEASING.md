# Release Memivy

Releases support Apple Silicon and macOS 26+. The build uses ad-hoc signing without
Apple notarization. No Apple signing credentials are needed.

## Set up the repository

Enable GitHub Actions and private vulnerability reporting in `boh5/memivy`.
Use read-only workflow permissions by default; the draft upload job requests write
access. Protect `main` with the CI check and restrict release tags to maintainers.
Check the actual CI check name after the first run when configuring branch rules.

## Prepare a version

1. Update the version in `package.json`, the root entries of `package-lock.json`,
   the Cargo workspace and its lockfile entries, and `src-tauri/tauri.conf.json`.
2. Add a `## <version>` section to `CHANGELOG.md` describing changes users will notice.
3. Run the checks below and review dependency vulnerabilities and possible leaked secrets.
4. Commit the changes and wait for CI to pass on `main`.

```sh
npm ci
cargo fetch --locked
npm run release:check
npm run i18n:check
npm run test:core-assets
python3 scripts/verify_restore.py
npm run build:release
```

## Create a draft

Create an annotated `v<version>` tag on the checked commit on `main` and push the tag.
The Release draft workflow runs the checks, builds the app and creates a GitHub
Release draft containing the DMG and its `.sha256` file. The notes include only
that version's changelog entry and the source commit.

The workflow does not publish the release. It stops if a release already exists
for that tag. If a run fails before creating a draft, fix the cause and rerun it.
If a draft already exists, inspect it before retrying.

## Test the download

Download the draft DMG through a browser on another supported Mac or a separate
user account without development tools or cached models. Follow the
[installation guide](INSTALL.md), including the checksum check, and confirm:

- First launch works, including the macOS permission to open the app.
- Notes can be saved without a model and are still there after restarting.
- Chinese typing, the desktop quick window and the global shortcut work.
- Voice input handles microphone permission being allowed or denied; local models
  download and load, and interrupted downloads can be retried.
- AI chat can find saved notes, show its changes and undo them.
- MCP starts disabled; after enabling it, another tool can save and search notes.
- Backup, restore and replacing the app preserve the expected notes, drafts and settings.

Use made-up notes. Record the test results and any failures in DEVELOPMENT_PLAN.md.

## Publish

Once the download checks pass, publish the tested draft with Pre-release unchecked
and Latest selected. Download the public asset and verify its checksum once more.
Do not move a published tag or replace its binaries; ship a new version for a fix.

## Database changes

Version 0.1.0 starts the public database at schema 1. Development databases using
other schemas are unsupported. Keep the shipped initial schema unchanged and add
future changes as schema 2+ migrations, with backup and recovery tests.
An older app may not open a newer database, so returning to an older version may
also require restoring a compatible backup. There is no automatic app updater.
