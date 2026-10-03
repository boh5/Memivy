import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';

const before={name:'Interview preparation',description:'Practice questions',revision:1,archived:false};
const after={...before,name:'Career planning',description:'Interviews and next steps',revision:2};
const change={collection_id:'collection-a',before,after,added_memory_ids:['memory-a'],removed_memory_ids:['memory-b']};
const receipt={request_id:'receipt-a',memory_id:null,status:'applied',action:'update_collection_members',collection_changes:[change]};

function mountChild(f,view,Component) {
  return f.mount(Component,f.find(view,node=>node.type===Component).props);
}

test('a collection-only turn reports collections and shares one undo with memory changes',async t=>{
  const f=workspaceFixture(t);let refreshed=0,finish;
  const Changes=f.load('src/workspace/MemoryChanges.tsx').default;
  f.overrides.discussion_undo=()=>new Promise(resolve=>finish=resolve);
  const props={inputId:'input-a',receipts:[receipt],onOpenRecord(){},onRefresh(){refreshed++;}};
  const view=f.mount(Changes,props);await f.settle();
  assert(f.text(view.tree).includes('Updated 1 collection'));
  assert(!f.text(view.tree).includes('Changes undone'));
  f.render(view,{...props,receipts:[receipt,{...receipt,request_id:'memory-edit',memory_id:'memory-a',collection_changes:[]}]});await f.settle();
  assert(f.text(view.tree).includes('Updated 1 memory'));
  const undo=f.nodes(view.tree).filter(node=>node.type==='button'&&f.text(node)==='Undo this turn');
  assert.equal(undo.length,1);undo[0].props.onClick();undo[0].props.onClick();await f.settle();
  assert.equal(f.calls.filter(call=>call.name==='discussion_undo').length,1);
  finish({receipt:{action:'undo'},conflicts:[],collection_conflicts:[]});await f.settle();
  assert.equal(refreshed,1);
});

test('stopped turns preserve committed collection receipts without claiming full completion',async t=>{
  const f=workspaceFixture(t),Changes=f.load('src/workspace/MemoryChanges.tsx').default;
  const props={inputId:'input-a',receipts:[receipt],incomplete:true,onOpenRecord(){},onRefresh(){}};
  const view=f.mount(Changes,props);await f.settle();
  assert(f.text(view.tree).includes('The changes shown here were saved.'));
  f.render(view,{...props,receipts:[]});await f.settle();
  assert.equal(view.tree,null);
  f.render(view,{...props,receipts:[{...receipt,status:'undone'}]});await f.settle();
  assert(f.text(view.tree).includes('Changes undone'));
  assert(!f.text(view.tree).includes('The changes shown here were saved.'));
  assert(!f.nodes(view.tree).some(node=>node.type==='button'&&f.text(node)==='Undo this turn'));
});

test('collection changes remain available in comparison without inventing a memory version',async t=>{
  const f=workspaceFixture(t),Changes=f.load('src/workspace/MemoryChanges.tsx').default;
  f.overrides.discussion_changes=async()=>[];
  const props={inputId:'input-a',receipts:[receipt],onOpenRecord(){},onRefresh(){}};
  const view=f.mount(Changes,props);await f.settle();
  f.find(view,node=>node.type==='button'&&f.text(node)==='View changes').props.onClick();await f.settle();
  const CollectionChanges=f.load('src/workspace/CollectionChanges.tsx').default;
  const comparison=mountChild(f,view,CollectionChanges);await f.settle();
  assert(f.text(comparison.tree).includes('Career planning'));
  assert(f.text(comparison.tree).includes('Interview preparation'));
  assert(f.text(comparison.tree).includes('Practice questions'));
  assert(f.text(comparison.tree).includes('Interviews and next steps'));
  assert(!f.text(view.tree).includes('(new memory)'));
  f.render(view,{...props,receipts:[{...receipt,collection_changes:[{...change,after:{...after,name:'Later name'}}]}]});await f.settle();
  assert.equal(f.find(view,node=>node.type===CollectionChanges).props.changes[0].after.name,'Career planning');
});

test('collection undo conflicts name the collection and never open it as a memory',async t=>{
  const f=workspaceFixture(t),opened=[];
  const Conflict=f.load('src/workspace/MemoryChanges.tsx').UndoConflicts;
  const view=f.mount(Conflict,{result:{receipt:null,conflicts:[],collection_conflicts:['collection-a']},receipts:[receipt],onOpenRecord:key=>opened.push(key)});await f.settle();
  assert(f.text(view.tree).includes('Nothing was undone:'));
  assert(f.text(view.tree).includes('Career planning'));
  assert(!f.text(view.tree).includes('collection-a'));
  assert.equal(f.nodes(view.tree).filter(node=>node.type==='button').length,0);
  assert.deepEqual(opened,[]);
});

test('collection history reads durable groups without reading the original discussion',async t=>{
  const f=workspaceFixture(t),History=f.load('src/workspace/MemoryChanges.tsx').CollectionChangeHistory;
  f.overrides.collection_agent_changes=async({collectionId})=>collectionId==='collection-a'?[{input_id:'deleted-discussion-input',receipts:[receipt]}]:[];
  const props={collectionId:'collection-a',onOpenRecord(){},onRefresh(){}};
  const view=f.mount(History,props);await f.settle();
  const Changes=f.load('src/workspace/MemoryChanges.tsx').default;
  assert.equal(f.find(view,node=>node.type===Changes).props.inputId,'deleted-discussion-input');
  assert.equal(f.calls.filter(call=>call.name==='discussion_messages').length,0);
  f.render(view,{...props,collectionId:'collection-b'});await f.settle();
  assert(!f.nodes(view.tree).some(node=>node.type===Changes));
  assert(f.text(view.tree).includes('No changes from discussions yet.'));
});

test('late undo completion does not refresh an unmounted turn',async t=>{
  const f=workspaceFixture(t),Changes=f.load('src/workspace/MemoryChanges.tsx').default;
  let finish,refreshed=0;
  f.overrides.discussion_undo=()=>new Promise(resolve=>finish=resolve);
  const view=f.mount(Changes,{inputId:'input-a',receipts:[receipt],onOpenRecord(){},onRefresh(){refreshed++;}});await f.settle();
  f.find(view,node=>node.type==='button'&&f.text(node)==='Undo this turn').props.onClick();await f.settle();
  f.unmount(view);finish({receipt:{action:'undo'},conflicts:[],collection_conflicts:[]});await f.settle();
  assert.equal(refreshed,0);
});

test('member read failures stay retryable while unavailable memories are identified separately',async t=>{
  const f=workspaceFixture(t),opened=[];let attempts=0;
  f.overrides.library_detail=async({key})=>{
    if(key.id==='memory-b') throw {code:'unavailable'};
    if(++attempts===1) throw {code:'operation_failed'};
    return {title:'A recovered memory'};
  };
  const CollectionChanges=f.load('src/workspace/CollectionChanges.tsx').default;
  const view=f.mount(CollectionChanges,{changes:[{...change,added_memory_ids:['memory-a','memory-b'],removed_memory_ids:[]}],onOpenRecord:key=>opened.push(key.id)});await f.settle();
  const node=f.find(view,node=>typeof node.type==='function'&&node.type.name==='ChangedMembers'&&node.props.added);
  const members=f.mount(node.type,node.props);await f.settle();
  assert.equal(f.calls.filter(call=>call.name==='library_detail').length,0);
  f.find(members,node=>node.props.className==='collection-members-toggle').props.onClick();await f.settle();
  assert(f.text(members.tree).includes('This memory could not be read. Try again.'));
  assert(f.text(members.tree).includes('This memory is no longer available.'));
  const retry=f.find(members,node=>node.type==='button'&&f.text(node)==='Try again');retry.props.onClick();await f.settle();
  assert(f.text(members.tree).includes('A recovered memory'));
  assert(!f.text(members.tree).includes('This memory could not be read.'));
  f.find(members,node=>node.type==='button'&&f.text(node)==='A recovered memory').props.onClick();await f.settle();
  assert.deepEqual(opened,['memory-a']);
  assert(f.find(members,node=>node.type==='button'&&f.text(node)==='This memory is no longer available.').props.disabled);
});

test('undone collection history and turn comparisons identify their details as historical',async t=>{
  const f=workspaceFixture(t),undone={...receipt,status:'undone'};
  f.overrides.collection_agent_changes=async()=>[{input_id:'input-a',receipts:[undone]}];
  f.overrides.discussion_changes=async()=>[{receipt:undone,before:null,after:null}];
  const module=f.load('src/workspace/MemoryChanges.tsx');
  const history=f.mount(module.CollectionChangeHistory,{collectionId:'collection-a',onOpenRecord(){},onRefresh(){}});await f.settle();
  assert(f.text(history.tree).includes('These changes were undone.'));
  const turn=mountChild(f,history,module.default);await f.settle();
  f.find(turn,node=>node.type==='button'&&f.text(node)==='View changes').props.onClick();await f.settle();
  assert(f.text(turn.tree).includes('The details below show the original changes.'));
  assert(!f.nodes(turn.tree).some(node=>node.type==='button'&&f.text(node)==='Undo this turn'));
});
