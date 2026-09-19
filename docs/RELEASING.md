# Releasing Memivy

Pushing a version tag starts the release workflow. It builds a DMG, a signed updater archive and a version manifest, then creates a
GitHub Release draft. Test the downloaded app before publishing the draft.

Packages target Apple Silicon and macOS 26+. They are ad-hoc signed, without Apple
notarization. Building them does not require an Apple Developer account.

## Repository setup

Before the first release:

- Enable GitHub Actions and private vulnerability reporting in `boh5/memivy`.
- Use read-only workflow permissions by default. The draft job requests the write access it needs.
- Protect `main` with the CI check, using its name from a completed run, and restrict release tags to maintainers.

## Updater signing setup (once)

The updater signature is independent of Apple code signing and costs nothing.
Keep the existing key pair: changing its public key prevents already-installed
clients from accepting new updates. Back up the private key outside the repository.

In [repository Actions secrets](https://github.com/boh5/memivy/settings/secrets/actions),
choose **New repository secret** and add:

- `TAURI_SIGNING_PRIVATE_KEY`: the complete private-key file contents, not its path.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: only if the key has a password.

The public key belongs in `src-tauri/tauri.release.conf.json`. Never put the private
key in source, release assets, logs or chat. GitHub builds use the two secrets only
for packaging. Local builds can use an absolute file path without printing the key:

```sh
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.config/memivy-release/updater.key"
npm run build:release
```

If setting up a different project, generate its own key with `npm run tauri --
signer generate --ci -w /private/path/updater.key`; do not regenerate this project's
existing key for each release. The current key has no password; file permissions
and GitHub Secrets protect it.

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
Tauri also writes `Memivy.app.tar.gz` and `Memivy.app.tar.gz.sig` beside the built
app under the Cargo target's `aarch64-apple-darwin/release/bundle/macos/` directory.
CI uses `scripts/prepare-update.mjs` to copy those artifacts and generate
`latest.json` with a fixed URL for that version. Both packages contain the same app. CI also verifies the actual archive with the official updater and the
shipped public key before uploading assets.

Review dependency vulnerabilities and check for accidentally included secrets.
Commit the release changes, merge them into `main`, and wait for CI to pass.

## 2. Create the draft

Create an annotated tag named `v<version>` on the checked commit on `main`, then
push that tag. This starts the **Release draft** workflow.

The workflow verifies the source and builds the app in parallel. Both jobs must
pass before the DMG, checksum, updater archive, signature, `latest.json`, changelog
entry, and source commit appear in the draft.

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

Also test upgrading a release that already contains the updater in the separate
test account. Use settings → App updates: check, download, then restart and install.
A draft is not visible through the public latest endpoint; test draft artifacts
with a controlled test feed and a test-only build configuration, never by changing
the production latest release or replacing your daily installation. Verify:

- Both windows' drafts, notes, settings and cached models survive.
- Active recording, transcription, background work and external MCP sessions defer installation.
- Network or signature failure leaves the running app usable and supports retry.
- A read-only installation or an app launched from a DMG reports installation failure.
- Gatekeeper prompts, administrator authorization and microphone permissions are
  recorded separately after update and after another launch. Ad-hoc signing does
  not guarantee permission continuity; updater signatures do not change that.

Record results and any failures in `DEVELOPMENT_PLAN.md`. Publish after these
checks pass.

## 4. Publish

Publish the tested draft with **Pre-release** unchecked and **Latest** selected.
Download the public DMG and verify its checksum again.

Keep published tags and binaries unchanged. If a fix is needed, release a new
version. Users check and confirm updates in settings; there is no automatic
polling or silent installation. Users on a release without the updater must
manually install an updater-enabled release once. The DMG remains the fallback.

## Database compatibility

The first public release, 0.1.2, uses schema 1. Pre-release development databases
with other schema numbers are unsupported.

Leave the shipped `migrations/memory/001_initial.sql` unchanged. Add later database
changes as migrations starting at schema 2 and append them to `MIGRATIONS` in
`crates/memivy-core/src/memory/migrations.rs`. Scripts contain transactional SQL;
the runner owns the transaction and `user_version`. Test backup and recovery with
each change. Startup backs up to the library's private `recovery/` directory before
running missing migrations together in one transaction. Backup or migration
failure stops startup without resetting the library. Unchanged schemas do not
create migration backups. Older supported backups migrate on a private copy;
the original remains unchanged. MCP never performs startup migrations. Downgrading the app may require restoring a backup compatible with the
older version.
