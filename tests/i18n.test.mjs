import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolveLanguage } from '../src/i18n/languages.ts';
import { message, renderMessage } from '../src/i18n/messages.ts';
import { createInstance } from 'i18next';
import { JSDOM } from 'jsdom';
import { createServer } from 'vite';
import React from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';

test('ordered language preferences match the native shared contract', async () => {
  const cases = JSON.parse(await readFile(new URL('./fixtures/languages.json', import.meta.url)));
  for (const { preferences, language } of cases) {
    assert.equal(resolveLanguage(preferences), language, JSON.stringify(preferences));
  }
});

test('real React notice switches language while preserving focused input and its draft', async () => {
  const server = await createServer({ server: { middlewareMode: true, watch: null }, logLevel: 'silent' });
  const dom = new JSDOM('<!doctype html><div id="root"></div>');
  const previous = { window: globalThis.window, document: globalThis.document };
  globalThis.window = dom.window; globalThis.document = dom.window.document;
  // React was imported before jsdom and selects its legacy input-event adapter.
  dom.window.HTMLElement.prototype.attachEvent = () => {};
  dom.window.HTMLElement.prototype.detachEvent = () => {};
  let root;
  try {
    const { initializeI18n } = await server.ssrLoadModule('/src/i18n/index.ts');
    const { useNotice } = await server.ssrLoadModule('/src/i18n/react.ts');
    await initializeI18n('en');
    function View() {
      const [notice] = useNotice(message('common', 'retry'));
      return React.createElement('section', null,
        React.createElement('input', { defaultValue: 'Unsubmitted draft' }),
        React.createElement('p', null, notice));
    }
    root = createRoot(document.getElementById('root'));
    flushSync(() => root.render(React.createElement(View)));
    const input = document.querySelector('input');
    input.focus(); input.setSelectionRange(2, 7);
    assert.equal(document.querySelector('p').textContent, 'Retry');
    await initializeI18n('zh-CN');
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(document.querySelector('p').textContent, '重试');
    assert.equal(document.querySelector('input'), input);
    assert.equal(document.activeElement, input);
    assert.equal(input.value, 'Unsubmitted draft');
    assert.equal(input.selectionStart, 2); assert.equal(input.selectionEnd, 7);
  } finally {
    if (root) flushSync(() => root.unmount());
    await new Promise(resolve => setTimeout(resolve, 20));
    Object.assign(globalThis, previous); dom.window.close(); await server.close();
  }
});

test('retained semantic notices translate again, including nested failures and counts', async () => {
  const i18n = createInstance();
  await i18n.init({
    lng: 'en', fallbackLng: 'en', interpolation: { escapeValue: false },
    resources: {
      en: { errors: { failed: 'Failed: {{error}}', busy: 'Try again' }, common: { items_one: '{{count}} item', items_other: '{{count}} items' } },
      'zh-CN': { errors: { failed: '失败：{{error}}', busy: '请重试' }, common: { items_other: '{{count}} 项' } },
    },
  });
  const notice = message('errors', 'failed', { error: message('errors', 'busy') });
  const render = value => renderMessage(value, (key, options) => i18n.t(key, options));
  assert.equal(render(notice), 'Failed: Try again');
  assert.equal(render(message('common', 'items', { count: 1 })), '1 item');
  assert.equal(render(message('common', 'items', { count: 0 })), '0 items');
  await i18n.changeLanguage('zh-CN');
  assert.equal(render(notice), '失败：请重试');
  assert.equal(render(message('common', 'items', { count: 2 })), '2 项');
  assert.equal(render('Original user content'), 'Original user content');
});

test('independent formatting root and existing plugin DOM update without replacing user input', async () => {
  const server = await createServer({ server: { middlewareMode: true, watch: null }, logLevel: 'silent' });
  const dom = new JSDOM('<!doctype html><div id="toolbar"></div><div id="editor"><div class="milkdown-link-edit"><input value="https://example.org/draft"></div></div>');
  const previous = { window: globalThis.window, document: globalThis.document };
  globalThis.window = dom.window; globalThis.document = dom.window.document;
  let root;
  try {
    const { initializeI18n } = await server.ssrLoadModule('/src/i18n/index.ts');
    const { default: Toolbar } = await server.ssrLoadModule('/src/workspace/FormattingToolbar.tsx');
    const { tableFeatureConfig, localizeEditorDom } = await server.ssrLoadModule('/src/workspace/editorExtensions.ts');
    await initializeI18n('en');
    root = createRoot(document.getElementById('toolbar'));
    flushSync(() => root.render(React.createElement(Toolbar, { state: { block: 'paragraph' }, onFormat() {} })));
    const host = document.getElementById('editor');
    host.insertAdjacentHTML('beforeend', tableFeatureConfig.addRowIcon);
    const icon = host.querySelector('svg');
    const input = host.querySelector('input');
    input.setSelectionRange(3, 8);
    assert.equal(icon.getAttribute('aria-label'), 'Add row');
    assert.ok(document.querySelector('button[aria-label="Bold"]'));
    await initializeI18n('zh-CN'); localizeEditorDom(host);
    await new Promise(resolve => setTimeout(resolve, 20));
    assert.equal(host.querySelector('svg'), icon);
    assert.equal(icon.getAttribute('aria-label'), '新增行');
    assert.equal(icon.querySelector('title').textContent, '新增行');
    assert.ok(document.querySelector('button[aria-label="加粗"]'));
    assert.equal(input.placeholder, '粘贴链接地址');
    assert.equal(input.value, 'https://example.org/draft');
    assert.equal(input.selectionStart, 3); assert.equal(input.selectionEnd, 8);
  } finally {
    if (root) flushSync(() => root.unmount());
    await new Promise(resolve => setTimeout(resolve, 20));
    Object.assign(globalThis, previous); dom.window.close(); await server.close();
  }
});

test('production translation boundary resolves plural counts and unknown keys safely', async () => {
  const server = await createServer({ server: { middlewareMode: true, watch: null }, logLevel: 'silent' });
  const dom = new JSDOM('<!doctype html>');
  const previous = globalThis.document;
  globalThis.document = dom.window.document;
  try {
    const { default: i18n, initializeI18n, translateCatalog } = await server.ssrLoadModule('/src/i18n/index.ts');
    await initializeI18n('en');
    i18n.addResourceBundle('en', 'common', { countTest_one: '{{count}} item', countTest_other: '{{count}} items' });
    assert.equal(translateCatalog('countTest', { ns: 'common', count: 1 }), '1 item');
    assert.equal(translateCatalog('countTest', { ns: 'common', count: 0 }), '0 items');
    assert.equal(translateCatalog('countTest', { ns: 'common', count: 5 }), '5 items');
    assert.equal(translateCatalog('missing-private-error', { ns: 'errors' }), i18n.t('operation_failed', { ns: 'errors' }));
  } finally {
    globalThis.document = previous; dom.window.close(); await server.close();
  }
});
