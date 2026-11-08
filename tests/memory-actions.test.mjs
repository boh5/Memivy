import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';

for (const language of ['en', 'zh-CN']) test(`library copy and import actions preserve their scope in ${language}`, async t => {
  const f = workspaceFixture(t);
  const App = f.load('src/workspace/App.tsx').default;
  const List = f.load('src/workspace/MemoryList.tsx').default;
  const Sidebar = f.load('src/workspace/WorkspaceSidebar.tsx').default;
  const Importer = f.load('src/workspace/ImportMemories.tsx').default;
  const Exporter = f.load('src/workspace/ContextExport.tsx').default;
  f.overrides.navigation_collections = () => [{ id: 'scope', name: 'Reference', revision: 1, count: 1 }];
  const app = f.mount(App); await f.settle(); await f.language(language);
  const copyLabel = language === 'en' ? 'Copy for AI' : '复制给 AI';
  const importLabel = language === 'en' ? 'Import' : '导入';
  const list = f.mount(List, f.find(app, node => node.type === List).props); await f.settle();
  const click = async label => { f.find(list, node => node.type === 'button' && f.text(node) === label).props.onClick(); await f.settle(); };
  await click(importLabel);
  assert.equal(f.find(app, node => node.type === Importer).props.open, true);
  assert(!f.nodes(app.tree).some(node => node.type === Exporter));
  f.find(app, node => node.type === Importer).props.onClose(); await f.settle();
  await click(copyLabel);
  assert.equal(f.find(app, node => node.type === Exporter).props.collectionId, undefined);
  assert.equal(f.find(app, node => node.type === Importer).props.open, false);
  f.find(app, node => node.type === Exporter).props.onClose(); await f.settle();

  f.find(app, node => node.type === Sidebar).props.onCollection('scope'); await f.settle();
  const scoped = f.find(app, node => node.type === List).props;
  assert.equal(scoped.onImport, undefined);
  f.render(list, scoped); await f.settle();
  assert(!f.nodes(list.tree).some(node => node.type === 'button' && f.text(node) === importLabel));
  await click(copyLabel);
  assert.equal(f.find(app, node => node.type === Exporter).props.collectionId, 'scope');
  f.find(app, node => node.type === Exporter).props.onClose(); await f.settle();

  f.find(app, node => node.type === Sidebar).props.onTrash(); await f.settle();
  f.render(list, f.find(app, node => node.type === List).props); await f.settle();
  assert(!f.nodes(list.tree).some(node => node.type === 'button' && [copyLabel, importLabel].includes(f.text(node))));
});

test('library keyword search waits for IME, retains filters, and opens the matching result', async t => {
  let timer;
  const f = workspaceFixture(t, { timers: {
    setTimeout(fn) { timer = fn; return fn; },
    clearTimeout(fn) { if (timer === fn) timer = undefined; },
  } });
  const List = f.load('src/workspace/MemoryList.tsx').default;
  const { Highlight } = f.load('src/workspace/components.tsx');
  const selections = [];
  f.overrides.library_query = ({ query }) => ({ items: [{
    key: f.keyA, title: query.query || 'Original result', snippet: '中文匹配', updated_at: 1, origin: null,
  }], next_offset: null });
  const view = f.mount(List, { collectionId: 'scope', trash: false, active: true, selected: null,
    onSelect: (...args) => selections.push(args), onCapture() {}, onRefresh() {},
  });
  await f.settle();
  f.find(view, node => node.props['aria-expanded'] === false).props.onClick(); await f.settle();
  f.find(view, node => node.type === 'select' && node.props['aria-label'] === 'Filter by source').props.onChange({ target: { value: 'user' } });
  await f.settle();
  const search = () => f.find(view, node => node.type === 'input' && node.props.type === 'search').props;
  search().onCompositionStart(); search().onChange({ target: { value: '中文' } }); await f.settle();
  assert.equal(timer, undefined);
  assert(f.text(view.tree).includes('Original result'));
  search().onCompositionEnd(); await f.settle();
  assert(!f.text(view.tree).includes('Original result'));
  timer(); await f.settle();
  const query = f.calls.filter(call => call.name === 'library_query').at(-1).args.query;
  assert.equal(query.query, '中文'); assert.equal(query.keyword_only, true);
  assert.equal(query.collection_id, 'scope'); assert.equal(query.origin, 'user');
  const result = f.find(view, node => node.props['data-record'] === 'memory:a');
  assert(f.nodes(result).some(node => node.type === Highlight && node.props.query === '中文'));
  result.props.onClick(); assert.deepEqual(selections, [[f.keyA, '中文']]);
});

test('a stale edit draft is rebased only onto the version the user reviewed', async t => {
  const f = workspaceFixture(t);
  const Detail = f.load('src/workspace/MemoryDetail.tsx').default;
  const detail = version => ({ key: f.keyA, state: 'active', title: 'Current title', body: 'Current body', current: { id: version }, sources: [], history: [] });
  f.overrides.library_detail = () => detail('saved-first');
  f.overrides.library_edit = () => ({ request_id: 'result' });
  const key = 'memory:a';
  f.db.set(key, { key, request_id: 'stale', title: 'Draft title', body: 'Draft body', expected_version: 'older' });
  const view = f.mount(Detail, { record: f.keyA, query: '', initialReceipt: null, onChanged() {}, onBack() {}, onDiscuss() {}, onOpenDiscussion() {} });
  await f.settle();
  f.find(view, node => node.props.className === 'detail-edit-action').props.onClick(); await f.settle();
  const editorNode = f.find(view, node => node.type.name === 'Editor');
  const editor = f.mount(editorNode.type, editorNode.props); await f.settle();
  const button = label => f.find(editor, node => node.type === 'button' && f.text(node) === label);
  assert.equal(button('Save').props.disabled, true);
  button('View latest content').props.onClick(); await f.settle();
  f.render(editor, { ...editorNode.props, detail: detail('saved-later') }); await f.settle();
  assert(!f.nodes(editor.tree).some(node => node.type === 'button' && f.text(node) === 'Continue editing my draft'));
  const modal = f.find(editor, node => node.props.className === 'edit-conflict-dialog');
  f.nodes(modal).find(node => node.type === 'button' && f.text(node) === 'View latest content').props.onClick(); await f.settle();
  button('Continue editing my draft').props.onClick(); await f.settle();
  assert.equal(f.db.get(key).expected_version, 'saved-later');
  assert.equal(f.db.get(key).body, 'Draft body');
  assert.equal(button('Save').props.disabled, false);
  button('Save').props.onClick(); await f.settle();
  assert.equal(f.calls.find(call => call.name === 'library_edit').args.draft.expected_version, 'saved-later');
});

test('moving a memory to Trash shows a scoped undo and reports restoration', async t => {
  const messages = [];
  const f = workspaceFixture(t, { modules: { './Toast': { notify: (...args) => messages.push(args) } } });
  const Detail = f.load('src/workspace/MemoryDetail.tsx').default;
  f.overrides.library_action = ({ action }) => action.action === 'trash' ? { revision: 15 } : null;
  let backs = 0;
  const view = f.mount(Detail, { record: f.keyA, query: '', initialReceipt: null, onChanged() {}, onBack() { backs++; }, onDiscuss() {}, onOpenDiscussion() {} });
  await f.settle();
  const move = () => f.nodes(view.tree).filter(node => node.type === 'button' && f.text(node) === 'Move to trash').at(-1);
  move().props.onClick(); await f.settle(); move().props.onClick(); await f.settle();
  assert.equal(backs, 1);
  assert.equal(messages[0][0].key, 'detail.trashedNotice');
  messages[0][2](); await f.settle();
  const undo = f.calls.filter(call => call.name === 'library_action').at(-1).args.action;
  assert.equal(undo.action, 'undo_trash'); assert.equal(undo.key.id, f.keyA.id);
  assert.equal(undo.version, 'v-a'); assert.equal(undo.revision, 15);
  assert.equal(messages.at(-1)[0].key, 'detail.restoredNotice');
});
