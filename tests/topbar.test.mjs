import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const key={key:'Enter',metaKey:false,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,keyCode:13,nativeEvent:{isComposing:false},preventDefault(){},stopPropagation(){}};

test('dismiss/reopen keeps the mounted question draft, and successful submission closes the panel',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  const app=f.mount(App);await f.settle();
  const bar=()=>f.find(f.query(app),n=>n.type===Bar).props;
  const props=()=>f.find(f.query(app),n=>n.type===Form).props;
  assert.equal(bar().open,false);assert.equal(props().focus,0);
  bar().onOpen();await f.settle();
  const form=f.mount(Form,props());await f.settle();
  const input=()=>f.find(form,n=>n.type==='textarea');
  input().props.onChange({target:{value:"First line question\nSecond line context"}});await f.settle();
  assert.equal(bar().preview,'');
  bar().onClose();await f.settle();f.render(form,props());await f.settle();
  assert.equal(input().props.value,"First line question\nSecond line context");
  assert.equal(bar().preview,"First line question\nSecond line context");
  assert.equal(f.calls.filter(c=>c.name==='discussion_submit').length,0);
  f.key({...key,key:'k',metaKey:true});await f.settle();f.render(form,props());await f.settle();
  assert.equal(bar().open,true);assert.equal(input().props.value,"First line question\nSecond line context");
  input().props.onKeyDown({...key,metaKey:true});await f.settle();
  // Dismissal while IPC is pending neither duplicates the request nor consumes the draft.
  bar().onClose();await f.settle();f.render(form,props());await f.settle();assert(f.db.has('input'));
  f.completeAsk();await f.settle();
  assert.equal(bar().open,false);assert.equal(bar().preview,'');
  assert.equal(f.calls.filter(c=>c.name==='discussion_submit').length,1);assert.equal(f.db.has('input'),false);
});

test('Escape cancels composition before dismissing, and dismissal restores trigger focus',async t=>{
  const f=workspaceFixture(t),Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  let closed=0,focused=0;
  const view=f.mount(Bar,{open:true,preview:"Draft",scope:null,onOpen(){},onClose(){closed++;},onNewDiscussion(){},children:'input'});
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
  const view=f.mount(Bar,{open:true,preview:'',scope:null,onOpen(){},onClose(){closed++;},onNewDiscussion(){},children:'input'});
  const query=f.find(view,n=>n.props.className==='titlebar-query');
  f.find(view,n=>n.props.className==='recall-trigger').props.ref.current.focus=()=>{focused++;};
  query.props.onBlur({relatedTarget:{},currentTarget:{contains:()=>true}});assert.equal(closed,0);
  query.props.onBlur({relatedTarget:{},currentTarget:{contains:()=>false}});assert.equal(closed,1);assert.equal(focused,0);
});

test('new discussion preserves the existing unsent input',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  const app=f.mount(App);await f.settle();const form=f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  f.find(form,n=>n.type==='textarea').props.onChange({target:{value:"Unsent idea"}});await f.settle();
  f.key({...key,key:'n',metaKey:true});await f.settle();
  assert.equal(f.db.get('input').body,"Unsent idea");
  assert(f.nodes(app.tree).some(n=>n.props.topic?.id==='topic-a'));
});

test('missing model does not prevent submitting and preserving the expression',async t=>{
  const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
  f.overrides.workspace_settings=async()=>({configured:false});let saved;
  f.overrides.discussion_submit=async input=>{saved=input;return f.topic;};
  const app=f.mount(App);await f.settle();const form=f.mount(Form,f.find(f.query(app),n=>n.type===Form).props);await f.settle();
  f.find(form,n=>n.type==='textarea').props.onChange({target:{value:"Input before model setup"}});await f.settle();
  f.find(form,n=>n.type==='textarea').props.onKeyDown({...key,metaKey:true});await f.settle();
  assert.equal(saved.text,"Input before model setup");
  assert(f.nodes(app.tree).some(n=>n.props.topic?.id==='topic-a'));
  assert(!f.nodes(app.tree).some(n=>n.type===f.load('src/workspace/Settings.tsx').default));
});

test('the accessible search name includes the collection boundary and unsent-draft state',t=>{
  const f=workspaceFixture(t),Bar=f.load('src/workspace/WorkspaceTopBar.tsx').default;
  const view=f.mount(Bar,{open:false,preview:"Not sent yet",scopeLabel:"Product direction",scope:null,onOpen(){},onClose(){},onNewDiscussion(){}});
  const name=f.find(view,n=>n.props.className==='recall-trigger').props['aria-label'];
  assert(name.includes("focusing on Product direction"));assert(name.includes("unsent draft"));
});
