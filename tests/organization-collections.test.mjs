import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const suggestion={collection:{id:'collection-a',name:"Product design",description:'',revision:0,count:0},reason:"Also interested in the capture experience"};
function setup(t){
  const f=workspaceFixture(t,{native:true});
  f.overrides.organization_collections=async()=>[suggestion];
  f.overrides.organization_dismiss=async()=>{};
  const Component=f.load('src/workspace/OrganizationCollections.tsx').default;
  const props={receipt:'receipt-a',record:f.keyA,revision:0,disabled:false,onRefresh(){}};
  return {f,Component,props};
}
const button=(f,v,label)=>f.find(v,n=>n.type==='button'&&f.text(n)===label);

test('receipt recommendations share a request across views and never automatically join',async t=>{
  const {f,Component,props}=setup(t);const a=f.mount(Component,props),b=f.mount(Component,props);await f.settle();
  f.render(a,{...props,revision:1});await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='organization_collections').length,1);
  assert.equal(f.calls.filter(c=>c.name==='organization_collect').length,0);
  assert(f.text(a.tree).includes("Product design"));assert(!f.text(b.tree).includes("Also interested"));button(f,b,'?').props.onClick();await f.settle();assert(f.text(b.tree).includes("Also interested"));
});
test('explicit joining locks double clicks, uses receipt identity and can undo',async t=>{
  const {f,Component,props}=setup(t);let finish;
  f.overrides.organization_collect=()=>new Promise(r=>finish=r);
  f.overrides.navigation_collect=async()=>{};
  const Toast=f.load('src/workspace/Toast.tsx').default,toast=f.mount(Toast);
  const a=f.mount(Component,props);await f.settle();const add=button(f,a,"＋ Product design");
  add.props.onClick();add.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='organization_collect').length,1);
  assert.equal(f.calls.find(c=>c.name==='organization_collect').args.receipt,'receipt-a');
  finish();await f.settle();assert.equal(a.tree,null);button(f,toast,"Undo").props.onClick();await f.settle();
  assert.equal(f.calls.find(c=>c.name==='navigation_collect').args.included,false);
  assert.equal(f.calls.find(c=>c.name==='navigation_collect').args.key.id,'a');
  f.unmount(toast);
});
test('failed recommendation only retries explicitly; dismiss survives remount',async t=>{
  const {f,Component,props}=setup(t);let count=0;
  f.overrides.organization_collections=async()=>{if(++count===1)throw "Model temporarily disconnected";return [suggestion];};
  const a=f.mount(Component,props);await f.settle();assert(f.text(a.tree).includes("Model temporarily disconnected"));
  f.render(a,{...props,revision:2});await f.settle();assert.equal(count,1);
  button(f,a,"Retry suggestions").props.onClick();await f.settle();assert.equal(count,2);
  button(f,a,"Ignore these suggestions").props.onClick();await f.settle();assert.equal(a.tree,null);
  f.unmount(a);const b=f.mount(Component,props);await f.settle();assert.equal(b.tree,null);assert.equal(count,2);
});
test('empty recommendations stay quiet and late completion cannot refresh an unmounted record',async t=>{
  const {f,Component,props}=setup(t);let finish,refreshed=0;
  f.overrides.organization_collect=()=>new Promise(r=>finish=r);
  const a=f.mount(Component,{...props,onRefresh(){refreshed++;}});await f.settle();
  button(f,a,"＋ Product design").props.onClick();await f.settle();f.unmount(a);finish();await f.settle();assert.equal(refreshed,0);
  f.overrides.organization_collections=async()=>[];
  const b=f.mount(Component,{...props,receipt:'empty'});await f.settle();assert.equal(b.tree,null);
});

test('receipt only offers recommendations after successful organization and uses its actual destination',async t=>{
  const {f}=setup(t),Receipt=f.load('src/workspace/MemoryCollections.tsx').default,Recommendations=f.load('src/workspace/OrganizationCollections.tsx').default;
  let status='processing',r=null;
  f.overrides.organization_jobs=async()=>[{capture_id:'raw',attempt_id:'attempt',status,reason:'',receipt:r}];
  const props={record:{kind:'capture',id:'raw'},currentVersion:'new',revision:0,onOpen(){},onRefresh(){}};
  const view=f.mount(Receipt,props);await f.settle();assert(!f.nodes(view.tree).some(n=>n.type===Recommendations));
  status='done';r={request_id:'receipt',memory_id:'existing-target',before_version:'old',after_version:'new',status:'applied'};
  f.render(view,{...props,revision:1});await f.settle();
  assert.equal(f.find(view,n=>n.type===Recommendations).props.record.id,'existing-target');
  f.render(view,{...props,currentVersion:'newer',revision:1});await f.settle();assert(!f.nodes(view.tree).some(n=>n.type===Recommendations));
  r={...r,status:'undone'};f.render(view,{...props,revision:2});await f.settle();assert(!f.nodes(view.tree).some(n=>n.type===Recommendations));
});

test('joining failure leaves the suggestion available without claiming success',async t=>{
  const {f,Component,props}=setup(t);f.overrides.organization_collect=async()=>{throw "Collection changed";};
  const view=f.mount(Component,props);await f.settle();button(f,view,"＋ Product design").props.onClick();await f.settle();
  assert(f.text(view.tree).includes("Collection changed"));assert(!f.text(view.tree).includes("Undo addition"));assert(button(f,view,"＋ Product design"));
});
