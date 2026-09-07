import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const props = f => ({topic:f.topic,revision:0,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
const textarea = (f,view) => f.find(view,n=>n.type==='textarea');
function send(f,view) { textarea(f,view).props.onKeyDown({key:'Enter',shiftKey:false,nativeEvent:{isComposing:false},keyCode:13,preventDefault(){}}); }

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
  f.find(app,n=>n.type==='button'&&f.text(n)==='记忆库').props.onClick();await f.settle();
  const select=async id=>{f.find(app,n=>n.type==='button'&&n.key===`memory:${id}`).props.onClick();await f.settle();};
  await select('a');
  const receipt={request_id:'edit-a',memory_id:'a',capture_id:'raw-a',before_version:'v-a0',after_version:'v-a1',action:'edit'};
  f.find(app,n=>n.type===Detail).props.onChanged(f.keyA,receipt);await f.settle();
  await select('b');
  const b=f.mount(Detail,f.find(app,n=>n.type===Detail).props);await f.settle();
  assert(!f.nodes(b.tree).some(n=>n.type==='button'&&f.text(n)==='撤销这次修改'));f.unmount(b);
  await select('a');const a=f.mount(Detail,f.find(app,n=>n.type===Detail).props);await f.settle();
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

test('an invalid editor draft does not prevent submitting a valid home capture', async t => {
  const f=workspaceFixture(t), {useDraft}=f.load('src/workspace/useDraft.ts');
  const a=f.mount(()=>useDraft('memory:a',{title:'',body:'',expected_version:null}));await f.settle();
  a.tree.update({title:'中文'.repeat(40),body:'无效标题的草稿'});await f.settle();f.unmount(a);
  const App=f.load('src/workspace/App.tsx').default, app=f.mount(App);await f.settle();
  const formNode=f.find(app,n=>typeof n.type==='function'&&n.props.mode==='capture');
  const form=f.mount(formNode.type,formNode.props);await f.settle();
  f.overrides.library_capture=async ({request})=>({id:'saved',text:request.text});
  textarea(f,form).props.onChange({target:{value:'有效的首页记录'}});await f.settle();
  const button=f.find(form,n=>n.type==='button'&&n.props.className==='send-button');button.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_capture').at(-1).args.request.text,'有效的首页记录');
  assert.equal(textarea(f,form).props.value,'');
});
