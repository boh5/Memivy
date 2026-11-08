import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';

test('the collection picker adds an explicit local search result to the open collection', async t => {
  const f = workspaceFixture(t, { timers: { setTimeout: callback => setTimeout(callback, 0), clearTimeout } });
  const Picker = f.load('src/workspace/CollectionPicker.tsx').default;
  const collection = { id: 'scope', name: 'A collection' };
  const row = { key: f.keyA, title: 'Orchid note', snippet: 'A synthetic orchid', origin: null, updated_at: 1 };
  let added = false;
  f.overrides.library_query = async ({ query }) => {
    assert.equal(query.exclude_collection_id, collection.id);
    assert.equal(query.keyword_only, true);
    if (query.query === 'unavailable') throw 'Could not search';
    return { items: added ? [] : [row], next_offset: null };
  };
  f.overrides.navigation_collect = async ({ collection: id, key, included }) => {
    assert.equal(id, collection.id);
    assert.equal(key.id, row.key.id);
    assert.equal(included, true);
    added = true;
  };
  const view = f.mount(Picker, { collection, onClose() {} });
  await f.settle();
  const setQuery = value => f.find(view, n => n.type === 'input').props.onChange({ target: { value } });
  const canAdd = () => f.nodes(view.tree).some(n => n.props['aria-label'] === 'Add Orchid note to the collection');
  setQuery('unavailable'); await f.settle();
  assert.equal(canAdd(), false);
  assert(f.nodes(view.tree).some(n => n.props.text === 'Could not search'));
  setQuery('orchid'); await f.settle();
  assert.equal(canAdd(), true);
  assert(!f.nodes(view.tree).some(n => n.props.text === 'Could not search'));
  assert(!added);
  f.find(view, n => n.props['aria-label'] === 'Add Orchid note to the collection').props.onClick({ currentTarget: {} });
  await f.settle();
  assert(added);
  assert.equal(canAdd(), false);
  assert(f.text(view.tree).includes('Added “Orchid note” to the collection.'));
});

test('the collection picker preserves the selected memory and old discussions return to an active list', async t => {
  const f = workspaceFixture(t);
  const App = f.load('src/workspace/App.tsx').default;
  const Sidebar = f.load('src/workspace/WorkspaceSidebar.tsx').default;
  const List = f.load('src/workspace/MemoryList.tsx').default;
  const Picker = f.load('src/workspace/CollectionPicker.tsx').default;
  const Detail = f.load('src/workspace/MemoryDetail.tsx').default;
  const active = { id: 'active', name: 'Active collection', revision: 1, count: 1 };
  f.overrides.navigation_collections = async () => [active];
  const view = f.mount(App); await f.settle();
  const sidebar = () => f.find(view, n => n.type === Sidebar).props;
  const list = () => f.find(view, n => n.type === List).props;
  sidebar().onCollection(active.id); await f.settle();
  list().onSelect(f.keyA, 'orchid'); await f.settle();
  f.find(view, n => n.type === 'button' && f.text(n) === 'Add memories').props.onClick(); await f.settle();
  assert.equal(f.find(view, n => n.type === Picker).props.collection.id, active.id);
  assert.equal(f.find(view, n => n.type === Detail).props.query, 'orchid');
  f.find(view, n => n.type === Picker).props.onClose(); await f.settle();
  const closeOldDetail = f.find(view, n => n.type === Detail).props.onBack;
  list().onSelect(f.keyB, 'another'); await f.settle();
  closeOldDetail(); await f.settle();
  assert.equal(f.find(view, n => n.type === Detail).props.record, f.keyB);
  for (const [collection_id, expected] of [['archived', undefined], ['active', 'active']]) {
    sidebar().onTopic({ ...f.topic, collection_id }); await f.settle();
    assert.equal(list().collectionId, expected);
    f.find(view, n => n.type === 'button' && f.text(n) === 'Back to memories').props.onClick(); await f.settle();
    assert.equal(list().collectionId, expected);
    assert.equal(list().selected, null);
  }
});

test('collection paging recovers from a failed page and does not search during IME composition', async t => {
  const f = workspaceFixture(t, { timers: { setTimeout: callback => setTimeout(callback, 0), clearTimeout } });
  const Picker = f.load('src/workspace/CollectionPicker.tsx').default;
  f.overrides.library_query = async ({ query }) => {
    if (query.offset === 30) throw 'Page unavailable';
    return { items: [{ key: f.keyA, title: 'Orchid note' }], next_offset: 30 };
  };
  const view = f.mount(Picker, { collection: { id: 'scope', name: 'A collection' }, onClose() {} });
  await f.settle();
  const control = label => f.find(view, n => n.type === 'button' && f.text(n) === label);
  control('Next').props.onClick(); await f.settle();
  assert(f.nodes(view.tree).some(n => n.props.text === 'Page unavailable'));
  assert.equal(control('Previous').props['aria-disabled'], false);
  control('Previous').props.onClick(); await f.settle();
  assert(f.text(view.tree).includes('Orchid note'));

  const search = () => f.find(view, n => n.type === 'input').props;
  const before = f.calls.length;
  search().onCompositionStart();
  search().onChange({ target: { value: 'qingci' } }); await f.settle();
  assert.equal(f.calls.length, before);
  search().onCompositionEnd({ currentTarget: { value: '青瓷' } }); await f.settle();
  assert.equal(f.calls.at(-1).args.query.query, '青瓷');
});
