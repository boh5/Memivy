import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function fixture({ native = true, failListen = false } = {}) {
  const pending = [], order = [], rendered = [];
  let listener;
  const module = { exports: {} };
  const code = ts.transpileModule(fs.readFileSync('src/i18n/preferences.ts', 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(code, {
    module, exports: module.exports, navigator: { languages: ['fr', 'zh-CN'] },
    require(name) {
      if (name === '../nativeIpc' || name === '@tauri-apps/api/core') return {
        isTauri: () => native,
        invoke: (name, args) => { order.push(name); return new Promise((resolve, reject) => pending.push({ name, args, resolve, reject })); },
      };
      if (name === '@tauri-apps/api/event') return { listen: async (_name, callback) => {
        order.push('listen');
        if (failListen) { failListen = false; throw Error('subscription failed'); }
        listener = callback; return () => {};
      } };
      if (name === './index') return { initializeI18n: async language => { rendered.push(language); } };
      if (name === './languages') return { resolveLanguage: () => 'zh-CN' };
      throw Error(name);
    },
  });
  return { api: module.exports, pending, order, rendered, emit: payload => listener({ payload }) };
}
const tick = () => new Promise(resolve => setImmediate(resolve));
const snapshot = (revision, language = 'en') => ({ revision, preference: language, language, error: null });

test('subscribe before read; later event wins over stale initial snapshot, with no business IPC', async () => {
  const f = fixture();
  const ready = f.api.startLanguage(); await tick();
  assert.deepEqual(f.order, ['listen', 'ui_language_snapshot']);
  f.emit(snapshot(2, 'zh-CN')); await tick();
  f.pending.shift().resolve(snapshot(1)); await ready;
  assert.equal(f.api.getLanguageSnapshot().language, 'zh-CN');
  assert.equal(f.rendered.at(-1), 'zh-CN');
  assert.equal(f.order.length, 2);
});

test('failed persistence preserves the accepted choice and successful retry updates it', async () => {
  const f = fixture(); const ready = f.api.startLanguage(); await tick();
  f.pending.shift().resolve(snapshot(1)); await ready;
  const failed = f.api.setLanguagePreference('zh-CN');
  f.pending.shift().reject({ code: 'language_save_failed' });
  await assert.rejects(failed);
  assert.equal(f.api.getLanguageSnapshot().language, 'en');
  const retry = f.api.setLanguagePreference('zh-CN');
  f.pending.shift().resolve(snapshot(2, 'zh-CN')); await retry;
  assert.equal(f.api.getLanguageSnapshot().language, 'zh-CN');
});

test('listener failure presents usable fallback and retry installs a live listener without a write', async () => {
  const f = fixture({ failListen: true }); await f.api.startLanguage();
  assert.equal(f.api.getLanguageSnapshot().error, 'preferences_unavailable');
  assert.equal(f.rendered.at(-1), 'en');
  const retry = f.api.refreshLanguage(); await tick();
  f.pending.shift().resolve(snapshot(1)); await retry;
  f.emit(snapshot(2, 'zh-CN')); await tick();
  assert.equal(f.api.getLanguageSnapshot().language, 'zh-CN');
  assert.deepEqual(f.order, ['listen', 'listen', 'ui_language_snapshot']);
});

test('browser preview initializes and switches without native IPC', async () => {
  const f = fixture({ native: false }); await f.api.startLanguage();
  assert.equal(f.api.getLanguageSnapshot().language, 'zh-CN');
  await f.api.setLanguagePreference('en');
  assert.equal(f.api.getLanguageSnapshot().language, 'en');
  assert.deepEqual(f.order, []);
});
