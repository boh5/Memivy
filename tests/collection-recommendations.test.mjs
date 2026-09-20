import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const suggestion={collection:{id:'collection-a',name:'Product design',description:'',revision:3,count:0},reason:'Related to the capture experience'};
const button=(f,v,label)=>f.find(v,n=>n.type==='button'&&f.text(n)===label);
function setup(t){
  const f=workspaceFixture(t,{native:true});
  f.overrides.collection_recommendations=async()=>[suggestion];
  f.overrides.collection_accept_recommendation=async()=>{};
  const Component=f.load('src/workspace/CollectionRecommendations.tsx').default;
  return {f,Component,props:{record:f.keyA,currentVersion:'v1',members:[],collections:[suggestion.collection],onRefresh(){}}};
}
test('recommendations require a click and membership requires a separate explicit choice',async t=>{
  const {f,Component,props}=setup(t),view=f.mount(Component,props);await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='collection_recommendations').length,0);
  button(f,view,'Suggest collections').props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='collection_recommendations').length,1);
  assert.equal(f.calls.filter(c=>c.name==='collection_accept_recommendation').length,0);
  button(f,view,'?').props.onClick();await f.settle();assert(f.text(view.tree).includes(suggestion.reason));
  let finish;f.overrides.collection_accept_recommendation=()=>new Promise(resolve=>finish=resolve);
  const add=button(f,view,'＋ Product design');add.props.onClick();add.props.onClick();await f.settle();
  const calls=f.calls.filter(c=>c.name==='collection_accept_recommendation');assert.equal(calls.length,1);
  assert.deepEqual({...calls[0].args},{memoryId:'a',expectedVersion:'v1',collection:'collection-a',revision:3});
  finish();await f.settle();assert(!f.text(view.tree).includes('＋ Product design'));
});
test('model failures retry only on request and dismissing results needs no write',async t=>{
  const {f,Component,props}=setup(t);let attempts=0;
  f.overrides.collection_recommendations=async()=>{if(++attempts===1)throw 'Model unavailable';return [suggestion];};
  const view=f.mount(Component,props);await f.settle();button(f,view,'Suggest collections').props.onClick();await f.settle();
  assert(f.text(view.tree).includes('Model unavailable'));
  f.render(view,{...props,members:['unrelated']});await f.settle();assert.equal(attempts,1);
  button(f,view,'Suggest collections').props.onClick();await f.settle();assert.equal(attempts,2);
  button(f,view,'Dismiss').props.onClick();await f.settle();assert(!f.text(view.tree).includes('＋ Product design'));
  assert.equal(f.calls.filter(c=>c.name==='collection_accept_recommendation').length,0);
});
test('failed acceptance retains the suggestion and an unmounted request cannot refresh another memory',async t=>{
  const {f,Component,props}=setup(t);let refreshed=0;
  const view=f.mount(Component,{...props,onRefresh(){refreshed++;}});await f.settle();
  button(f,view,'Suggest collections').props.onClick();await f.settle();
  f.overrides.collection_accept_recommendation=async()=>{throw 'Collection changed';};
  button(f,view,'＋ Product design').props.onClick();await f.settle();assert(button(f,view,'＋ Product design'));assert.equal(refreshed,0);
  let finish;f.overrides.collection_accept_recommendation=()=>new Promise(resolve=>finish=resolve);
  button(f,view,'＋ Product design').props.onClick();await f.settle();f.unmount(view);finish();await f.settle();assert.equal(refreshed,0);
});
test('a late model response is filtered against the latest collection revisions and membership',async t=>{
  const {f,Component,props}=setup(t);let finish;
  f.overrides.collection_recommendations=()=>new Promise(resolve=>finish=resolve);
  const view=f.mount(Component,props);await f.settle();
  button(f,view,'Suggest collections').props.onClick();await f.settle();
  f.render(view,{...props,collections:[{...suggestion.collection,revision:4}]});await f.settle();
  finish([suggestion]);await f.settle();
  assert(!f.text(view.tree).includes('＋ Product design'));
  assert.equal(f.calls.filter(c=>c.name==='collection_recommendations').length,1);
  f.render(view,{...props,members:[suggestion.collection.id]});await f.settle();
  assert(!f.text(view.tree).includes('＋ Product design'));
});

test('memory collections forwards existing collection snapshots while displaying only memberships',async t=>{
  const {f,props}=setup(t),Parent=f.load('src/workspace/MemoryCollections.tsx').default,Child=f.load('src/workspace/CollectionRecommendations.tsx').default;
  const member={id:'member',name:'Existing membership',description:'',revision:1,count:1};
  let collections=[member,suggestion.collection];
  f.overrides.navigation_collections=async()=>collections;
  f.overrides.navigation_record=async()=>({pinned:false,collections:[member.id]});
  const parent=f.mount(Parent,{...props,revision:0});await f.settle();
  let node=f.find(parent,n=>n.type===Child);
  assert.deepEqual(node.props.collections,collections);
  assert.deepEqual(node.props.members,[member.id]);
  assert(f.text(parent.tree).includes(member.name));
  assert(!f.text(parent.tree).includes(suggestion.collection.name));
  const child=f.mount(Child,node.props),key=node.key;await f.settle();
  button(f,child,'Suggest collections').props.onClick();await f.settle();
  assert(button(f,child,'＋ Product design'));
  collections=[member];
  f.render(parent,{...props,revision:1});await f.settle();
  node=f.find(parent,n=>n.type===Child);assert.equal(node.key,key);
  f.render(child,node.props);await f.settle();
  assert(!f.text(child.tree).includes('＋ Product design'));
  assert.equal(f.calls.filter(c=>c.name==='navigation_collections').length,2);
  assert.equal(f.calls.filter(c=>c.name==='collection_recommendations').length,1);
});
