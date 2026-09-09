import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const key={key:'Enter',metaKey:false,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,keyCode:13,nativeEvent:{isComposing:false},preventDefault(){},stopPropagation(){}};

test('dismiss/reopen keeps the mounted question draft, and successful submission closes the panel',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  const app=f.mount(App);await f.settle();
  const bar=()=>f.find(app,n=>n.type===Bar).props;
  const props=()=>f.find(app,n=>n.type===Form).props;
  assert.equal(bar().open,false);assert.equal(props().focus,0);
  bar().onOpen();await f.settle();
  const form=f.mount(Form,props());await f.settle();
  const input=()=>f.find(form,n=>n.type==='textarea');
  input().props.onChange({target:{value:'第一行问题\n第二行背景'}});await f.settle();
  assert.equal(bar().preview,'第一行问题\n第二行背景');
  bar().onClose();await f.settle();f.render(form,props());await f.settle();
  assert.equal(input().props.value,'第一行问题\n第二行背景');
  assert.equal(f.calls.filter(c=>c.name==='discussion_ask').length,0);
  f.key({...key,key:'k',metaKey:true});await f.settle();f.render(form,props());await f.settle();
  assert.equal(bar().open,true);assert.equal(input().props.value,'第一行问题\n第二行背景');
  input().props.onKeyDown(key);await f.settle();
  // Dismissal while IPC is pending neither duplicates the request nor consumes the draft.
  bar().onClose();await f.settle();assert(f.db.has('question'));
  f.completeAsk();await f.settle();
  assert.equal(bar().open,false);assert.equal(bar().preview,'');
  assert.equal(f.calls.filter(c=>c.name==='discussion_ask').length,1);assert.equal(f.db.has('question'),false);
});

test('Escape cancels composition before dismissing, and dismissal restores trigger focus',async t=>{
  const f=workspaceFixture(t),Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  let closed=0,focused=0;
  const view=f.mount(Bar,{open:true,preview:'草稿',scope:null,onOpen(){},onClose(){closed++;},onCapture(){},children:'input'});
  const query=f.find(view,n=>n.props.className==='titlebar-query');
  f.find(view,n=>n.props.className==='recall-trigger').props.ref.current.focus=()=>{focused++;};
  query.props.onCompositionStart();query.props.onKeyDown({...key,key:'Escape'});assert.equal(closed,0);
  query.props.onCompositionEnd();query.props.onKeyDown({...key,key:'Escape',nativeEvent:{isComposing:true}});assert.equal(closed,0);
  query.props.onKeyDown({...key,key:'Escape',keyCode:229});assert.equal(closed,0);
  query.props.onKeyDown({...key,key:'Escape'});assert.equal(closed,1);assert.equal(focused,1);
  const body=f.find(view,n=>n.props.id==='recall-panel');assert.equal(body.props['aria-modal'],undefined);
  f.render(view,{...view.props,open:false});assert.equal(f.find(view,n=>n.props.id==='recall-panel').props.hidden,true);
});

test('Tab to the document dismisses without stealing focus; moving within the panel does not',async t=>{
  const f=workspaceFixture(t),Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  let closed=0,focused=0;
  const view=f.mount(Bar,{open:true,preview:'',scope:null,onOpen(){},onClose(){closed++;},onCapture(){},children:'input'});
  const query=f.find(view,n=>n.props.className==='titlebar-query');
  f.find(view,n=>n.props.className==='recall-trigger').props.ref.current.focus=()=>{focused++;};
  query.props.onBlur({relatedTarget:{},currentTarget:{contains:()=>true}});assert.equal(closed,0);
  query.props.onBlur({relatedTarget:{},currentTarget:{contains:()=>false}});assert.equal(closed,1);assert.equal(focused,0);
});

test('opening capture from recall gives the modal a visible return-focus target',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  const app=f.mount(App);await f.settle();const bar=()=>f.find(app,n=>n.type===Bar).props;
  let focused=0;bar().triggerRef.current={focus(){focused++;assert.equal(bar().open,true);}};
  bar().onOpen();await f.settle();
  f.key({...key,key:'n',metaKey:true});await f.settle();
  assert.equal(focused,1);assert.equal(bar().open,false);
  assert(f.nodes(app.tree).some(n=>n.type===f.load('src/workspace/CaptureDialog.tsx').default));
});

test('missing-model settings returns to the trigger and preserves the question draft',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.workspace_settings=async()=>({configured:false});
  const app=f.mount(App);await f.settle();const bar=()=>f.find(app,n=>n.type===Bar).props;
  let focused=0;bar().triggerRef.current={focus(){focused++;}};
  bar().onOpen();await f.settle();
  const form=f.mount(Form,f.find(app,n=>n.type===Form).props);await f.settle();
  const input=()=>f.find(form,n=>n.type==='textarea');
  input().props.onChange({target:{value:'尚未配置模型的问题'}});await f.settle();
  input().props.onKeyDown(key);await f.settle();
  assert.equal(focused,1);assert.equal(bar().open,false);assert.equal(f.db.get('question').body,'尚未配置模型的问题');
  assert.equal(f.calls.filter(c=>c.name==='discussion_ask').length,0);
  assert(f.nodes(app.tree).some(n=>n.type===f.load('src/workspace/Settings.tsx').default));
});

test('the accessible search name includes the collection boundary and unsent-draft state',t=>{
  const f=workspaceFixture(t),Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  const view=f.mount(Bar,{open:false,preview:'尚未发送',scopeLabel:'产品方向',scope:null,onOpen(){},onClose(){},onCapture(){}});
  const name=f.find(view,n=>n.props.className==='recall-trigger').props['aria-label'];
  assert(name.includes('仅在产品方向中查找'));assert(name.includes('有未发送的草稿'));
});
