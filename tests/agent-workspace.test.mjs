import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('candidate capability tests explain limitations without activating the model',async t=>{
 const f=workspaceFixture(t,{native:true});
 let binding={source:'service',connection:'qa',model:'QA',dimensions:null,query_prefix:'',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',connections:[{id:'qa',name:'QA',base_url:'http://localhost:1234/v1',has_key:false}],llm:binding,embedding:binding,voice:binding,auto_organize:true};
 f.overrides.models_test=async()=>({token:'proof',binding,message:'连接可用：支持基本工具调用，部分增强能力不可用'});
 let applied=0;
 const view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,{kind:'llm',models,draft:binding,embedding:null,voice:null,setDraft:b=>{binding=b},onBusy(){},onSaved(){applied++},onRefresh:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='测试连接').props.onClick();await f.settle();
 assert(f.text(view.tree).includes('部分增强能力不可用'));assert.equal(applied,0);
 assert(!f.calls.some(c=>c.name==='models_apply'||c.name==='workspace_configure'));
 f.find(view,n=>n.type==='input'&&n.props.placeholder==='填写对话模型 ID').props.onChange({target:{value:'another'}});await f.settle();
 assert(!f.text(view.tree).includes('连接可用：支持基本工具调用'));
});

test('a citation shows disjoint source windows separately without invented intervening text',async t=>{
 const f=workspaceFixture(t);const source={kind:'version',id:'v-a'};
 f.messages(()=>[{id:'answer-a',role:'assistant',status:'complete',text:'grounded',answer:{recollections:[{text:'grounded',sources:[source]}],ideas:'',conclusion:''},citations:[{source,available:true}],created_at:1}]);
 f.overrides.discussion_source=async()=>({source,title:'QA',text:'FIRST',start:0,truncated:true,current:true,recorded_at:1,additional_spans:[{start:6000,text:'LAST',truncated:true}]});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,{topic:f.topic,revision:1,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='查看依据').props.onClick();await f.settle();
 const preview=f.find(view,n=>typeof n.type==='function'&&n.type.name==='SourcePreview');const modal=f.mount(preview.type,preview.props);await f.settle();
 assert(f.text(modal.tree).includes('FIRST'));assert(f.text(modal.tree).includes('LAST'));assert(f.text(modal.tree).includes('另一处引用片段'));
 assert.equal(f.calls.filter(c=>c.name==='discussion_source').length,1);
});

test('voice settings entry opens the capability and navigation retains model drafts',async t=>{
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval(){}}});
 const binding={source:'local',connection:'',model:'',dimensions:null,query_prefix:'',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 f.overrides.models_load=async()=>({revision:'r1',connections:[],llm:null,embedding:binding,voice:binding,auto_organize:true});
 const view=f.mount(f.load('src/workspace/Settings.tsx').default,{initialPage:'voice',onClose(){},onChanged(){},onRestore(){}});await f.settle();
 const capability=()=>f.find(view,n=>typeof n.type==='function'&&n.type.name==='ModelCapability');
 assert.equal(capability().props.kind,'voice');
 capability().props.setDraft({...binding,source:'service',model:'unfinished-model'});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='快捷入口').props.onClick();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='AI 与模型').props.onClick();await f.settle();
 const sections=f.nodes(view.tree).filter(n=>n.type==='section'&&f.text(n).includes('语音输入'));
 const button=f.nodes(sections[0]).find(n=>n.type==='button');button.props.onClick();await f.settle();
 assert.equal(capability().props.draft.model,'unfinished-model');
 assert(!f.calls.some(c=>c.name==='models_apply'));
});

test('each capability configures its own endpoint directly and failed tests never save it',async t=>{
 const f=workspaceFixture(t,{native:true});
 const original={source:'service',connection:'active',model:'active-model',dimensions:4,query_prefix:'',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',connections:[{id:'active',name:'Existing',base_url:'http://localhost:1234/v1',has_key:true}],llm:original,embedding:original,voice:original,auto_organize:true};
 for(const kind of ['llm','embedding','voice']){
  let sent;f.overrides.models_test=async args=>{sent=args;throw '测试连接失败';};
  let view;const props={kind,models,draft:{...original},connectionDraft:null,embedding:null,voice:null,onBusy(){},onSaved(){assert.fail('must not apply')},onRefresh:async()=>{},setDraft(d){props.draft=d;f.render(view,props)},setConnectionDraft(d){props.connectionDraft=d;f.render(view,props)}};
  view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
  assert(!f.nodes(view.tree).some(n=>n.type==='select'&&n.props.value==='active'));
  assert(!f.text(view.tree).includes('添加连接'));
  f.find(view,n=>n.type==='input'&&n.props.type==='url').props.onChange({target:{value:'http://localhost:4321/v1'}});await f.settle();
  f.find(view,n=>n.type==='input'&&n.props.type==='password').props.onChange({target:{value:'synthetic-key'}});await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='测试连接').props.onClick();await f.settle();
  assert.equal(sent.connection.base_url,'http://localhost:4321/v1');assert.equal(sent.connection.api_key,'synthetic-key');assert.equal(sent.kind,kind);
  assert.equal(models[kind].connection,'active');assert(f.nodes(view.tree).some(n=>n.props?.text==='测试连接失败'));
  assert(!f.calls.some(c=>c.name==='models_apply'||c.name==='models_connection'));f.unmount(view);
 }
});

test('source buttons preserve remote model and query drafts, including repeated selection',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',connection:'qa',model:'draft-model',dimensions:4,query_prefix:'query: ',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',connections:[],llm:null,embedding:binding,voice:binding,auto_organize:true};
 for(const kind of ['embedding','voice']){
  let edits=0,view;const props={kind,models,draft:{...binding},connectionDraft:{id:'',base_url:'http://localhost:1234/v1',api_key:'draft-key'},embedding:null,voice:null,onBusy(){},setDraft(b){edits++;props.draft=b;f.render(view,props)}};
  view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
  const button=text=>f.find(view,n=>n.type==='button'&&f.text(n).includes(text));
  button('连接模型服务').props.onClick();await f.settle();assert.equal(edits,0);
  button('内置本地模型').props.onClick();await f.settle();
  button('连接模型服务').props.onClick();await f.settle();
  assert.equal(props.draft.model,'draft-model');assert.equal(props.draft.query_prefix,'query: ');
  assert.equal(props.connectionDraft.api_key,'draft-key');f.unmount(view);
 }
});

test('failed activation reloads the actual revision and keeps the candidate draft',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',connection:'qa',model:'draft-model',dimensions:null,query_prefix:'',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',connections:[{id:'qa',name:'QA',base_url:'http://localhost:1234/v1'}],llm:null,embedding:binding,voice:binding,auto_organize:true};
 f.overrides.models_test=async()=>({token:'proof',binding,message:'测试通过'});
 f.overrides.models_apply=async()=>{throw 'activation failed'};
 f.overrides.models_load=async()=>({...models,revision:'rollback-revision'});
 let reconciled,view;const props={kind:'llm',models,draft:binding,embedding:null,voice:null,onBusy(){},onSaved(){assert.fail('must not claim saved')},onReconcile(m){reconciled=m},onRefresh:async()=>{},setDraft(b){props.draft=b;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='测试连接').props.onClick();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='启用问答与整理').props.onClick();await f.settle();
 assert.equal(reconciled.revision,'rollback-revision');assert.equal(props.draft.model,'draft-model');
 assert(f.nodes(view.tree).some(n=>n.props?.text==='activation failed'));
});

test('changing only an embedding key applies without claiming the index will be rebuilt',async t=>{
 const f=workspaceFixture(t,{native:true});
 const binding={source:'service',connection:'qa',model:'same-model',dimensions:4,query_prefix:'',disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens'};
 const models={revision:'r1',connections:[{id:'qa',name:'QA',base_url:'http://localhost:1234/v1',has_key:true}],llm:null,embedding:binding,voice:binding,auto_organize:true};
 const tested={...binding,connection:'candidate'};
 f.overrides.models_test=async()=>({token:'proof',binding:tested,message:'测试通过'});
 let applied=false;f.overrides.models_apply=async()=>{applied=true;return models};
 let view;const props={kind:'embedding',models,draft:binding,connectionDraft:{id:'qa',name:'QA',base_url:'http://localhost:1234/v1/',api_key:'fixed-key',remove:false},embedding:{enabled:true,preparing:false},voice:null,onBusy(){},onSaved(){},onRefresh:async()=>{},setDraft(b){props.draft=b;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='测试连接').props.onClick();await f.settle();
 assert(!f.text(view.tree).includes('更换并重建'));
 f.find(view,n=>n.type==='button'&&f.text(n)==='应用设置').props.onClick();await f.settle();assert(applied);
});
