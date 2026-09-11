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
  assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)==='开启').props.disabled,true);
  reads[0]('enabled');await f.settle();
  f.focus();await f.settle();assert.equal(reads.length,2);
  f.find(view,n=>n.type==='button'&&f.text(n)==='关闭').props.onClick();await f.settle();
  assert.equal(f.calls.find(c=>c.name==='desktop_login').args.enabled,false);
  reads[1]('enabled');await f.settle();
  assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)==='开启').props.disabled,false);
  assert.equal(reads.length,2);
  f.unmount(view);f.focus();assert.equal(reads.length,2);
});

test('background login-status failure keeps the last known controls visible',async t=>{
 const f=workspaceFixture(t,{native:true});const {previewDesktop}=f.load('src/workspace/desktopApi.ts');
 f.overrides.desktop_state=async()=>({...previewDesktop});f.overrides.desktop_login_status=async()=> 'enabled';
 const view=f.mount(f.load('src/workspace/DesktopSettings.tsx').default);await f.settle();
 f.overrides.desktop_login_status=async()=>{throw 'temporarily unavailable';};f.focus();await f.settle();
 assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)==='关闭').props.disabled,false);
 assert(f.nodes(view.tree).some(n=>n.props.text==='temporarily unavailable'));
});
