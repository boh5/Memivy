import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';

function setup(t) {
  const f = workspaceFixture(t);
  const detail = {key:f.keyA,state:'active',title:'A thought',body:'Displayed body',current:{id:'v1'},sources:[],history:[],source_count:1,history_count:2};
  const props = {detail,revision:0,query:'',busy:false,onRefresh(){},onOpenDiscussion:async()=>{},onRestoreArchive(){},onRestoreVersion(){}};
  const view = f.mount(f.load('src/workspace/MemoryEvidence.tsx').default,props);
  const toggle = name => f.find(view,n=>n.type==='button' && f.text(n).startsWith(name)).props.onClick();
  return {f,view,props,toggle};
}

test('archives load only on demand and switching disclosure reuses the same snapshot',async t=>{
  const {f,view,toggle} = setup(t);
  await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_detail').length,0);
  toggle('Sources');await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_detail').length,1);
  assert.equal(f.calls.find(c=>c.name==='library_detail').args.archives,true);
  toggle('History');await f.settle();
  toggle('History');await f.settle();
  toggle('Sources');await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_detail').length,1);
  assert.equal(f.nodes(view.tree).filter(n=>n.props['aria-expanded']===true).length,1);
});

test('late archive reads cannot populate a newer revision and failed reads can retry',async t=>{
  const {f,view,props,toggle} = setup(t);
  let finish;
  f.overrides.library_detail = () => new Promise(resolve=>finish=resolve);
  toggle('Sources');await f.settle();
  const old = finish;
  f.overrides.library_detail = async()=>{throw Error('Read failed');};
  f.render(view,{...props,revision:1});await f.settle();
  old({...props.detail,sources:[{id:'old',capture:{text:'Obsolete archive',origin:{},created_at:1}}]});await f.settle();
  assert(!f.nodes(view.tree).some(n=>n.props.text==='Obsolete archive'));
  f.overrides.library_detail = async()=>({...props.detail,sources:[{id:'raw',capture:{text:'Original source',origin:{},created_at:1}}]});
  f.find(view,n=>n.type==='button' && f.text(n)==='Try again').props.onClick();await f.settle();
  assert(f.nodes(view.tree).some(n=>n.props.text==='Original source'));
});

test('archive restoration passes the original ID and unmodified multilingual input',async t=>{
  const {f,view,props,toggle} = setup(t);
  let restored;
  f.render(view,{...props,onRestoreArchive:value=>{restored=value;}});
  f.overrides.library_detail = async()=>({...props.detail,sources:[{id:'raw',capture:{text:'保留原话。\nOriginal words.',origin:{},created_at:1}}]});
  toggle('Sources');await f.settle();
  f.find(view,n=>n.type==='button' && f.text(n)==='Restore this input as the body').props.onClick();
  assert.equal(restored.id,'raw');assert.equal(restored.text,'保留原话。\nOriginal words.');
  assert.equal(f.calls.filter(c=>c.name==='library_action').length,0);
});

test('same-record refresh retains history review and disables stale restoration',async t=>{
  const {f,view,props,toggle} = setup(t);
  const historical = {id:'v0',title:'Earlier title',body:'Immutable earlier body',created_at:1,capture_ids:[],actor:'user'};
  f.overrides.library_detail = async()=>({...props.detail,history:[historical]});
  toggle('History');await f.settle();
  f.find(view,n=>n.type==='button' && f.text(n).startsWith('v1 ·')).props.onClick();await f.settle();
  const receipt = f.find(view,n=>n.type?.name==='OrganizationReceipt');
  let finish;
  f.overrides.library_detail = () => new Promise(resolve=>finish=resolve);
  f.render(view,{...props,revision:1});await f.settle();
  assert.equal(f.find(view,n=>n.type?.name==='OrganizationReceipt').type,receipt.type);
  assert(f.nodes(view.tree).some(n=>n.props.text==='Immutable earlier body'));
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='Restore this version').props.disabled,true);
  finish({...props.detail,history:[historical]});await f.settle();
  assert(f.nodes(view.tree).some(n=>n.props.text==='Immutable earlier body'));
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='Restore this version').props.disabled,false);
});

test('newer archive heads remain read-only until the displayed head catches up',async t=>{
  const {f,view,props,toggle} = setup(t);
  const newer = {...props.detail,current:{id:'v2'},sources:[{id:'raw',capture:{text:'Original input',origin:{},created_at:1}}],
    history:[{id:'v1',title:'Earlier title',body:'Earlier body',created_at:1,capture_ids:[],actor:'user'}]};
  f.overrides.library_detail=async()=>newer;
  toggle('Sources');await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='Restore this input as the body').props.disabled,true);
  toggle('History');await f.settle();
  // Select the new server head while the parent still displays v1.
  const newest = {...newer,history:[{...newer.history[0],id:'v2'}]};
  f.overrides.library_detail=async()=>newest;
  f.render(view,{...props,revision:1});await f.settle();
  f.find(view,n=>n.type==='button' && f.text(n).startsWith('v1 ·')).props.onClick();await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='Restore this version').props.disabled,true);
  toggle('Sources');await f.settle();
  f.render(view,{...props,detail:newer,revision:1});await f.settle();
  assert.equal(f.find(view,n=>n.type==='button' && f.text(n)==='Restore this input as the body').props.disabled,false);
  assert.equal(f.calls.filter(c=>c.name==='library_action').length,0);
});
