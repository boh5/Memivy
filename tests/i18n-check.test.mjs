import test from 'node:test';
import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

test('i18n CLI accepts TypeScript and comments but rejects hardcoded UI literals', async t => {
  const root = await mkdtemp(path.join(tmpdir(), 'memivy-i18n-check-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  await mkdir(path.join(root, 'scripts'));
  await mkdir(path.join(root, 'src/workspace'), { recursive: true });
  await cp('scripts/check-i18n.mjs', path.join(root, 'scripts/check-i18n.mjs'));
  await cp('locales', path.join(root, 'locales'), { recursive: true });
  await symlink(path.resolve('node_modules'), path.join(root, 'node_modules'));
  await writeFile(path.join(root, 'src/workspace/valid.ts'), [
    '// 中文注释 is not UI text.',
    'export const identity = <T>(value: T): T => value;',
    'export const label = "English";',
    'export const pattern = /中文/u;',
  ].join('\n'));
  const run = () => spawnSync(process.execPath, ['scripts/check-i18n.mjs'], { cwd: root, encoding: 'utf8' });
  const clean = run();
  assert.equal(clean.status, 0, clean.stderr);
  await writeFile(path.join(root, 'src/workspace/invalid.tsx'), [
    'const name = "World";',
    'const plain = "普通";',
    String.raw`const escaped = "\u4e2d";`,
    'const template = `前${name}中${name}后`;',
    'const standalone = `独立`;',
    'export const view = <div title="属性">正文</div>;',
  ].join('\n'));
  const rejected = run();
  assert.equal(rejected.status, 1, rejected.stderr);
  for (const value of ['普通', '中', '前', '后', '独立', '属性', '正文']) {
    assert.ok(rejected.stderr.includes(JSON.stringify(value)), rejected.stderr);
  }
  assert.match(rejected.stderr, /invalid\.tsx:3: hardcoded UI text/);
  assert.doesNotMatch(rejected.stderr, /valid\.ts:/);
});
