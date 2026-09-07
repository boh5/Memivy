import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
const root = resolve(import.meta.dirname, '..');
const work = resolve(root, 'research/tray-icon');
mkdirSync(work, { recursive: true });
const source = readFileSync(resolve(root, 'design-demo/brand/memivy-icon.svg'), 'utf8');
// The system menu bar supplies the color. Keep the approved m and leaf paths.
const template = source.replace(/<rect[^>]*\/>/, '')
  .replace('viewBox="0 0 48 48"', 'viewBox="9 6 32 32"')
  .replace('stroke="#1c1c1e"', 'stroke="#000"');
writeFileSync(resolve(work, 'tray.svg'), template);
execFileSync(resolve(root, 'node_modules/.bin/tauri'), ['icon', resolve(work, 'tray.svg'), '-o', work], { stdio: 'ignore' });
copyFileSync(resolve(work, '32x32.png'), resolve(root, 'src-tauri/icons/tray-icon.png'));
