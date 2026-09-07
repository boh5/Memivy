import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
for (const failure of [false, true]) test(`switching record removes stale receipt during ${failure ? 'failed' : 'pending'} loading`, async t => {
  const f = workspaceFixture(t);
  let finish;
  f.overrides.organization_jobs = async ({key}) => key.id === 'a' ? [{capture_id:'capture-a',attempt_id:'attempt-a',status:'done',reason:'',receipt:{request_id:'receipt-a',memory_id:'a',capture_id:'capture-a',status:'applied',before_version:'v0',after_version:'v1'}}] : new Promise((resolve,reject) => {finish = failure ? reject : resolve;});
  const props = {record:{kind:'memory',id:'a'},revision:0,onOpen(){},onRefresh(){}};
  const view = f.mount(f.load('src/workspace/OrganizationReceipt.tsx').default, props);
  await f.settle();
  const undo = f.find(view,n=>n.type==='button' && f.text(n)==='撤销整理');
  f.render(view, {...props,record:{kind:'memory',id:'b'}});
  await f.settle();
  assert(!f.text(view.tree).includes('撤销整理'));
  undo.props.onClick(); await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_action').length,0);
  finish(failure ? '读取失败' : []); await f.settle();
  assert(!f.text(view.tree).includes('撤销整理'));
});

test('late completion from an unmounted receipt cannot navigate to the old record', async t => {
  const f=workspaceFixture(t), opened=[];
  let finish;
  f.overrides.organization_jobs=async()=>[{capture_id:'raw-a',attempt_id:'attempt-a',status:'failed',reason:'',receipt:null}];
  f.overrides.organization_new=()=>new Promise(resolve=>finish=resolve);
  const view=f.mount(f.load('src/workspace/OrganizationReceipt.tsx').default,{record:{kind:'capture',id:'raw-a'},revision:0,onOpen:key=>opened.push(key),onRefresh(){}});
  await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='另存为新记忆').props.onClick();await f.settle();
  f.unmount(view); finish({memory_id:'old-memory'});await f.settle();
  assert.deepEqual(opened,[]);
});

test('background revisions preserve an open immutable change comparison', async t => {
  const f=workspaceFixture(t);
  f.overrides.organization_jobs=async()=>[{capture_id:'raw-a',attempt_id:'attempt-a',status:'done',reason:'',receipt:{request_id:'receipt-a',memory_id:'a',capture_id:'raw-a',status:'applied',before_version:null,after_version:'v1'}}];
  f.overrides.library_detail=async()=>({history:[{id:'v1',body:'已确认的版本'}]});
  const props={record:{kind:'memory',id:'a'},revision:0,onOpen(){},onRefresh(){}};
  const view=f.mount(f.load('src/workspace/OrganizationReceipt.tsx').default,props);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='查看变化').props.onClick();await f.settle();
  assert(f.find(view,n=>n.props?.title==='这次整理的变化'));
  f.render(view,{...props,revision:1});await f.settle();
  assert(f.find(view,n=>n.props?.title==='这次整理的变化'));
});

test('background refresh neither unlocks nor discards an in-flight save on the same record', async t => {
  const f=workspaceFixture(t),opened=[];let finish;
  f.overrides.organization_jobs=async()=>[{capture_id:'raw-a',attempt_id:'attempt-a',status:'failed',reason:'',receipt:null}];
  f.overrides.organization_new=()=>new Promise(resolve=>finish=resolve);
  const props={record:{kind:'capture',id:'raw-a'},revision:0,onOpen:k=>opened.push(k),onRefresh(){}};
  const view=f.mount(f.load('src/workspace/OrganizationReceipt.tsx').default,props);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='另存为新记忆').props.onClick();await f.settle();
  f.render(view,{...props,revision:1});await f.settle();
  assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)==='另存为新记忆').props.disabled,true);
  finish({memory_id:'new-memory'});await f.settle();
  assert.equal(opened[0]?.id,'new-memory');
});
