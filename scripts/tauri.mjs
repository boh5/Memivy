import { withRustPath } from './rust-env.mjs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

process.env.PATH = withRustPath().PATH;
if (process.argv[2] === 'dev') {
  const runtime = fileURLToPath(new URL('./dev-runtime.py', import.meta.url));
  if (!process.env.MEMIVY_DEV_SESSION) {
    const result = spawnSync('python3', [runtime, 'session', process.execPath,
      fileURLToPath(import.meta.url), ...process.argv.slice(2)], { stdio: 'inherit' });
    if (result.error) throw result.error;
    process.exit(result.status ?? 1);
  }
  // Cargo's executable runner puts every hot-reloaded build in the same bundle.
  // Tauri still owns its normal watcher, frontend server and process lifecycle.
  const runner = ['python3', runtime, 'run'];
  const override = {
    productName: 'Memivy Dev', identifier: 'com.memivy.app.dev',
    build: { runner: { cmd: 'cargo', args: ['--config',
      `target.aarch64-apple-darwin.runner=${JSON.stringify(runner)}`] } },
    bundle: { icon: ['icons/dev-icon.icns', 'icons/dev-icon.png'] },
  };
  const separator = process.argv.indexOf('--', 3);
  process.argv.splice(separator < 0 ? process.argv.length : separator, 0,
    '--config', JSON.stringify(override));
}
// Keep the official CLI's argument parsing, exit handling and dev lifecycle.
await import('@tauri-apps/cli/tauri.js');
