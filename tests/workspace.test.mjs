import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const props = f => ({topic:f.topic,revision:0,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
const textarea = (f,view) => f.find(view,n=>n.type==='textarea');
function send(f,view) { textarea(f,view).props.onKeyDown({key:'Enter',metaKey:true,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,nativeEvent:{isComposing:false},keyCode:13,preventDefault(){}}); }

const captureProps = sourceApp => ({quick:true,mode:'capture',sourceApp,focus:1,onSaved(){},onAsk:async()=>{},onMode(){},onEdit(){}});

test('repeated empty handoffs use the new source and reset it after a capture', async t => {
  const f=workspaceFixture(t), Capture=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.desktop_capture=async ({request})=>({id:request.request_id,text:request.text});
  const view=f.mount(Capture,captureProps('Safari'));await f.settle();
  for (const source of ['Notes','Chrome']) {
    // The main-window form stays mounted through successive handoffs.
    f.render(view,{...captureProps(source),focus:2});await f.settle();
    textarea(f,view).props.onChange({target:{value:'交接后输入的原话'}});await f.settle();
    assert.equal(f.db.get('quick_capture').origin.app,source);
    send(f,view);await f.settle();
    assert.equal(f.calls.filter(c=>c.name==='desktop_capture').at(-1).args.request.origin.app,source);
    assert.equal(textarea(f,view).props.value,'');
    assert(!f.db.has('quick_capture'));
  }
});

test('restored drafts retain their source and explicit URI even with an empty body', async t => {
  const f=workspaceFixture(t), Capture=f.load('src/workspace/CaptureForm.tsx').default;
  const origin={kind:'user',app:'Safari',project:null,uri:'https://example.com/selected'};
  f.db.set('quick_capture',{key:'quick_capture',request_id:'existing',title:'',body:'',expected_version:null,origin});
  const view=f.mount(Capture,captureProps('Notes'));await f.settle();
  f.render(view,captureProps('Chrome'));await f.settle();
  textarea(f,view).props.onChange({target:{value:'恢复后补写'}});await f.settle();
  assert.deepEqual(f.db.get('quick_capture').origin,origin);
});

test('a source refresh cannot relabel a draft whose first write is pending', async t => {
  const f=workspaceFixture(t), Capture=f.load('src/workspace/CaptureForm.tsx').default;
  let finish;
  f.overrides.draft_write=({draft})=>new Promise(resolve=>{finish=()=>{f.db.set(draft.key,structuredClone(draft));resolve(true);};});
  const view=f.mount(Capture,captureProps('Safari'));await f.settle();
  textarea(f,view).props.onChange({target:{value:'正在保存的原话'}});await f.settle();
  f.render(view,captureProps('Notes'));await f.settle();finish();await f.settle();
  assert.equal(f.db.get('quick_capture').origin.app,'Safari');
  assert.equal(textarea(f,view).props.value,'正在保存的原话');
});

test('late discussion acknowledgement preserves the reopened input and restart draft', async t => {
  const f=workspaceFixture(t), Discussion=f.load('src/workspace/Discussion.tsx').default;
  const old=f.mount(Discussion,props(f));await f.settle();
  textarea(f,old).props.onChange({target:{value:'已发送的问题'}});await f.settle();send(f,old);await f.settle();
  f.unmount(old);
  const reopened=f.mount(Discussion,props(f));await f.settle();
  textarea(f,reopened).props.onChange({target:{value:'切回来后的新草稿'}});await f.settle();
  f.completeAsk();await f.settle();
  assert.equal(textarea(f,reopened).props.value,'切回来后的新草稿');
  await f.load('src/workspace/useDraft.ts').flushDrafts();
  assert.equal(f.db.get('discussion:topic-a').body,'切回来后的新草稿');
});

test('late acknowledgement clears the same sent draft in a reopened input', async t => {
  const f=workspaceFixture(t), Discussion=f.load('src/workspace/Discussion.tsx').default;
  const old=f.mount(Discussion,props(f));await f.settle();
  textarea(f,old).props.onChange({target:{value:'问题'}});await f.settle();send(f,old);await f.settle();f.unmount(old);
  const reopened=f.mount(Discussion,props(f));await f.settle();f.completeAsk();await f.settle();
  assert.equal(textarea(f,reopened).props.value,'');assert.equal(f.db.get('discussion:topic-a').body,'');
});

test('a receipt is shown only for its record and undo targets that record', async t => {
  const f=workspaceFixture(t), App=f.load('src/workspace/App.tsx').default, Detail=f.load('src/workspace/MemoryDetail.tsx').default;
  const app=f.mount(App);await f.settle();
  const List=f.load('src/workspace/MemoryList.tsx').default;
  const select=async id=>{f.find(app,n=>n.type===List).props.onSelect({kind:'memory',id});await f.settle();};
  await select('a');
  const receipt={request_id:'edit-a',memory_id:'a',capture_id:'raw-a',before_version:'v-a0',after_version:'v-a1',action:'edit'};
  f.find(app,n=>n.type===Detail).props.onChanged(f.keyA,receipt);await f.settle();
  await select('b');
  const b=f.mount(Detail,f.find(app,n=>n.type===Detail).props);await f.settle();
  f.find(b,n=>n.props.role==='tab'&&f.text(n).startsWith('版本历史')).props.onClick();await f.settle();
  assert(!f.nodes(b.tree).some(n=>n.type==='button'&&f.text(n)==='撤销这次修改'));f.unmount(b);
  await select('a');const a=f.mount(Detail,f.find(app,n=>n.type===Detail).props);await f.settle();
  f.find(a,n=>n.props.role==='tab'&&f.text(n).startsWith('版本历史')).props.onClick();await f.settle();
  f.find(a,n=>n.type==='button'&&f.text(n)==='撤销这次修改').props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_action').at(-1).args.action.original_request,'edit-a');
  await select('b');await select('a');assert.equal(f.find(app,n=>n.type===Detail).props.initialReceipt,null);
});

test('loading older messages preserves the visible message and new replies still scroll', async t => {
  const f=workspaceFixture(t), Discussion=f.load('src/workspace/Discussion.tsx').default;
  const msg=i=>({id:'m'+i,seq:i,role:i%2?'user':'assistant',status:'complete',text:'消息'+i,citations:[]});
  f.messages(args=>Array.from({length:args.before?10:40},(_,i)=>msg(i+(args.before?1:11))));
  const view=f.mount(Discussion,props(f));await f.settle();view.dom.scrolls=0;view.dom.list.scrollTop=500;
  const anchor=view.dom.ordered[5], top=anchor.getBoundingClientRect().top;
  f.find(view,n=>n.type==='button'&&f.text(n)==='查看更早的讨论').props.onClick();await f.settle();
  assert.equal(view.dom.scrolls,0);assert.equal(anchor.getBoundingClientRect().top,top);assert.equal(view.dom.list.scrollTop,1500);
  f.messages(()=>[msg(51)]);f.render(view,{...props(f),revision:1});await f.settle();assert.equal(view.dom.scrolls,1);
});

test('an invalid editor draft does not prevent submitting a valid quick capture', async t => {
  const f=workspaceFixture(t), {useDraft}=f.load('src/workspace/useDraft.ts');
  const a=f.mount(()=>useDraft('memory:a',{title:'',body:'',expected_version:null}));await f.settle();
  a.tree.update({title:'中文'.repeat(40),body:'无效标题的草稿'});await f.settle();f.unmount(a);
  const App=f.load('src/workspace/App.tsx').default, app=f.mount(App);await f.settle();
  f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.onCapture();await f.settle();
  const Dialog=f.load('src/workspace/CaptureDialog.tsx').default;
  const dialog=f.mount(Dialog,f.find(app,n=>n.type===Dialog).props);await f.settle();
  const formNode=f.find(dialog,n=>typeof n.type==='function'&&n.props.mode==='capture');
  const form=f.mount(formNode.type,formNode.props);await f.settle();
  f.overrides.library_capture=async ({request})=>({id:'saved',text:request.text});
  textarea(f,form).props.onChange({target:{value:'有效的首页记录'}});await f.settle();
  const button=f.find(form,n=>n.type==='button'&&n.props.className==='send-button');button.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_capture').at(-1).args.request.text,'有效的首页记录');
  assert.equal(textarea(f,form).props.value,'');
});

test('the collapsed leaf supports accessible activation and dragging never opens it', async t => {
  const f=workspaceFixture(t);
  f.load('src/workspace/desktopApi.ts').previewDesktop.expanded=false;
  f.overrides.desktop_open=async()=>{};
  const Desktop=f.load('src/workspace/Desktop.tsx').default, view=f.mount(Desktop);await f.settle();
  const leaf=f.find(view,n=>n.type==='button' && n.props.className?.startsWith('desktop-leaf'));
  leaf.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_open').length,1);
  const element={setPointerCapture(){},hasPointerCapture(){return true;},releasePointerCapture(){}};
  const event={button:0,pointerId:1,clientX:10,clientY:10,currentTarget:element,target:{closest(){return element;}}};
  leaf.props.onPointerDown(event);
  leaf.props.onPointerMove({...event,clientX:40});
  leaf.props.onPointerUp({...event,clientX:40,type:'pointerup'});
  leaf.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_open').length,1);
  leaf.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_open').length,2);
});
