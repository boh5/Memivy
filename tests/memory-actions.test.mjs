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
