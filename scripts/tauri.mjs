import { withRustPath } from './rust-env.mjs';

process.env.PATH = withRustPath().PATH;
// Keep the official CLI's argument parsing, exit handling and dev lifecycle.
await import('@tauri-apps/cli/tauri.js');
