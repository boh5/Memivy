// Build one self-contained sidecar; never include local runtime data or config.
import { spawnSync } from 'node:child_process';
import { mkdirSync, copyFileSync, chmodSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { withRustPath } from './rust-env.mjs';
process.env.PATH = withRustPath().PATH;
const root = fileURLToPath(new URL('..', import.meta.url));
if (process.platform !== 'darwin' || process.arch !== 'arm64' ||
    (process.env.TAURI_ENV_ARCH && !['aarch64', 'arm64'].includes(process.env.TAURI_ENV_ARCH))) {
  throw new Error('Memivy development beta requires an Apple Silicon Mac.');
}
const debug = process.argv.includes('--debug') || process.env.TAURI_ENV_DEBUG === 'true';
for (const name of ['memivy-mcp', 'memivy-embedding']) {
const args = ['build', '-p', name, '--bin', name, '--locked', '--offline', '--message-format=json-render-diagnostics'];
if (!debug) args.push('--release');
const result = spawnSync('cargo', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'], env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: '26.0' } });
if (result.error) throw new Error(`Could not start cargo for ${name}: ${result.error.message}. Ensure Rust is installed and its bin directory is in PATH.`, { cause: result.error });
if (result.status !== 0) process.exit(result.status ?? 1);
const artifacts = result.stdout.split('\n').filter(Boolean).map(line => JSON.parse(line));
const executable = artifacts.findLast(message => message.reason === 'compiler-artifact' &&
  message.target?.name === name && message.target.kind.includes('bin') && message.executable)?.executable;
if (!executable) throw new Error('Cargo did not report a memivy-mcp executable; refusing to package an old file.');
const arch = spawnSync('lipo', [executable, '-verify_arch', 'arm64'], { stdio: 'inherit' });
if (arch.status !== 0) throw new Error('The MCP artifact is not an Apple Silicon executable.');
const folder = path.join(root, 'src-tauri/binaries');
mkdirSync(folder, { recursive: true });
const binary = path.join(folder, `${name}-aarch64-apple-darwin`);
copyFileSync(executable, binary);
chmodSync(binary, 0o755);

}
