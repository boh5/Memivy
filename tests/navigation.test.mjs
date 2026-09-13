import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const input=(f,v)=>f.find(v,n=>n.type==='textarea');

test('browsing selection or discussions does not refetch the paged list',async t=>{
  const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
  const props={trash:false,active:true,selected:null,revision:0,onSelect(){},onCapture(){},onRefresh(){}};
  const view=f.mount(List,props);await f.settle();
  for(let i=0;i<20;i++){f.render(view,{...props,active:i%2===0,selected:f.keyA});await f.settle();}
  const queries=f.calls.filter(c=>c.name==='library_query');assert.equal(queries.length,1);assert.equal(queries[0].args.query.limit,40);assert.equal(queries[0].args.query.query,'');
});

test('a stale list request cannot replace a newer filter result',async t=>{
  const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;const resolves=[];
  f.overrides.library_query=()=>new Promise(r=>resolves.push(r));
  const props={trash:false,active:true,selected:null,revision:0,onSelect(){},onCapture(){},onRefresh(){}};
  const view=f.mount(List,props);await f.settle();f.render(view,{...props,revision:1});await f.settle();
  resolves[1]({items:[{key:f.keyB,title:'新的结果',snippet:'',origin:null,updated_at:1}],next_offset:null});await f.settle();
  resolves[0]({items:[{key:f.keyA,title:'迟到旧结果',snippet:'',origin:null,updated_at:1}],next_offset:null});await f.settle();
  assert(f.text(view.tree).includes('新的结果'));assert(!f.text(view.tree).includes('迟到旧结果'));
});

test('query handoff reads the existing quick-question draft before acknowledging',async t=>{
  const f=workspaceFixture(t,{native:true}),App=f.load('src/workspace/App.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.backup_result=async()=>null;f.overrides.desktop_ready=async()=>{};f.overrides.desktop_state=async()=>null;f.overrides.desktop_handoff_ready=async()=>{};
  f.db.set('quick_input',{key:'quick_input',request_id:'handoff-query',title:'',body:'小窗里没问完的问题',expected_version:null});
  const app=f.mount(App);await f.settle();
  f.emit('desktop-route',{generation:42,quick:true,topic:null,record:null,settings:false});await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_handoff_ready').length,0);
  const form=f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  assert.equal(input(f,form).props.value,'小窗里没问完的问题');
  assert.equal(f.calls.filter(c=>c.name==='desktop_handoff_ready').length,1);
  assert.equal(f.calls.find(c=>c.name==='desktop_handoff_ready').args.generation,42);
});

test('new desktop questions do not inherit the collection open in the main window',async t=>{
  const f=workspaceFixture(t,{native:true}),App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.backup_result=async()=>null;f.overrides.desktop_ready=async()=>{};f.overrides.desktop_state=async()=>null;
  f.overrides.desktop_update=async()=>null;f.overrides.discussion_submit=async()=>f.topic;
  f.overrides.navigation_collections=async()=>[{id:'scope',name:'产品',revision:1}];
  const app=f.mount(App);await f.settle();
  f.find(app,n=>n.type===Sidebar).props.onCollection('scope');await f.settle();
  assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('重点参考 产品'));
  f.emit('desktop-route',{generation:43,quick:true,topic:null,record:null,settings:false});await f.settle();
  assert(!f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('重点参考 产品'));
  await f.find(f.query(app),n=>n.type===Form).props.onSubmit({text:'全库问题',id:'desktop-question',context:[]});await f.settle();
  assert.equal(f.calls.find(c=>c.name==='discussion_submit').args.collectionId,null);
});

test('native initialization errors keep language recovery mounted in the main window',async t=>{
  const LanguageRecovery = () => null;
  const f=workspaceFixture(t,{native:true,modules:{'./LanguageRecovery':{default:LanguageRecovery}}}),App=f.load('src/workspace/App.tsx').default;
  f.overrides.backup_result=async()=>null;f.overrides.desktop_state=async()=>null;
  const app=f.mount(App);await f.settle();
  assert(f.nodes(app.tree).some(n=>n.type===LanguageRecovery));
});

test('generated topic titles refresh the open discussion without replacing its composer draft',async t=>{
  let revision=0;
  const f=workspaceFixture(t,{modules:{'./resources':{useResourceVersion:()=>revision,useResourceBridge(){},expireQueries:async()=>{}}}});
  const App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Discussion=f.load('src/workspace/Discussion.tsx').default;
  let topic={...f.topic,title:'New discussion'};
  f.overrides.library_topics=async()=>[topic];
  const app=f.mount(App);await f.settle();
  f.find(app,n=>n.type===Sidebar).props.onTopic(topic);await f.settle();
  const before=f.find(app,n=>n.type===Discussion);
  const discussion=f.mount(Discussion,before.props);await f.settle();
  const composer=f.composer(discussion);await f.settle();
  input(f,composer).props.onChange({target:{value:'还没有发送的下一句'}});await f.settle();
  topic={...topic,title:'每周二十分钟的折纸练习'};revision++;
  f.render(app,{});await f.settle();
  const after=f.find(app,n=>n.type===Discussion);
  assert.equal(after.key,before.key);assert.equal(after.props.topic.title,topic.title);
  f.render(discussion,after.props);await f.settle();
  assert.equal(input(f,f.composer(discussion)).props.value,'还没有发送的下一句');
});

for (const quick of [false, true]) test(`discussion submission uses the ${quick ? 'handed-off application' : 'main window'} as its source`,async t=>{
  const f=workspaceFixture(t,{native:true}),App=f.load('src/workspace/App.tsx').default;
  const Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Discussion=f.load('src/workspace/Discussion.tsx').default;
  const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
  f.overrides.backup_result=async()=>null;f.overrides.desktop_ready=async()=>{};
  f.overrides.desktop_state=async()=>({...previewDesktop,source_app:'Safari'});
  f.overrides.desktop_handoff_ready=async()=>{};
  f.overrides.discussion_submit=async()=>f.topic;
  const app=f.mount(App);await f.settle();
  if(quick) f.emit('desktop-route',{generation:44,quick:true,topic:f.topic,record:null,settings:false});
  else f.find(app,n=>n.type===Sidebar).props.onTopic(f.topic);
  await f.settle();
  const discussion=f.mount(Discussion,f.find(app,n=>n.type===Discussion).props);await f.settle();
  const composer=f.composer(discussion);await f.settle();
  input(f,composer).props.onChange({target:{value:'每周安排二十分钟练习。'}});await f.settle();
  await f.find(composer,n=>n.type==='button'&&n.props['aria-label']==='发送').props.onClick();await f.settle();
  assert.equal(f.calls.find(c=>c.name==='discussion_submit').args.origin.app,quick?'Safari':'Memivy');
});

for (const target of ['topic','library']) test(`local ${target} navigation ends quick-entry submission ownership and preserves the handoff draft`,async t=>{
  const f=workspaceFixture(t,{native:true}),App=f.load('src/workspace/App.tsx').default;
  const Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Discussion=f.load('src/workspace/Discussion.tsx').default;
  const Form=f.load('src/workspace/CaptureForm.tsx').default,{previewDesktop}=f.load('src/workspace/desktopApi.ts');
  f.overrides.backup_result=async()=>null;f.overrides.desktop_ready=async()=>{};
  f.overrides.desktop_state=async()=>({...previewDesktop,topic:f.topic,source_app:'Safari'});
  f.overrides.desktop_handoff_ready=async()=>{};f.overrides.desktop_update=async()=>previewDesktop;
  f.overrides.discussion_submit=async()=>({...f.topic,id:'other-topic'});
  const original={key:`discussion:${f.topic.id}`,request_id:'handoff-original',title:'',body:'小窗未发送的原话',expected_version:null,origin:{kind:'user',app:'Safari'}};
  f.db.set(original.key,structuredClone(original));
  const app=f.mount(App);await f.settle();
  f.emit('desktop-route',{generation:45,quick:true,topic:f.topic,record:null,settings:false});await f.settle();
  const navigation=f.find(app,n=>n.type===Sidebar).props;
  if(target==='topic') navigation.onTopic({...f.topic,id:'other-topic'});else navigation.onLibrary();
  await f.settle();
  const surface=target==='topic'?f.mount(Discussion,f.find(app,n=>n.type===Discussion).props):null;
  if(surface) await f.settle();
  const composer=surface?f.composer(surface):f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  input(f,composer).props.onChange({target:{value:'这条表达来自主窗口。'}});await f.settle();
  await f.find(composer,n=>n.type==='button'&&n.props['aria-label']==='发送').props.onClick();await f.settle();
  const sent=f.calls.find(c=>c.name==='discussion_submit');
  assert.equal(sent.args.quick,false);assert.equal(sent.args.origin.app,'Memivy');
  assert(!f.calls.some(c=>c.name==='desktop_update'),'local input must not replace the quick-entry topic');
  assert.deepEqual(f.db.get(original.key),original);
});
