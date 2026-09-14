import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { withRustPath } from './rust-env.mjs';
const read = name => JSON.parse(readFileSync(name, 'utf8'));
const pkg = read('package.json');
const lock = read('package-lock.json');
const tauri = read('src-tauri/tauri.conf.json');
const version = pkg.version;
if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('A stable x.y.z release version is required.');
if ([lock.version, lock.packages[''].version, tauri.version].some(v => v !== version)) throw new Error('npm, lockfile and Tauri versions differ.');
const result = spawnSync('cargo', ['metadata', '--no-deps', '--locked', '--offline', '--format-version=1'], {encoding:'utf8', env:withRustPath()});
if (result.status !== 0) throw new Error('Cargo metadata failed. Fetch dependencies with cargo fetch --locked first.');
const metadata = JSON.parse(result.stdout);
for (const id of metadata.workspace_members) {
  const p = metadata.packages.find(p => p.id === id);
  if (p.version !== version || p.license !== 'MIT') throw new Error(`${p.name}: version or license differs.`);
}
const tag = process.argv[2];
if (tag !== undefined && tag !== `v${version}`) throw new Error(`Expected tag v${version}.`);
if (!readFileSync('CHANGELOG.md','utf8').includes(`## ${version}\n`)) throw new Error('Missing changelog section.');
console.log(`Release metadata valid: ${version}${tag ? ` (${tag})` : ''}`);
