import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { cpSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { withRustPath } from './rust-env.mjs';
process.env.PATH = withRustPath().PATH;
const root = fileURLToPath(new URL('..', import.meta.url));
if (process.platform !== 'darwin' || process.arch !== 'arm64') throw new Error('Use an Apple Silicon Mac to build this release.');
// Ad-hoc distribution package: no submission to Apple or use of a host identity.
const env = { ...process.env, MACOSX_DEPLOYMENT_TARGET: '26.0', APPLE_SIGNING_IDENTITY: '-' };
for (const key of ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID', 'APPLE_API_KEY', 'APPLE_API_KEY_PATH', 'APPLE_API_ISSUER']) delete env[key];
function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', env });
  if (result.status !== 0) throw new Error(`${command} failed (${result.status ?? 'could not start'}).`);
}
function verifyMicrophoneAccess(app) {
  const options = { cwd: root, encoding: 'utf8', env };
  const signature = spawnSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', app], options);
  if (signature.status !== 0) throw new Error(`The built application signature is invalid; refusing to package it. ${signature.stderr?.trim() || ''}`);
  const signedEntitlements = spawnSync('/usr/bin/codesign', ['--display', '--entitlements', '-', '--xml', app], options);
  const plist = spawnSync('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], { ...options, input: signedEntitlements.stdout });
  if (signedEntitlements.status !== 0 || plist.status !== 0 || JSON.parse(plist.stdout)['com.apple.security.device.audio-input'] !== true) {
    throw new Error('The built application lacks a signed audio-input entitlement set to true; refusing to package it.');
  }
}
// Cargo may put outputs outside the repository. Use the same metadata and an
// explicit architecture as Tauri; a previous default-target bundle is not evidence.
const metadata = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--locked', '--offline'], { cwd: root, encoding: 'utf8', env });
if (metadata.status !== 0) throw new Error('Cannot resolve the Cargo build directory.');
const workspace = JSON.parse(metadata.stdout);
const version = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version;
run('node', ['scripts/check-release.mjs']);
const targetDirectory = workspace.target_directory;
if (typeof targetDirectory !== 'string' || !path.isAbsolute(targetDirectory)) throw new Error('Invalid Cargo build directory.');
const target = 'aarch64-apple-darwin';
run('npm', ['run', 'tauri', '--', 'build', '--target', target, '--config', 'src-tauri/tauri.release.conf.json', '--bundles', 'app']);
const builtApp = path.join(targetDirectory, target, 'release/bundle/macos/Memivy.app');
const folder = path.join(root, 'target/release/bundle/dmg');
mkdirSync(folder, { recursive: true });
const dmg = path.join(folder, `Memivy_${version}_aarch64.dmg`);
const stage = mkdtempSync(path.join(tmpdir(), 'memivy-release-package-'));
try {
  const stagedApp = path.join(stage, 'Memivy.app');
  cpSync(builtApp, stagedApp, { recursive: true });
  // Check the actual bundle that will enter the DMG, including its embedded signature.
  verifyMicrophoneAccess(stagedApp);
  cpSync(path.join(root, 'LICENSE'), path.join(stage, 'LICENSE'));
  cpSync(path.join(root, 'docs/INSTALL.md'), path.join(stage, 'Installation.md'));
  symlinkSync('/Applications', path.join(stage, 'Applications'));
  // Standard disk image, with an Applications link. No Finder/AppleScript or
  // machine-specific window arrangement is required to build the installer.
  run('hdiutil', ['create', '-volname', 'Memivy', '-srcfolder', stage, '-format', 'UDZO', '-ov', dmg]);
  const digest = createHash('sha256').update(readFileSync(dmg)).digest('hex');
  writeFileSync(`${dmg}.sha256`, `${digest}  ${path.basename(dmg)}\n`);
  console.log(`Release package: ${dmg}\nAd-hoc signed; NOT notarized.`);
} finally { rmSync(stage, { recursive: true, force: true }); }
