import test from 'node:test';
import assert from 'node:assert/strict';
import { cleanupDiff } from '../src/workspace/cleanupDiff.ts';
import { workspaceFixture } from './helpers/workspace.mjs';

test('diff preserves complete before and after text with bounded work', () => {
  for (const [before, after] of [['a\nb\nc\n', 'a\nx\nb\ny\n'], ['', '正文'], ['旧', ''], ['a\n'.repeat(64000), 'b\n'.repeat(64000)], ['相同', '相同']]) {
    const start = performance.now(), parts = cleanupDiff(before, after);
    assert.equal(parts.filter(p => p.kind !== 'add').map(p => p.text).join(''), before);
    assert.equal(parts.filter(p => p.kind !== 'remove').map(p => p.text).join(''), after);
    assert.ok(performance.now() - start < 1000);
    assert.ok(parts.length <= 8000);
  }
});
const snapshot = { memory_id: 'a', expected_version: 'v-a', draft_request: 'draft-a', title: '草稿标题', body: '未保存草稿' };
function setup(t) {
  const f = workspaceFixture(t, { modules: { './useDraft': { flushDraft: async () => {}, refreshDrafts: async () => {} } } });
  f.overrides.cleanup_prepare = async () => snapshot;
  const props = { detail: { key: f.keyA, state:'active', title:'原题', body:'原文', current:{id:'v-a'} }, toolbar: {}, onClose(){}, onSaved(){} };
  return { f, props, Cleanup: f.load('src/workspace/MemoryCleanup.tsx').default };
}
test('late generation after switching memories cannot write or show a result', async t => {
  const {f,props,Cleanup} = setup(t); let finish;
  f.overrides.cleanup_generate = () => new Promise(resolve => { finish = resolve; });
  const view = f.mount(Cleanup, props); await f.settle(); f.unmount(view); finish('迟到结果'); await f.settle();
  assert.ok(f.calls.some(c => c.name === 'cleanup_cancel'));
  assert.ok(!f.calls.some(c => c.name === 'cleanup_save'));
});
test('accept saves the final edited candidate once without another model call', async t => {
  const {f,props,Cleanup} = setup(t); let received;
  f.overrides.cleanup_generate = async () => '模型候选';
  f.overrides.cleanup_save = async args => { received = args.request; return {request_id:args.request.request_id}; };
  const view = f.mount(Cleanup, props); await f.settle();
  f.find(view, n => n.type === 'MarkdownEditor').props.onChange('用户最终审核稿'); await f.settle();
  const save = f.find(view, n => n.type === 'button' && f.text(n) === '接受并保存');
  save.props.onClick(); save.props.onClick(); await f.settle();
  assert.equal(received.body, '用户最终审核稿'); assert.deepEqual({...received.snapshot}, snapshot);
  assert.equal(f.calls.filter(c => c.name === 'cleanup_generate').length, 1);
  assert.equal(f.calls.filter(c => c.name === 'cleanup_save').length, 1);
});
test('a newer version disables acceptance while retaining the candidate', async t => {
  const {f,props,Cleanup} = setup(t); f.overrides.cleanup_generate = async () => '保留候选';
  const view = f.mount(Cleanup, props); await f.settle();
  f.render(view, {...props, detail: {...props.detail, current:{id:'v-new'}}}); await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='接受并保存').props.disabled, true);
  assert.equal(f.find(view,n=>n.type==='MarkdownEditor').props.value, '保留候选');
});

test('editor shortcut saves the very last input without waiting for a render', async t => {
  const {f,props,Cleanup} = setup(t); let received;
  f.overrides.cleanup_generate = async () => '候选';
  f.overrides.cleanup_save = async args => { received = args.request.body; return {}; };
  const view = f.mount(Cleanup,props); await f.settle();
  const editor = f.find(view,n=>n.type==='MarkdownEditor');
  editor.props.onChange('最后一次输入'); editor.props.onSave(); await f.settle();
  assert.equal(received,'最后一次输入');
});

test('Escape dismisses hovered tooltips even when keyboard focus is elsewhere', async t => {
  const f=workspaceFixture(t), Tooltip=f.load('src/workspace/IconButton.tsx').ActionTooltip;
  const view=f.mount(Tooltip,{label:'整理正文',children:null}); await f.settle();
  f.key({key:'Escape'}); await f.settle();
  assert.match(view.tree.props.className,/tooltip-dismissed/);
  view.tree.props.onFocus(); await f.settle();
  assert.doesNotMatch(view.tree.props.className,/tooltip-dismissed/);
});

test('lost save acknowledgement can replay the exact request after a head refresh', async t => {
  const {f,props,Cleanup} = setup(t); let firstRequest, attempts=0, saved=0;
  f.overrides.cleanup_generate = async () => '审核稿';
  f.overrides.cleanup_save = async ({request}) => {
    attempts++;
    if (attempts===1) { firstRequest=structuredClone(request); throw Error('IPC reply lost'); }
    assert.deepEqual(JSON.parse(JSON.stringify(request)),firstRequest);
    return {request_id:request.request_id,after_version:'v-saved'};
  };
  const view=f.mount(Cleanup,{...props,onSaved(){saved++;}}); await f.settle();
  f.find(view,n=>n.type==='button' && f.text(n)==='接受并保存').props.onClick(); await f.settle();
  f.render(view,{...view.props,detail:{...props.detail,title:snapshot.title,body:'审核稿',current:{id:'v-saved'}}}); await f.settle();
  const retry=f.find(view,n=>n.type==='button' && /接受并保存|重试保存/.test(f.text(n)));
  assert.equal(retry.props.disabled,false);
  retry.props.onClick(); await f.settle(); assert.equal(saved,1); assert.equal(attempts,2);
});

test('an editing draft may be reviewed back to the saved body', async t => {
  const {f,props,Cleanup} = setup(t);
  f.overrides.cleanup_prepare=async()=>({...snapshot,title:props.detail.title});
  f.overrides.cleanup_generate=async()=>props.detail.body;
  const view=f.mount(Cleanup,props); await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='接受并保存').props.disabled,false);
});

test('a transient detail refresh failure retains the cleanup session', async t => {
  const {f,props}=setup(t); let fail=false;
  f.overrides.library_detail=async()=>{if(fail)throw Error('database temporarily busy');return {...props.detail,current:{...props.detail.current,capture_ids:[],actor:"user",created_at:1},history:[],sources:[]};};
  const Detail=f.load('src/workspace/MemoryDetail.tsx').default;
  const view=f.mount(Detail,{record:f.keyA,revision:0,query:'',initialReceipt:null,onChanged(){},onBack(){},onDiscuss(){}});await f.settle();
  f.find(view,n=>n.props.label==='整理正文').props.onClick();await f.settle();
  const toolbar=f.find(view,n=>n.props.className==='cleanup-toolbar');toolbar.props.ref({});await f.settle();
  assert.ok(f.find(view,n=>n.type?.name==='MemoryCleanup'));
  fail=true;f.render(view,{...view.props,revision:1});await f.settle();
  assert.ok(f.find(view,n=>n.type?.name==='MemoryCleanup'));
});

test('editing a failed-save candidate cannot use replay to bypass a version conflict', async t => {
  const {f,props,Cleanup}=setup(t);
  f.overrides.cleanup_generate=async()=> '审核稿';
  f.overrides.cleanup_save=async()=>{throw Error('reply lost');};
  const view=f.mount(Cleanup,props);await f.settle();
  f.find(view,n=>n.type==='button' && f.text(n)==='接受并保存').props.onClick();await f.settle();
  f.find(view,n=>n.type==='MarkdownEditor').props.onChange('审核稿');await f.settle();
  assert.ok(f.find(view,n=>n.type==='button' && f.text(n)==='重试保存'));
  f.render(view,{...props,detail:{...props.detail,current:{id:'newer'}}});await f.settle();
  f.find(view,n=>n.type==='MarkdownEditor').props.onChange('新的改动');await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='接受并保存').props.disabled,true);
  f.find(view,n=>n.type==='MarkdownEditor').props.onSave();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='cleanup_save').length,1);
});
