import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
import { contextCopyText, contextSourceFooter, fullContextText } from '../src/workspace/contextExportText.ts';

const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
const labels = { sources: 'Sources', version: 'Version', captured: 'Captured', localSource: 'Local source', user: 'User input', agent: 'Agent capture', conversation: 'Conversation', discussion: 'Discussion input', imported: 'Imported file (authorship unknown)', userMessage: 'Written by user', assistantMessage: 'Written by AI', confirmedByUser: 'Saved with user confirmation', application: 'Application', file: 'File', importedAt: 'Imported', source: 'Source', attributionHelp: 'Saving AI output does not establish a user decision. Import time is not the writing or event date.' };
const selection = id => ({ memory_id: id, version_id: `v-${id}`, title: id });
const item = id => ({ ...selection(id), body: `Saved ${id}`, created_at: 10, sources: [{ capture_id: `raw-${id}`, recorded_at: 2, origin: { kind: 'user', uri: `https://example.com/${id}` } }] });
const snapshot = ids => ({ items: ids.map(item) });
const row = id => ({ key: { kind: 'memory', id }, title: id, snippet: `Saved ${id}`, updated_at: 10, origin: null });
function setup(t, options = {}) {
  const copied = [], f = workspaceFixture(t, { clipboard: { async writeText(text) { copied.push(text); } }, ...options });
  f.overrides.context_export_prepare = ({ selections }) => snapshot(selections.map(s => s.memory_id));
  f.overrides.context_export_validate = () => {};
  f.overrides.context_export_start = () => {};
  f.overrides.context_export_cancel = () => {};
  f.overrides.context_export_generate = () => 'A careful summary [S1].';
  const props = { collections: [], initialSelection: selection('a'), configured: true, onClose() {} };
  const component = f.load('src/workspace/ContextExport.tsx').default;
  const view = f.mount(component, props);
  const button = text => f.find(view, n => n.type === 'button' && f.text(n) === text);
  const click = async text => { button(text).props.onClick(); await f.settle(); };
  const editor = () => f.find(view, n => n.type === 'MarkdownEditor');
  return { ...f, view, props, component, button, click, editor, copied };
}

test('full export preserves saved bodies and appends readable version sources without local paths', () => {
  const value = snapshot(['a', 'b']);
  value.items[0].title = 'Line\n[title]';
  value.items[0].sources.push({ capture_id: 'local', recorded_at: 3, origin: { kind: 'user', uri: '/Users/private/secret.md' } });
  const text = fullContextText(value), footer = contextSourceFooter(value, labels, String);
  assert.match(text, /Saved a/); assert.match(text, /\[S2\] b/);
  assert.match(footer, /https:\/\/example.com\/a/); assert.match(footer, /Version: v-a/);
  assert(!footer.includes('/Users/private')); assert(!footer.includes('Line\n'));
  assert.equal(contextCopyText('edited', footer, 'Only 2 memories').startsWith('Only 2 memories\n\nedited'), true);
});

test('full copy requires no model and validates the pinned snapshot before copying edited preview', async t => {
  const f = setup(t); await f.settle();
  f.render(f.view, { ...f.props, configured: false });
  await f.click('Prepare selected memories');
  assert(f.text(f.view.tree).includes('Selected memories in full'));
  assert.equal(f.button('Summarize background').props.disabled, true);
  f.editor().props.onChange('User edited preview'); await f.settle();
  await f.click('Copy text and sources');
  assert.match(f.copied[0], /User edited preview/); assert.match(f.copied[0], /Sources/); assert.match(f.copied[0], /Based only on the selected saved memories \(1\)/);
  assert.equal(f.calls.filter(c => c.name === 'context_export_generate').length, 0);
  assert.equal(f.db.get('context_export').body, 'User edited preview');
  assert(f.calls.some(c => c.name === 'context_export_validate' && c.args.snapshot.items[0].version_id === 'v-a'));
});

test('source changes block copying and preserve edited preview for explicit replacement', async t => {
  const f = setup(t); await f.settle(); await f.click('Prepare selected memories');
  f.editor().props.onChange('Keep my revision'); await f.settle();
  f.overrides.context_export_validate = () => { throw { code: 'context_export_stale' }; };
  await f.click('Copy text and sources');
  assert.equal(f.copied.length, 0); assert.equal(f.editor().props.value, 'Keep my revision');
  assert(f.text(f.view.tree).includes('Your preview is preserved'));
  assert.equal(f.button('Copy text and sources').props.disabled, true);
  await f.click('Review selected memories');
  assert(f.text(f.view.tree).includes('replaces your edited preview'));
  f.overrides.context_export_validate = () => {};
  f.overrides.library_detail = ({ key }) => ({ key, state: 'active', title: key.id, current: { id: 'latest' } });
  await f.click('Replace preview with current memories');
  assert.equal(f.calls.filter(c => c.name === 'context_export_prepare').at(-1).args.selections[0].version_id, 'latest');
});

test('cancelled summaries and late start acknowledgements cannot overwrite the preview', async t => {
  const f = setup(t); await f.settle(); await f.click('Prepare selected memories');
  const start = deferred(); f.overrides.context_export_start = () => start.promise;
  await f.click('Summarize background'); await f.click('Cancel summarizing');
  f.editor().props.onChange('After cancellation'); await f.settle();
  start.resolve(); await f.settle();
  assert.equal(f.calls.filter(c => c.name === 'context_export_generate').length, 0);
  assert.equal(f.editor().props.value, 'After cancellation');
  const generated = deferred(); f.overrides.context_export_start = () => {}; f.overrides.context_export_generate = () => generated.promise;
  await f.click('Summarize background'); await f.click('Cancel summarizing');
  generated.resolve('Late result'); await f.settle();
  assert.equal(f.editor().props.value, 'After cancellation');
  assert(f.calls.filter(c => c.name === 'context_export_cancel').length >= 2);
});

test('closing flushes the edited draft and reopening resumes the same preview', async t => {
  const f = setup(t); let closed = 0; f.render(f.view, { ...f.props, onClose() { closed++; } });
  await f.settle(); await f.click('Prepare selected memories');
  f.editor().props.onChange('A recoverable 中文 draft');
  await f.click('Save preview and close');
  assert.equal(closed, 1); assert.equal(f.db.get('context_export').body, 'A recoverable 中文 draft');
  f.unmount(f.view);
  const reopened = f.mount(f.component, f.props); await f.settle();
  assert.equal(f.find(reopened, n => n.type === 'MarkdownEditor').props.value, 'A recoverable 中文 draft');
  assert.equal(f.calls.filter(c => c.name === 'context_export_prepare').length, 1);
});

test('selections persist across search and pagination without implicitly selecting a collection', async t => {
  const f = setup(t); f.render(f.view, { ...f.props, initialSelection: undefined, collectionId: 'collection-a' });
  f.overrides.library_query = ({ query }) => query.query ? { items: [row('match')], next_offset: null } : query.offset ? { items: [row('later')], next_offset: null } : { items: Array.from({ length: 40 }, (_, i) => row(`m${i}`)), next_offset: 40 };
  await f.settle();
  assert.equal(f.button('Prepare selected memories').props.disabled, true);
  f.find(f.view, n => n.type === 'input' && n.props.type === 'checkbox').props.onChange(); await f.settle();
  await f.click('Load more');
  const checkboxes = f.nodes(f.view.tree).filter(n => n.type === 'input' && n.props.type === 'checkbox');
  checkboxes.at(-1).props.onChange(); await f.settle();
  f.find(f.view, n => n.type === 'input' && n.props.type === 'search').props.onChange({ target: { value: 'query' } }); await f.settle();
  assert.match(f.text(f.view.tree), /Selected: 2 \/ 20/);
  await f.click('Prepare selected memories');
  assert.deepEqual(Array.from(f.calls.filter(c => c.name === 'context_export_prepare').at(-1).args.selections, s => s.memory_id), ['m0', 'later']);
});

test('late search replies do not replace a newer query result', async t => {
  const f = setup(t); const old = deferred();
  f.overrides.library_query = ({ query }) => query.query === 'old' ? old.promise : { items: [row(query.query || 'initial')], next_offset: null };
  await f.settle();
  const search = () => f.find(f.view, n => n.type === 'input' && n.props.type === 'search');
  search().props.onChange({ target: { value: 'old' } }); await f.settle();
  search().props.onChange({ target: { value: 'new' } }); await f.settle();
  old.resolve({ items: [row('wrong-result')], next_offset: null }); await f.settle();
  assert(f.text(f.view.tree).includes('new')); assert(!f.text(f.view.tree).includes('wrong-result'));
});

test('native copy uses the validated host clipboard command and summary keeps source labels', async t => {
  const f = setup(t, { native: true }); f.overrides.context_export_copy = () => {};
  await f.settle(); await f.click('Prepare selected memories'); await f.click('Summarize background');
  assert.equal(f.editor().props.value, 'A careful summary [S1].');
  await f.click('Copy text and sources');
  const copied = f.calls.find(c => c.name === 'context_export_copy');
  assert.match(copied.args.body, /A careful summary \[S1\]/); assert.match(copied.args.body, /Version: v-a/);
  assert.equal(f.copied.length, 0);
  assert(!f.calls.some(c => ['library_edit', 'discussion_submit', 'cleanup_save'].includes(c.name)));
});

test('selection limit is explicit and removing a selection enables another memory', async t => {
  const f = setup(t);
  const selections = Array.from({ length: 20 }, (_, index) => selection(`m${index}`));
  f.db.set('context_export', { key: 'context_export', request_id: 'seed', title: '', body: '', expected_version: null, context_export: { snapshot: null, mode: 'full', selections } });
  f.overrides.library_query = () => ({ items: [row('extra')], next_offset: null });
  await f.settle();
  assert.equal(f.find(f.view, n => n.type === 'input' && n.props.type === 'checkbox').props.disabled, true);
  f.find(f.view, n => n.type === 'button' && n.props['aria-label'] === 'Remove m0 from selection').props.onClick(); await f.settle();
  const extra = f.find(f.view, n => n.type === 'input' && n.props.type === 'checkbox');
  assert.equal(extra.props.disabled, false); extra.props.onChange(); await f.settle();
  await f.click('Prepare selected memories');
  const prepared = f.calls.find(c => c.name === 'context_export_prepare').args.selections;
  assert.equal(prepared.length, 20); assert(prepared.some(item => item.memory_id === 'extra')); assert(!prepared.some(item => item.memory_id === 'm0'));
});

test('unmount cancels pending generation and never writes its late result', async t => {
  const f = setup(t); await f.settle(); await f.click('Prepare selected memories');
  const result = deferred(); f.overrides.context_export_generate = () => result.promise;
  await f.click('Summarize background'); f.unmount(f.view);
  result.resolve('Do not persist this'); await f.settle();
  assert.equal(f.db.get('context_export').body, '## [S1] a\n\nSaved a');
  assert.equal(f.calls.filter(c => c.name === 'context_export_cancel').length, 1);
});

test('late background validation cannot clear a newer copy conflict', async t => {
  const f = setup(t); await f.settle();
  const validation = deferred(); f.overrides.context_export_validate = () => validation.promise;
  await f.click('Prepare selected memories');
  f.overrides.context_export_validate = () => { throw { code: 'context_export_stale' }; };
  await f.click('Copy text and sources'); validation.resolve(); await f.settle();
  assert.equal(f.button('Copy text and sources').props.disabled, true);
  assert(f.text(f.view.tree).includes('Your preview is preserved'));
});

test('opening another collection explicitly changes scope without replacing a saved preview', async t => {
  const f = setup(t); await f.settle(); await f.click('Prepare selected memories');
  f.editor().props.onChange('My previous preview'); await f.settle();
  f.unmount(f.view);
  const opened = f.mount(f.component, { ...f.props, initialSelection: undefined, collectionId: 'b', collections: [{ id: 'b', name: 'Collection B' }] });
  await f.settle();
  assert.equal(f.find(opened, n => n.type === 'MarkdownEditor').props.value, 'My previous preview');
  f.find(opened, n => n.type === 'button' && f.text(n) === 'Choose from Collection B').props.onClick(); await f.settle();
  assert.equal(f.db.get('context_export').body, 'My previous preview');
  assert.equal(f.db.get('context_export').context_export.collection_id, 'b');
  assert.equal(f.db.get('context_export').context_export.selections.length, 0);
  assert.equal(f.calls.filter(c => c.name === 'library_query').at(-1).args.query.collection_id, 'b');
  f.unmount(opened);
  const all = f.mount(f.component, { ...f.props, initialSelection: undefined }); await f.settle();
  f.find(all, n => n.type === 'button' && f.text(n) === 'Choose from All memories').props.onClick(); await f.settle();
  assert.equal(f.calls.filter(c => c.name === 'library_query').at(-1).args.query.collection_id, undefined);
  assert.equal(f.db.get('context_export').body, 'My previous preview');
});

test('source footer preserves distinct authorship and confirmation even with identical dates and links', () => {
  const value = snapshot(['a']);
  const first = value.items[0].sources[0];
  value.items[0].sources = [first, { ...first, capture_id: 'agent', origin: { ...first.origin, kind: 'agent', app: 'Outside AI' } }, { ...first, capture_id: 'conversation', origin: { kind: 'conversation', message_role: 'assistant', confirmed_by: 'user' } }];
  const footer = contextSourceFooter(value, labels, String);
  assert.match(footer, /Source 1: User input/);
  assert.match(footer, /Source 2: Agent capture · Application: Outside AI/);
  assert.match(footer, /Source 3: Conversation · Written by AI · Saved with user confirmation/);
  assert.match(footer, /Saving AI output does not establish a user decision/);
});

test('mixed imported sources retain unknown authorship, escaped filenames and import dates', () => {
  const value = snapshot(['a']);
  value.items[0].sources.push({ capture_id: 'imported', recorded_at: 7, origin: { kind: 'imported', file_name: 'Interview [notes].md', uri: '/Users/private/Interview [notes].md' } });
  const footer = contextSourceFooter(value, labels, String);
  assert.match(footer, /Source 1: User input/);
  assert(footer.includes('Source 2: Imported file (authorship unknown) · File: Interview \\[notes\\].md'));
  assert.match(footer, /Captured: 2/);
  assert.match(footer, /Imported: 7/);
  assert(!footer.includes('Captured: 7'));
  assert(!footer.includes('/Users/private'));
  assert.match(footer, /Import time is not the writing or event date/);
});

test('copying an imported memory preserves source attribution in both interface languages', async t => {
  const f = setup(t);
  f.overrides.context_export_prepare = () => {
    const value = snapshot(['a']);
    value.items[0].sources.push({ capture_id: 'imported', recorded_at: 7, origin: { kind: 'imported', file_name: '采访记录.md' } });
    return value;
  };
  await f.settle(); await f.click('Prepare selected memories');
  f.editor().props.onChange('Quoted material: 我正在考虑这个计划，还没有决定。'); await f.settle();
  await f.click('Copy text and sources');
  assert.match(f.copied[0], /Source 2: Imported file \(authorship unknown\) · File: 采访记录.md/);
  assert.match(f.copied[0], /Local source · Imported:/);
  assert.match(f.copied[0], /import time is not the writing or event date/);
  await f.language('zh-CN'); await f.click('复制正文和来源');
  assert.match(f.copied[1], /来源 2: 导入文件（作者身份未知） · 文件: 采访记录.md/);
  assert.match(f.copied[1], /本地来源 · 导入时间:/);
  assert.match(f.copied[1], /导入文件不代表由用户撰写或认同；导入时间不是写作或事件时间/);
  assert.equal(f.editor().props.value, 'Quoted material: 我正在考虑这个计划，还没有决定。');
  assert.equal(f.db.get('context_export').context_export.snapshot.items[0].sources[1].origin.kind, 'imported');
  assert.equal(f.calls.filter(call => call.name === 'context_export_generate').length, 0);
});

test('late background validation cannot clear a newer summary conflict', async t => {
  const f = setup(t); await f.settle();
  const validation = deferred(); f.overrides.context_export_validate = () => validation.promise;
  await f.click('Prepare selected memories');
  f.editor().props.onChange('Preserve this edited preview'); await f.settle();
  f.overrides.context_export_generate = () => { throw { code: 'context_export_stale' }; };
  await f.click('Summarize background'); validation.resolve(); await f.settle();
  assert.equal(f.button('Copy text and sources').props.disabled, true);
  assert.equal(f.button('Summarize background').props.disabled, true);
  assert.equal(f.editor().props.value, 'Preserve this edited preview');
  assert(f.text(f.view.tree).includes('Your preview is preserved'));
});
