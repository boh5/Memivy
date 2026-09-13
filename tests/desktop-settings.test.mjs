import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('desktop state refreshes never query the system login service', async t => {
  const f=workspaceFixture(t,{native:true});
  const {useDesktop,previewDesktop}=f.load('src/workspace/desktopApi.ts');
  f.overrides.desktop_state=async()=>({...previewDesktop});
  const view=f.mount(useDesktop);await f.settle();
  await view.tree.refresh();await view.tree.refresh();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='desktop_state').length,3);
  assert(!f.calls.some(c=>c.name.startsWith('desktop_login')));
});

test('slow login status leaves other settings usable and stale responses cannot undo a toggle', async t => {
  const f=workspaceFixture(t,{native:true});
  const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
  f.overrides.desktop_state=async()=>({...previewDesktop});
  const reads=[];
  f.overrides.desktop_login_status=()=>new Promise(resolve=>reads.push(resolve));
  f.overrides.desktop_login=async()=> 'disabled';
  const Settings=f.load('src/workspace/DesktopSettings.tsx').default, view=f.mount(Settings);
  await f.settle();
  assert.equal(reads.length,1);
  assert.equal(f.find(view,n=>n.type==='input'&&n.props.type==='checkbox').props.disabled,false);
  assert.equal(f.find(view,n=>n.props.role==='switch').props.disabled,true);
  reads[0]('enabled');await f.settle();
  f.focus();await f.settle();assert.equal(reads.length,2);
  f.find(view,n=>n.props.role==='switch').props.onClick();await f.settle();
  assert.equal(f.calls.find(c=>c.name==='desktop_login').args.enabled,false);
  reads[1]('enabled');await f.settle();
  assert.equal(f.find(view,n=>n.props.role==='switch').props.disabled,false);
  assert.equal(f.find(view,n=>n.props.role==='switch').props['aria-checked'],false);
  assert.equal(reads.length,2);
  f.unmount(view);f.focus();assert.equal(reads.length,2);
});

test('background login-status failure keeps the last known controls visible',async t=>{
 const f=workspaceFixture(t,{native:true});const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
 f.overrides.desktop_state=async()=>({...previewDesktop});f.overrides.desktop_login_status=async()=> 'enabled';
 const view=f.mount(f.load('src/workspace/DesktopSettings.tsx').default);await f.settle();
 f.overrides.desktop_login_status=async()=>{throw 'temporarily unavailable';};f.focus();await f.settle();
 assert.equal(f.find(view,n=>n.props.role==='switch').props.disabled,false);
 assert(f.nodes(view.tree).some(n=>n.props.text==='temporarily unavailable'));
});

test('desktop icon and optional shortcut update independently, and failed removal keeps its value', async t => {
  const f=workspaceFixture(t,{native:true});
  const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
  let state={...previewDesktop,shortcut:'',visible:true,sequence:1}, fail=false;
  f.overrides.desktop_state=async()=>({...state});
  f.overrides.desktop_login_status=async()=> 'disabled';
  f.overrides.desktop_update=async({patch})=>{
    if(fail) throw 'shortcut change failed';
    state={...state,...patch,sequence:state.sequence+1};return {...state};
  };
  const view=f.mount(f.load('src/workspace/DesktopSettings.tsx').default);await f.settle();
  const ShortcutSetting=f.load('src/workspace/ShortcutSetting.tsx').default;
  const shortcut=()=>f.find(view,n=>n.type===ShortcutSetting).props;
  shortcut().onChange('Alt+KeyM');await f.settle();
  assert.equal(state.shortcut,'Alt+KeyM');assert.equal(state.visible,true);
  f.find(view,n=>n.type==='input'&&n.props.type==='checkbox').props.onChange({target:{checked:false}});await f.settle();
  assert.equal(state.visible,false);assert.equal(state.shortcut,'Alt+KeyM');
  fail=true;shortcut().onChange('');await f.settle();
  assert.equal(shortcut().value,'Alt+KeyM');assert.equal(shortcut().disabled,false);
  assert(f.nodes(view.tree).some(n=>n.props.text==='shortcut change failed'));
  fail=false;shortcut().onChange('');await f.settle();
  assert.equal(state.shortcut,'');assert.equal(state.visible,false);
  assert.deepEqual(structuredClone(f.calls.filter(c=>c.name==='desktop_update').at(-1).args.patch),{shortcut:''});
});

test('login completion event supersedes an earlier pending status read without polling',async t=>{
 const f=workspaceFixture(t,{native:true});
 const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
 f.overrides.desktop_state=async()=>({...previewDesktop});
 let finishRead;
 f.overrides.desktop_login_status=()=>new Promise(resolve=>{finishRead=resolve});
 const view=f.mount(f.load('src/workspace/DesktopSettings.tsx').default);await f.settle();
 f.emit('desktop-login-changed','enabled');await f.settle();
 finishRead('disabled');await f.settle();
 assert.equal(f.find(view,n=>n.props.role==='switch').props['aria-checked'],true);
 assert.equal(f.calls.filter(c=>c.name==='desktop_login_status').length,1);
});
