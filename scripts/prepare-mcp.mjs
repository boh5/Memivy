// Build self-contained sidecars; never include local runtime data or config.
import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { withRustPath } from './rust-env.mjs';
process.env.PATH = withRustPath().PATH;
const root = fileURLToPath(new URL('..', import.meta.url));
if (process.platform !== 'darwin' || process.arch !== 'arm64' ||
    (process.env.TAURI_ENV_ARCH && !['aarch64', 'arm64'].includes(process.env.TAURI_ENV_ARCH))) {
  throw new Error('Memivy requires an Apple Silicon Mac.');
}
const debug = process.argv.includes('--debug') || process.env.TAURI_ENV_DEBUG === 'true';
const names = ['memivy-mcp', 'memivy-embedding', 'memivy-speech'];
// One dependency graph lets Cargo share features and schedule independent crates.
const args = ['build', '--bins', '--locked', '--offline', '--message-format=json-render-diagnostics',
  ...names.flatMap(name => ['-p', name])];
// Match the release app's target; dev helpers must remain beside target/debug/memivy.
if (!debug) args.push('--release', '--target', 'aarch64-apple-darwin');
const result = spawnSync('cargo', args, {
  cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 32 * 1024 * 1024,
  env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: '26.0' },
});
if (result.error) throw new Error(`Could not build sidecars: ${result.error.message}. Ensure Rust is installed and its bin directory is in PATH.`, { cause: result.error });
if (result.status !== 0) process.exit(result.status ?? 1);
const artifacts = result.stdout.split('\n').filter(Boolean).map(line => JSON.parse(line));
// Validate every reported artifact before replacing any staged sidecar.
const executables = names.map(name => {
  const executable = artifacts.findLast(message => message.reason === 'compiler-artifact' &&
    message.target?.name === name && message.target.kind.includes('bin') && message.executable)?.executable;
  if (!executable) throw new Error(`Cargo did not report a ${name} executable; refusing to package an old file.`);
  const arch = spawnSync('lipo', [executable, '-verify_arch', 'arm64'], { stdio: 'inherit' });
  if (arch.status !== 0) throw new Error(`The ${name} artifact is not an Apple Silicon executable.`);
  return executable;
});
const folder = path.join(root, 'src-tauri/binaries');
mkdirSync(folder, { recursive: true });
for (let i = 0; i < names.length; i++) {
  const binary = path.join(folder, `${names[i]}-aarch64-apple-darwin`);
  copyFileSync(executables[i], binary);
  chmodSync(binary, 0o755);
}
