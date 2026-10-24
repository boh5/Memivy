import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
import { importedSources, importOrganizationInstruction } from '../src/workspace/importMemoryBatch.ts';

const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; };
const prepared = () => ({ id: 'batch-a', files: [{ id: 'file-a', file_name: 'Ideas.md', bytes: 100, error: null }, { id: 'file-b', file_name: 'Notes.txt', bytes: 200, error: null }, { id: 'file-bad', file_name: 'Binary.txt', bytes: 50, error: 'import_file_binary' }] });
const result = () => ({ request_id: 'batch-a', items: [{ file_name: 'Ideas.md', status: 'imported', memory_id: 'a', version_id: 'version-a' }, { file_name: 'Notes.txt', status: 'duplicate', memory_id: 'b', version_id: 'version-b' }], receipt: { request_id: 'batch-a', action: 'import', status: 'applied' } });
function setup(t, extra = {}) {
  const f = workspaceFixture(t), events = [];
  f.overrides.import_prepare = () => prepared(); f.overrides.import_apply = () => result(); f.overrides.import_discard = () => {};
  const props = { open: true, configured: true, onClose: () => events.push('closed'), onChanged: () => events.push('changed'), onOpenRecord: key => events.push(['record', key]), onOrganize: async request => events.push(['organize', request]), onOpenDiscussion: async id => events.push(['discussion', id]), ...extra };
  const component = f.load('src/workspace/ImportMemories.tsx').default, view = f.mount(component, props);
  const button = label => f.find(view, node => node.type === 'button' && f.text(node) === label);
  const click = async label => { button(label).props.onClick(); await f.settle(); };
  return { ...f, view, props, component, events, button, click };
}

test('chooser previews valid selections and invalid rows without importing or invoking AI', async t => {
  const f = setup(t); await f.click('Choose files');
  const boxes = f.nodes(f.view.tree).filter(node => node.type === 'input');
  assert.equal(boxes[0].props.checked, true); assert.equal(boxes[1].props.checked, true);
  assert.equal(boxes[2].props.checked, false); assert.equal(boxes[2].props.disabled, true);
  assert(f.text(f.view.tree).includes('binary content'));
  assert.equal(f.calls.filter(call => call.name === 'import_apply').length, 0);
  boxes[1].props.onChange({ target: { checked: false } }); await f.settle();
  await f.click('Import selected files');
  const applied = f.calls.find(call => call.name === 'import_apply');
  assert.equal(applied.args.id, 'batch-a'); assert.deepEqual(Array.from(applied.args.fileIds), ['file-a']);
  assert(f.text(f.view.tree).includes('The imported memories are saved and searchable; originals are preserved.'));
  assert.equal(f.events.filter(event => Array.isArray(event) && event[0] === 'organize').length, 0);
});

test('cancelled chooser does not create a batch or discard an unrelated request', async t => {
  const f = setup(t); f.overrides.import_prepare = () => null;
  await f.click('Choose files'); await f.click('Close');
  assert.equal(f.calls.filter(call => call.name === 'import_apply' || call.name === 'import_discard').length, 0);
  assert.deepEqual(f.events, ['closed']);
});

test('close awaits preview discard and a failed discard leaves the preview recoverable', async t => {
  const f = setup(t); await f.click('Choose files'); const discarding = deferred();
  f.overrides.import_discard = () => discarding.promise;
  await f.click('Close'); assert(!f.events.includes('closed'));
  discarding.reject({ code: 'import_busy' }); await f.settle();
  assert(f.text(f.view.tree).includes('Ideas.md')); assert(!f.events.includes('closed'));
  f.overrides.import_discard = () => {}; await f.click('Close');
  assert(f.events.includes('closed'));
});

test('a new chooser waits until the previous staged preview is discarded', async t => {
  const f = setup(t); await f.click('Choose files'); const discarding = deferred();
  f.overrides.import_discard = () => discarding.promise;
  await f.click('Choose different files');
  assert.equal(f.calls.filter(call => call.name === 'import_prepare').length, 1);
  discarding.resolve(); await f.settle();
  assert.equal(f.calls.filter(call => call.name === 'import_prepare').length, 2);
});

test('unconfirmed import retries the identical batch and prevents selection changes and double submission', async t => {
  const f = setup(t); await f.click('Choose files'); const first = deferred();
  f.overrides.import_apply = () => first.promise;
  const confirm = f.button('Import selected files'); confirm.props.onClick(); confirm.props.onClick(); await f.settle();
  assert.equal(f.calls.filter(call => call.name === 'import_apply').length, 1);
  first.reject({ code: 'operation_failed' }); await f.settle();
  assert(f.nodes(f.view.tree).filter(node => node.type === 'input').every(node => node.props.disabled));
  assert.equal(f.button('Choose different files').props.disabled, true);
  f.overrides.import_apply = () => result(); await f.click('Retry this import');
  const attempts = f.calls.filter(call => call.name === 'import_apply');
  assert.equal(JSON.stringify(attempts[0].args), JSON.stringify(attempts[1].args));
});

test('all-duplicate imports show existing memories and never offer to organize or undo a new batch', async t => {
  const f = setup(t); f.overrides.import_apply = () => ({ ...result(), items: result().items.map(item => ({ ...item, status: 'duplicate' })), receipt: null });
  await f.click('Choose files'); await f.click('Import selected files');
  assert(f.text(f.view.tree).includes('No new memories were created'));
  assert(!f.nodes(f.view.tree).some(node => node.type === 'button' && ['Organize this batch', 'Undo this import'].includes(f.text(node))));
  f.find(f.view, node => node.type === 'button' && f.text(node) === 'View memory').props.onClick(); await f.settle();
  assert.equal(f.events.find(event => Array.isArray(event) && event[0] === 'record')[1].id, 'a');
});

test('organization is explicit, only includes newly imported versions, and keeps one request across retries', async t => {
  let fail = true, requests = [];
  const f = setup(t, { onOrganize: async request => { requests.push(request); if (fail) throw { code: 'discussion_unavailable' }; } });
  await f.click('Choose files'); await f.click('Import selected files');
  await f.language('zh-CN'); await f.click('整理这批记忆');
  assert.deepEqual(Array.from(requests[0].context, source => source.id), ['version-a']);
  assert.match(requests[0].text, /exact memory IDs: a\./); assert.match(requests[0].text, /stop and report it rather than substituting/); assert.match(requests[0].text, /Reply in Simplified Chinese/); assert.match(requests[0].text, /do not assume I wrote or endorsed/);
  assert(!f.nodes(f.view.tree).some(node => node.type === 'button' && f.text(node) === '撤销本次导入'));
  fail = false; await f.language('en'); await f.click('Retry opening organization');
  assert.equal(requests[1].id, requests[0].id); assert.equal(requests[1].topicId, requests[0].topicId);
  assert.equal(requests[1].text, requests[0].text); assert.equal(requests[1].title, requests[0].title);
  assert(f.text(f.view.tree).includes('Organization was started in a discussion'));
  assert(f.text(f.view.tree).includes('The imported memories are saved and searchable; originals are preserved.'));
  assert(!f.text(f.view.tree).includes('not organized this batch yet'));
  await f.click('Open organization discussion');
  assert.equal(requests.length, 2); assert(f.events.some(event => Array.isArray(event) && event[0] === 'discussion'));
});

test('import undo is stable across failed acknowledgements and disables organization after success', async t => {
  const f = setup(t); await f.click('Choose files'); await f.click('Import selected files');
  let fail = true; f.overrides.library_action = () => { if (fail) throw { code: 'conflict' }; return { action: 'undo' }; };
  await f.click('Undo this import'); fail = false; await f.click('Undo this import');
  const attempts = f.calls.filter(call => call.name === 'library_action');
  assert.equal(attempts[0].args.action.original_request, 'batch-a');
  assert.equal(attempts[1].args.action.request_id, attempts[0].args.action.request_id);
  assert(f.text(f.view.tree).includes('Import undone'));
  assert(!f.nodes(f.view.tree).some(node => node.type === 'button' && f.text(node) === 'Organize this batch'));
});

test('late chooser and import completion clean only their own staging without updating an unmounted view', async t => {
  const f = setup(t), chosen = deferred(); f.overrides.import_prepare = () => chosen.promise;
  await f.click('Choose files'); f.unmount(f.view); chosen.resolve(prepared()); await f.settle();
  assert.equal(f.calls.filter(call => call.name === 'import_discard')[0].args.id, 'batch-a');
  const other = setup(t), importing = deferred(); await other.click('Choose files');
  other.overrides.import_apply = () => importing.promise; await other.click('Import selected files'); other.unmount(other.view);
  assert.equal(other.calls.filter(call => call.name === 'import_discard').length, 0);
  importing.resolve(result()); await other.settle();
  assert.equal(other.calls.filter(call => call.name === 'import_discard').length, 1);
  assert(!other.events.includes('changed'));
});

test('closing and reopening retains the latest saved result for organizing after model setup', async t => {
  const f = setup(t, { configured: false }); await f.click('Choose files'); await f.click('Import selected files');
  assert.equal(f.button('Organize this batch').props.disabled, true);
  await f.click('Close'); f.render(f.view, { ...f.props, open: false }); await f.settle();
  f.render(f.view, { ...f.props, open: true, configured: true }); await f.settle();
  assert(f.text(f.view.tree).includes('Imported 1 · Already imported 1'));
  assert.equal(f.button('Organize this batch').props.disabled, false);
  assert.equal(f.calls.filter(call => call.name === 'import_apply').length, 1);
});

test('organization instruction retains attribution and scope limits in both UI languages', () => {
  assert.deepEqual(importedSources(result()), [{ kind: 'version', id: 'version-a' }]);
  assert.match(importOrganizationInstruction('en'), /Reply in English/);
  assert.match(importOrganizationInstruction('zh-CN'), /Reply in Simplified Chinese/);
  assert.match(importOrganizationInstruction('en'), /do not claim the whole batch is organized/);
  assert.match(importOrganizationInstruction('en'), /Do not add collection membership/);
  assert.match(importOrganizationInstruction('zh-CN'), /Preserve each target memory's existing language/);
  assert.match(importOrganizationInstruction('zh-CN'), /response language instruction applies only to your discussion reply/);
});

test('replayed import with an undone receipt cannot be organized or undone again', async t => {
  const f = setup(t); f.overrides.import_apply = () => ({ ...result(), receipt: { ...result().receipt, status: 'undone' } });
  await f.click('Choose files'); await f.click('Import selected files');
  assert(f.text(f.view.tree).includes('Import undone'));
  assert(!f.text(f.view.tree).includes('Trash'));
  assert(!f.nodes(f.view.tree).some(node => node.type === 'button' && ['Organize this batch', 'Undo this import'].includes(f.text(node))));
});

test('main app flushes voice and drafts before opening and submitting the organization discussion', async t => {
  const ordered = [];
  const f = workspaceFixture(t, { modules: { './useVoice': { finishVoiceInputs: async () => { ordered.push('voice'); } }, './useDraft': { flushDrafts: async () => { ordered.push('drafts'); }, refreshDrafts: async () => {} } } });
  f.overrides.discussion_open = args => { ordered.push('open'); return { id: args.id, title: 'Imported materials' }; };
  f.overrides.discussion_submit = args => { ordered.push('submit'); return { id: args.topicId, title: 'Imported materials' }; };
  const view = f.mount(f.load('src/workspace/App.tsx').default); await f.settle();
  const importer = f.find(view, node => node.type === f.load('src/workspace/ImportMemories.tsx').default);
  const request = { id: 'input-a', topicId: 'topic-a', title: 'Organize imported memories', text: importOrganizationInstruction('en', ['a']), context: [{ kind: 'version', id: 'v-a' }], memories: [{ memory_id: 'a', version_id: 'v-a' }] };
  await importer.props.onOrganize(request); await f.settle();
  assert.deepEqual(ordered, ['voice', 'drafts', 'open', 'submit']);
  const sent = f.calls.find(call => call.name === 'discussion_submit').args;
  assert.equal(sent.id, 'input-a'); assert.equal(sent.topicId, 'topic-a');
  assert.equal(sent.collectionId, null); assert.equal(sent.quick, false);
  assert.equal(sent.context[0].id, 'v-a');
});

test('organization preflight blocks changed or deleted imports but never rechecks an already dispatched request', async t => {
  const f = workspaceFixture(t, { modules: { './useVoice': { finishVoiceInputs: async () => {} }, './useDraft': { flushDrafts: async () => {}, refreshDrafts: async () => {} } } });
  const view = f.mount(f.load('src/workspace/App.tsx').default); await f.settle();
  const importer = f.find(view, node => node.type === f.load('src/workspace/ImportMemories.tsx').default);
  const request = { id: 'input-a', topicId: 'topic-a', title: 'Organize imported memories', text: importOrganizationInstruction('en', ['a']), context: [{ kind: 'version', id: 'v-a' }], memories: [{ memory_id: 'a', version_id: 'v-a' }] };
  f.overrides.library_detail = () => ({ state: 'active', current: { id: 'changed' } });
  await assert.rejects(importer.props.onOrganize(request), error => error.code === 'import_selection_changed');
  assert.equal(f.calls.filter(call => call.name === 'discussion_open').length, 0);
  f.overrides.library_detail = () => ({ state: 'trashed', current: { id: 'v-a' } });
  await assert.rejects(importer.props.onOrganize(request), error => error.code === 'import_selection_changed');
  f.overrides.library_detail = () => ({ state: 'active', current: { id: 'v-a' } });
  f.overrides.discussion_submit = () => { throw { code: 'discussion_unavailable' }; };
  await assert.rejects(importer.props.onOrganize(request));
  const reads = f.calls.filter(call => call.name === 'library_detail').length;
  f.overrides.library_detail = () => { throw Error('Must not reread after an unconfirmed submission'); };
  f.overrides.discussion_submit = () => ({ id: 'topic-a', title: 'Imported memories' });
  await importer.props.onOrganize(request);
  assert.equal(f.calls.filter(call => call.name === 'library_detail').length, reads);
  const submissions = f.calls.filter(call => call.name === 'discussion_submit');
  assert.equal(JSON.stringify(submissions[0].args), JSON.stringify(submissions[1].args));
});

for (const replayed of [false, true]) test(`${replayed ? 'replayed' : 'completed'} undo disables within-batch duplicate links but retains pre-existing duplicates`, async t => {
  const f = setup(t);
  f.overrides.import_apply = () => ({
    ...result(),
    items: [result().items[0], { ...result().items[0], file_name: 'Same ideas.md', status: 'duplicate' }, result().items[1]],
    receipt: { ...result().receipt, status: replayed ? 'undone' : 'applied' },
  });
  await f.click('Choose files'); await f.click('Import selected files');
  if (!replayed) await f.click('Undo this import');
  const links = f.nodes(f.view.tree).filter(node => node.type === 'button' && f.text(node) === 'View memory');
  assert.equal(links.length, 3);
  assert.equal(links[0].props.disabled, true);
  assert.equal(links[1].props.disabled, true);
  assert.equal(links[2].props.disabled, false);
  links[2].props.onClick(); await f.settle();
  assert.equal(f.events.find(event => Array.isArray(event) && event[0] === 'record')[1].id, 'b');
});
