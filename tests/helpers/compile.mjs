import fs from 'node:fs';
import { transformSync } from 'esbuild';

export function compileFixture(file) {
  return transformSync(fs.readFileSync(file, 'utf8'), {
    sourcefile: file,
    loader: file.endsWith('.tsx') ? 'tsx' : 'ts',
    format: 'cjs',
    jsx: 'automatic',
    target: 'es2022',
  }).code;
}
