import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
for (const failure of [false, true]) test(`switching record removes stale receipt during ${failure ? 'failed' : 'pending'} loading`, async t => {
  const f = workspaceFixture(t);
  let finish;
  f.overrides.memory_receipts = async ({key}) => key.id === 'a' ? [{request_id:'receipt-a',memory_id:'a',capture_id:'capture-a',status:'applied',before_version:'v0',after_version:'v1'}] : new Promise((resolve,reject) => {finish = failure ? reject : resolve;});
  const props = {record:{kind:'memory',id:'a'},revision:0,onOpen(){},onRefresh(){}};
  const view = f.mount(f.load('src/workspace/MemoryReceipts.tsx').default, props);
  await f.settle();
  const undo = f.find(view,n=>n.type==='button' && f.text(n)==="Undo change");
  f.render(view, {...props,record:{kind:'memory',id:'b'}});
  await f.settle();
  assert(!f.text(view.tree).includes("Undo change"));
  undo.props.onClick(); await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_action').length,0);
  finish(failure ? "Read failed" : []); await f.settle();
  assert(!f.text(view.tree).includes("Undo change"));
});

test('late undo completion cannot navigate to an unmounted record', async t => {
  const f=workspaceFixture(t), opened=[];
  let finish;
  f.overrides.memory_receipts=async()=>[{request_id:'receipt-a',memory_id:'a',status:'applied',logical_input_id:null}];
  f.overrides['library_action']=()=>new Promise(resolve=>finish=resolve);
  const view=f.mount(f.load('src/workspace/MemoryReceipts.tsx').default,{record:{kind:'memory',id:'a'},revision:0,onOpen:key=>opened.push(key),onRefresh(){}});
  await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)===('Undo change')).props.onClick();await f.settle();
  f.unmount(view); finish({memory_id:'old-memory'});await f.settle();
  assert.deepEqual(opened,[]);
});

test('background revisions preserve an open immutable change comparison', async t => {
  const f=workspaceFixture(t);
  f.overrides.memory_receipts=async()=>[{request_id:'receipt-a',memory_id:'a',capture_id:'raw-a',status:'applied',before_version:null,after_version:'v1'}];
  f.overrides.memory_receipt_changes=async()=>([{receipt:{before_version:null},before:null,after:{id:'v1',body:"Confirmed version"}}]);
  const props={record:{kind:'memory',id:'a'},revision:0,onOpen(){},onRefresh(){}};
  const view=f.mount(f.load('src/workspace/MemoryReceipts.tsx').default,props);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==="View changes").props.onClick();await f.settle();
  assert(f.find(view,n=>n.props?.title==="Memory changes"));
  f.overrides.memory_receipt_changes=async()=>([{receipt:{before_version:null},before:null,after:{id:'v1',body:"Other subsequent data"}}]);
  f.render(view,{...props,revision:1});await f.settle();
  assert(f.text(f.find(view,n=>n.props?.title==="Memory changes")).includes("Confirmed version"));
  assert(!f.text(view.tree).includes("Other subsequent data"));
});

test('background refresh preserves an in-flight undo', async t => {
  const f=workspaceFixture(t);let refreshed=0,finish;
  f.overrides.memory_receipts=async()=>[{request_id:'receipt-a',memory_id:'a',status:'applied',logical_input_id:null}];
  f.overrides['library_action']=()=>new Promise(resolve=>finish=resolve);
  const props={record:{kind:'memory',id:'a'},revision:0,onOpen(){},onRefresh(){refreshed++;}};
  const view=f.mount(f.load('src/workspace/MemoryReceipts.tsx').default,props);await f.settle();
  const label='Undo change';
  f.find(view,n=>n.type==='button'&&f.text(n)===label).props.onClick();await f.settle();
  f.render(view,{...props,revision:1});await f.settle();
  assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)===label).props.disabled,true);
  finish({memory_id:'new-memory'});await f.settle();
  assert.equal(refreshed,1);
});

for (const grouped of [false,true]) test(`${grouped?'conversation':'standalone'} receipts use the matching undo endpoint and retain receipt comparisons`,async t=>{
  const f=workspaceFixture(t),opened=[];let refreshed=0;
  const receipt={request_id:'receipt-a',memory_id:'a',action:'edit',status:'applied',before_version:'v0',after_version:'v1',logical_input_id:grouped?'input-a':null};
  f.overrides.memory_receipts=async()=>[receipt];
  f.overrides.memory_receipt_changes=async()=>[{receipt,before:{id:'v0',body:'Before'},after:{id:'v1',body:'After'}}];
  f.overrides.discussion_undo=async()=>({receipt:{action:'undo'},conflicts:[]});
  f.overrides.library_action=async()=>({action:'undo'});
  const props={record:f.keyA,presentation:'summary',currentVersion:'v1',revision:0,onOpen(key){opened.push(key.id);},onRefresh(){refreshed++;}};
  const view=f.mount(f.load('src/workspace/MemoryReceipts.tsx').default,props);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='View changes').props.onClick();await f.settle();
  assert.deepEqual({...f.calls.find(c=>c.name==='memory_receipt_changes').args},{request:'receipt-a'});
  const label=grouped?'Undo this turn':'Undo change';
  const undo=f.find(view,n=>n.type==='button'&&f.text(n)===label);undo.props.onClick();undo.props.onClick();await f.settle();
  const writes=f.calls.filter(c=>c.name==='discussion_undo'||c.name==='library_action');
  assert.equal(writes.length,1);assert.equal(writes[0].name,grouped?'discussion_undo':'library_action');
  if(grouped){assert.equal(writes[0].args.inputId,'input-a');assert(writes[0].args.requestId);}
  else {assert.equal(writes[0].args.action.original_request,'receipt-a');assert(writes[0].args.action.request_id);}
  assert.equal(refreshed,1);assert.deepEqual(opened,['a']);
  f.render(view,{...props,revision:1});await f.settle();
  const comparison=f.find(view,n=>n.props?.title==='Memory changes');
  assert(f.text(comparison).includes('Before'));assert(f.text(comparison).includes('After'));
});

test('grouped undo conflicts remain actionable and retry reuses the request id without claiming success',async t=>{
  const f=workspaceFixture(t),opened=[];let refreshed=0,attempts=0;
  f.overrides.memory_receipts=async()=>[{request_id:'receipt-a',memory_id:'a',status:'applied',logical_input_id:'input-a'}];
  f.overrides.discussion_undo=async()=>++attempts===1?{receipt:null,conflicts:['memory-b']}:{receipt:{action:'undo'},conflicts:[]};
  const view=f.mount(f.load('src/workspace/MemoryReceipts.tsx').default,{record:f.keyA,onOpen(key){opened.push(key.id);},onRefresh(){refreshed++;}});await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='Undo this turn').props.onClick();await f.settle();
  assert(f.text(view.tree).includes('Nothing was undone:'));assert.equal(refreshed,0);assert.deepEqual(opened,[]);
  f.find(view,n=>n.type==='button'&&f.text(n)==='memory-b').props.onClick();await f.settle();assert.deepEqual(opened,['memory-b']);
  f.find(view,n=>n.type==='button'&&f.text(n)==='Undo this turn').props.onClick();await f.settle();
  const writes=f.calls.filter(c=>c.name==='discussion_undo');assert.equal(writes.length,2);assert.equal(writes[0].args.requestId,writes[1].args.requestId);
  assert.equal(refreshed,1);assert.deepEqual(opened,['memory-b','a']);assert(!f.text(view.tree).includes('Nothing was undone:'));
});
