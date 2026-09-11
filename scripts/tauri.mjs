import { withRustPath } from './rust-env.mjs';

process.env.PATH = withRustPath().PATH;
if (process.argv[2] === 'dev') {
  // macOS dev sets an NSImage directly, so supply its own transparent margins
  // and rounded silhouette. Packaged apps keep the full-bleed bundle artwork.
  const separator = process.argv.indexOf('--', 3);
  process.argv.splice(separator < 0 ? process.argv.length : separator, 0,
    '--config', JSON.stringify({ bundle: { icon: ['icons/dev-icon.icns', 'icons/dev-icon.png'] } }));
}
// Keep the official CLI's argument parsing, exit handling and dev lifecycle.
await import('@tauri-apps/cli/tauri.js');
