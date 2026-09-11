import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { withRustPath } from '../scripts/rust-env.mjs';

test('native commands find rustup without shell startup files', () => {
  const env = { PATH: '/usr/bin', KEEP: 'value' };
  const result = withRustPath(env, '/home/test');
  assert.equal(result.PATH, ['/usr/bin', path.join('/home/test', '.cargo/bin')].join(path.delimiter));
  assert.equal(result.KEEP, 'value');
  assert.equal(env.PATH, '/usr/bin');
});

test('preserve custom Cargo installations, PATH priority and missing PATH', () => {
  const bin = path.join('/custom cargo', 'bin');
  assert.equal(withRustPath({ CARGO_HOME: '/custom cargo' }).PATH, bin);
  const env = { CARGO_HOME: '/custom cargo', PATH: ['/chosen/bin', bin].join(path.delimiter) };
  assert.equal(withRustPath(env).PATH, env.PATH);
});
