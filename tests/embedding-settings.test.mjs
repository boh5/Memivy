import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
const initial={enabled:false,preparing:false,paused:false,state:'not_downloaded',downloaded:0,bytes:639150592,processed:0,total:0,failed:0,error:null};
async function setup(t,state){
 const f=workspaceFixture(t,{native:true});
 const {emptyBinding}=f.load('src/workspace/modelTypes.ts');
 const binding=emptyBinding(),models={revision:'r1',llm:null,embedding:binding,voice:binding,auto_organize:true};
 let view;
 const props={kind:'embedding',models,draft:binding,embedding:state,voice:null,onBusy(){},onSaved(){},onRefresh:async()=>{f.render(view,props)},setDraft(draft){props.draft=draft;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 return {f,view,props,button:text=>f.find(view,n=>n.type==='button'&&f.text(n)===text)};
}
test('an existing shared model can be enabled without another download',async t=>{
 const {button}=await setup(t,{...initial,state:'disabled',downloaded:initial.bytes});
 assert.equal(button("Enable Semantic search").props.disabled,false);
});
test('confirmed model preparation continues after closing its settings',async t=>{
 const {f,view,props,button}=await setup(t,{...initial});
 f.overrides.models_apply=async args=>{
  assert.equal(args.kind,'embedding');assert.equal(args.confirmed,true);assert.equal(args.binding.source,'local');
  props.embedding={...initial,preparing:true,state:'downloading'};return props.models;
 };
 button("Download and enable").props.onClick();await f.settle();
 assert(!f.calls.some(c=>c.name==='models_apply'));
 button("Confirm and start").props.onClick();await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='progress'));
 f.unmount(view);
 assert.equal(f.calls.filter(c=>c.name==='models_apply').length,1);
 assert(!f.calls.some(c=>c.name==='embedding_control'));
});
test('paused downloads resume without a new model choice and failures remain retryable',async t=>{
 const {f,props,button}=await setup(t,{...initial,paused:true,preparing:true,state:'paused',downloaded:300000000});
 f.overrides.embedding_control=async({action})=>{
  assert.equal(action,'resume');props.embedding={...props.embedding,paused:false,state:'failed',error:'model_cache_mismatch'};
 };
 button("Resume").props.onClick();await f.settle();
 assert.equal(button("Retry").props.disabled,false);
 assert(!f.calls.some(c=>c.name==='models_apply'));
});
test('partial indexing failures expose a count and retry on the active capability page',async t=>{
 const {f,view,button}=await setup(t,{...initial,enabled:true,state:'ready',processed:9,total:10,failed:1});
 f.overrides.embedding_control=async({action})=>assert.equal(action,'retry');
 assert(f.text(view.tree).includes("1 memory could not be processed"));
 button("Retry").props.onClick();await f.settle();
 assert.equal(f.calls.filter(c=>c.name==='embedding_control').length,1);
});

for(const state of ['indexing','paused','disabled'])test(`partial failures preserve the ${state} state and its controls`,async t=>{
 const preparing=state!=='disabled',paused=state==='paused';
 const {f,props,button}=await setup(t,{...initial,state,preparing,paused,failed:1,processed:1,total:10});
 const {indexLabel}=f.load('src/workspace/modelTypes.ts');
 assert.equal(indexLabel(props.embedding),state==='indexing'?"Preparing memories for search":paused?"Paused":"Disabled");
 assert.equal(button(state==='indexing'?"Pause":paused?"Resume":"Download and enable").props.disabled,false);
});
