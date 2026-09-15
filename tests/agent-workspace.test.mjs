import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('a failed Agent capability check blocks model activation',async t=>{
 const f=workspaceFixture(t,{native:true});
 let binding={source:'service',base_url:'http://localhost:1234/v1',has_key:true,model:'QA',dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',llm:binding,embedding:binding,voice:binding,auto_organize:true};
 f.overrides.models_test=async()=>({token:'proof',binding,message:'model_test_agent_unsupported',message_params:{}});
 let applied=0;
 const view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,{kind:'llm',models,draft:binding,embedding:null,voice:null,setDraft:b=>{binding=b},onBusy(){},onSaved(){applied++},onRefresh:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Test connection").props.onClick();await f.settle();
 assert(f.text(view.tree).includes("This model did not pass Memivy's compatibility check"));assert.equal(applied,0);
 assert(f.find(view,n=>n.type==='button'&&n.props.className==='send-button').props.disabled);
 assert(!f.calls.some(c=>c.name==='models_apply'||c.name==='workspace_configure'));
 f.find(view,n=>n.type==='input'&&n.props.placeholder==="Enter a chat model ID").props.onChange({target:{value:'another'}});await f.settle();
 assert(!f.text(view.tree).includes("This model did not pass Memivy's compatibility check"));
});

test('a citation shows disjoint source windows separately without invented intervening text',async t=>{
 const f=workspaceFixture(t);const source={kind:'version',id:'v-a'};
 f.messages(()=>[{id:'answer-a',role:'assistant',status:'complete',text:'grounded',citations:[{source,available:true}],created_at:1}]);
 f.overrides.discussion_source=async()=>({source,title:'QA',text:'FIRST',start:0,truncated:true,current:true,recorded_at:1,additional_spans:[{start:6000,text:'LAST',truncated:true}]});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,{topic:f.topic,revision:1,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Evidence 1").props.onClick();await f.settle();
 const preview=f.find(view,n=>typeof n.type==='function'&&n.type.name==='SourcePreview');const modal=f.mount(preview.type,preview.props);await f.settle();
 assert(f.text(modal.tree).includes('FIRST'));assert(f.text(modal.tree).includes('LAST'));assert(f.text(modal.tree).includes("Another excerpt from the same source"));
 assert.equal(f.calls.filter(c=>c.name==='discussion_source').length,1);
});

test('voice settings entry opens the capability and navigation retains model drafts',async t=>{
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval(){}}});
 const binding={source:'local',base_url:'',has_key:false,model:'',dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 f.overrides.models_load=async()=>({revision:'r1',llm:null,embedding:binding,voice:binding,auto_organize:true});
 const view=f.mount(f.load('src/workspace/Settings.tsx').default,{initialPage:'voice',onClose(){},onChanged(){},onRestore(){}});await f.settle();
 const capability=()=>f.find(view,n=>typeof n.type==='function'&&n.type.name==='ModelCapability');
 assert.equal(capability().props.kind,'voice');
 capability().props.setDraft({...binding,source:'service',model:'unfinished-model'});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="General").props.onClick();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="AI & models").props.onClick();await f.settle();
 const Overview=f.load('src/workspace/ModelOverview.tsx').default;
 const overview=f.mount(Overview,f.find(view,n=>n.type===Overview).props);
 f.find(overview,n=>n.type==='button'&&n.props['aria-label']?.endsWith("Voice input")).props.onClick();await f.settle();
 assert.equal(capability().props.draft.model,'unfinished-model');
 assert(!f.calls.some(c=>c.name==='models_apply'));
});

test('each capability configures its own endpoint directly and failed tests never save it',async t=>{
 const f=workspaceFixture(t,{native:true});
 const original={source:'service',base_url:'http://localhost:1234/v1',has_key:true,model:'active-model',dimensions:4,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',llm:original,embedding:original,voice:original,auto_organize:true};
 for(const kind of ['llm','embedding','voice']){
  let sent;f.overrides.models_test=async args=>{sent=args;throw "Connection test failed";};
  let view;const props={kind,models,draft:{...original},embedding:null,voice:null,onBusy(){},onSaved(){assert.fail('must not apply')},onRefresh:async()=>{},setDraft(d){props.draft=d;f.render(view,props)},};
  view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
  assert(!f.nodes(view.tree).some(n=>n.type==='select'&&n.props.value==='active'));
  assert(!f.text(view.tree).includes("Add connection"));
  f.find(view,n=>n.type==='input'&&n.props.type==='url').props.onChange({target:{value:'http://localhost:4321/v1'}});await f.settle();
  f.find(view,n=>n.type==='input'&&n.props.type==='password').props.onChange({target:{value:'synthetic-key'}});await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==="Test connection").props.onClick();await f.settle();
  assert.equal(sent.binding.base_url,'http://localhost:4321/v1');assert.equal(sent.binding.api_key,'synthetic-key');assert.equal(sent.kind,kind);
  assert.equal(models[kind].base_url,'http://localhost:1234/v1');assert(f.nodes(view.tree).some(n=>n.props?.text==="Connection test failed"));
  assert(!f.calls.some(c=>c.name==='models_apply'||c.name==='models_connection'));f.unmount(view);
 }
});

test('source buttons preserve remote model drafts, including repeated selection',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',base_url:'http://localhost:1234/v1',has_key:true,model:'draft-model',dimensions:4,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',llm:null,embedding:binding,voice:binding,auto_organize:true};
 for(const kind of ['embedding','voice']){
  let edits=0,view;const props={kind,models,draft:{...binding,api_key:'draft-key'},embedding:null,voice:null,onBusy(){},setDraft(b){edits++;props.draft=b;f.render(view,props)}};
  view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
  const button=text=>f.find(view,n=>n.type==='button'&&f.text(n).includes(text));
  button("Use an API").props.onClick();await f.settle();assert.equal(edits,0);
  button("On this Mac").props.onClick();await f.settle();
  button("Use an API").props.onClick();await f.settle();
  assert.equal(props.draft.model,'draft-model');
  assert.equal(props.draft.api_key,'draft-key');f.unmount(view);
 }
});

test('failed activation reloads the actual revision and keeps the candidate draft',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',base_url:'http://localhost:1234/v1',has_key:true,model:'draft-model',dimensions:null,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',llm:null,embedding:binding,voice:binding,auto_organize:true};
 f.overrides.models_test=async()=>({token:'proof',binding,message:'model_test_agent',message_params:{}});
 f.overrides.models_apply=async()=>{throw 'activation failed'};
 f.overrides.models_load=async()=>({...models,revision:'rollback-revision'});
 let reconciled,view;const props={kind:'llm',models,draft:binding,embedding:null,voice:null,onBusy(){},onSaved(){assert.fail('must not claim saved')},onReconcile(m){reconciled=m},onRefresh:async()=>{},setDraft(b){props.draft=b;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Test connection").props.onClick();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Enable AI assistant").props.onClick();await f.settle();
 assert.equal(reconciled.revision,'rollback-revision');assert.equal(props.draft.model,'draft-model');
 assert(f.nodes(view.tree).some(n=>n.props?.text==='activation failed'));
});

test('changing only an embedding key applies without claiming the index will be rebuilt',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',base_url:'http://localhost:1234/v1',has_key:true,model:'same-model',dimensions:4,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',llm:null,embedding:binding,voice:binding,auto_organize:true};
 const tested={...binding};
 f.overrides.models_test=async()=>({token:'proof',binding:tested,message:'model_test_agent',message_params:{}});
 let applied=false;f.overrides.models_apply=async()=>{applied=true;return models};
 let view;const props={kind:'embedding',models,draft:{...binding,base_url:'http://localhost:1234/v1/',api_key:'fixed-key'},embedding:{enabled:true,preparing:false},voice:null,onBusy(){},onSaved(){},onRefresh:async()=>{},setDraft(b){props.draft=b;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Test connection").props.onClick();await f.settle();
 assert(!f.text(view.tree).includes("Change and rebuild"));
 f.find(view,n=>n.type==='button'&&f.text(n)==="Apply settings").props.onClick();await f.settle();assert(applied);
});
