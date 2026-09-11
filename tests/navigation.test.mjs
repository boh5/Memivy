import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
import { isRecallSubmitKey } from '../src/workspace/keyboard.ts';

const input=(f,v)=>f.find(v,n=>n.type==='textarea');
const enter={key:'Enter',metaKey:false,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,keyCode:13,nativeEvent:{isComposing:false},preventDefault(){}};
const captureProps={mode:'capture',presentation:'capture',focus:1,onSaved(){},onAsk:async()=>{},onMode(){},onEdit(){}};
const queryProps={...captureProps,mode:'ask',presentation:'query'};

test('recall Return submits, Shift Return and IME confirmation do not',()=>{
  assert.equal(isRecallSubmitKey(enter),true);
  assert.equal(isRecallSubmitKey({...enter,metaKey:true}),true);
  for(const patch of [{shiftKey:true},{ctrlKey:true},{altKey:true},{repeat:true},{isComposing:true},{keyCode:229}])
    assert.equal(isRecallSubmitKey({...enter,...patch}),false);
  assert.equal(isRecallSubmitKey(enter,true),false);
});

test('typing never starts RAG; Return starts one discussion and never captures the query',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  const app=f.mount(App);await f.settle();
  const form=f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  input(f,form).props.onChange({target:{value:'我为什么决定先做桌面版？'}});await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='discussion_ask').length,0);
  assert.equal(f.db.get('question').body,'我为什么决定先做桌面版？');
  input(f,form).props.onKeyDown(enter);input(f,form).props.onKeyDown(enter);await f.settle();
  const asks=f.calls.filter(c=>c.name==='discussion_ask');assert.equal(asks.length,1);
  assert.equal(asks[0].args.context.length,0);f.completeAsk();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_capture').length,0);
  assert.equal(f.db.has('question'),false);
  assert(f.nodes(app.tree).some(n=>n.props.topic?.id==='topic-a'));
});

test('failed recall keeps its draft and retries the same logical question',async t=>{
  const f=workspaceFixture(t),Form=f.load('src/workspace/CaptureForm.tsx').default;
  const ids=[];const form=f.mount(Form,{...queryProps,onAsk:async(_,id)=>{ids.push(id);throw '暂时断开';}});await f.settle();
  input(f,form).props.onChange({target:{value:'过去的决定'}});await f.settle();
  for(let i=0;i<2;i++){input(f,form).props.onKeyDown(enter);await f.settle();}
  assert.equal(input(f,form).props.value,'过去的决定');assert.equal(ids.length,2);assert.equal(ids[0],ids[1]);
  f.unmount(form);const reopened=f.mount(Form,queryProps);await f.settle();assert.equal(input(f,reopened).props.value,'过去的决定');
});

test('completed capture restores input focus while a query leaves focus with discussion',async t=>{
  const f=workspaceFixture(t),Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.library_capture=async()=>({id:'saved'});
  for(const props of [captureProps,{...queryProps,onAsk:async()=>{}}]) {
    const form=f.mount(Form,props);await f.settle();let enabledFocus=0;
    input(f,form).props.ref.current.focus=()=>{if(!input(f,form).props.disabled)enabledFocus++;};
    input(f,form).props.onChange({target:{value:'焦点验证'}});await f.settle();
    input(f,form).props.onKeyDown({...enter,metaKey:true});await f.settle();
    assert.equal(enabledFocus>0,props.mode==='capture');f.unmount(form);
  }
});

test('capture dialog preserves the selected record and recall draft when saving',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,List=f.load('src/workspace/MemoryList.tsx').default,Dialog=f.load('src/workspace/CaptureDialog.tsx').default,Detail=f.load('src/workspace/MemoryDetail.tsx').default;
  const app=f.mount(App);await f.settle();f.find(app,n=>n.type===List).props.onSelect(f.keyA);await f.settle();
  const question={key:'question',request_id:'q',title:'',body:'还没提问的草稿',expected_version:null};f.db.set('question',question);
  f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.onCapture();await f.settle();
  assert.equal(f.find(app,n=>n.type===Detail).props.record.id,'a');
  f.find(app,n=>n.type===Dialog).props.onSaved({kind:'capture',id:'new'});await f.settle();
  assert.equal(f.find(app,n=>n.type===Detail).props.record.id,'a');
  assert.equal(f.db.get('question').body,'还没提问的草稿');assert(!f.nodes(app.tree).some(n=>n.type===Dialog));
});

test('closing capture flushes its own draft, ignores unrelated failures, and waits for submission',async t=>{
  const f=workspaceFixture(t),Dialog=f.load('src/workspace/CaptureDialog.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  const {useDraft}=f.load('src/workspace/useDraft.ts');let closed=0;
  const bad=f.mount(()=>useDraft('memory:a',{title:'',body:'',expected_version:null}));await f.settle();bad.tree.update({title:'中文'.repeat(101)});await f.settle();
  const dialog=f.mount(Dialog,{quick:false,focus:1,onReady(){},onSaved(){},onClose(){closed++;}});await f.settle();
  const formNode=f.find(dialog,n=>n.type===Form);const form=f.mount(Form,formNode.props);await f.settle();
  input(f,form).props.onChange({target:{value:'收起后还在'}});await f.settle();
  formNode.props.onBusy(true);dialog.tree.props.onClose();await f.settle();assert.equal(closed,0);
  formNode.props.onBusy(false);dialog.tree.props.onClose();await f.settle();assert.equal(closed,1);assert.equal(f.db.get('capture').body,'收起后还在');
});

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
  f.db.set('quick_question',{key:'quick_question',request_id:'handoff-query',title:'',body:'小窗里没问完的问题',expected_version:null});
  const app=f.mount(App);await f.settle();
  f.emit('desktop-route',{generation:42,quick:true,mode:'ask',topic:null,record:null,settings:false});await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_handoff_ready').length,0);
  const form=f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  assert.equal(input(f,form).props.value,'小窗里没问完的问题');
  assert.equal(f.calls.filter(c=>c.name==='desktop_handoff_ready').length,1);
  assert.equal(f.calls.find(c=>c.name==='desktop_handoff_ready').args.generation,42);
});

test('new desktop questions do not inherit the collection open in the main window',async t=>{
  const f=workspaceFixture(t,{native:true}),App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.backup_result=async()=>null;f.overrides.desktop_ready=async()=>{};f.overrides.desktop_state=async()=>null;
  f.overrides.desktop_update=async()=>null;f.overrides.discussion_ask=async()=>f.topic;
  f.overrides.navigation_collections=async()=>[{id:'scope',name:'产品',revision:1}];
  const app=f.mount(App);await f.settle();
  f.find(app,n=>n.type===Sidebar).props.onCollection('scope');await f.settle();
  assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('仅在 产品 中提问'));
  f.emit('desktop-route',{generation:43,quick:true,mode:'ask',topic:null,record:null,settings:false});await f.settle();
  assert(!f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('仅在 产品 中提问'));
  await f.find(f.query(app),n=>n.type===Form).props.onAsk('全库问题','desktop-question');await f.settle();
  assert.equal(f.calls.find(c=>c.name==='discussion_ask').args.collectionId,null);
});
