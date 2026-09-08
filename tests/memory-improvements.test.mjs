import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const wait = () => new Promise(resolve => setTimeout(resolve,200));

test('related results ignore stale requests and carry the actual matching source into discussion', async t => {
  const f=workspaceFixture(t); let finish; const discussed=[];
  f.overrides.memory_related=({memoryId})=>memoryId==='a'?new Promise(resolve=>finish=resolve):[{memory_id:'c',version_id:'cv',title:'相关内容',snippet:'真实原话',source:{kind:'capture',id:'raw-c'}}];
  const props={memoryId:'a',versionId:'av',revision:0,onOpen(){},onDiscuss:s=>discussed.push(s)};
  const view=f.mount(f.load('src/workspace/RelatedMemories.tsx').default,props);
  await wait(); await f.settle();
  f.render(view,{...props,memoryId:'b',versionId:'bv'});await wait();await f.settle();
  finish([{memory_id:'wrong',version_id:'wrong',title:'过期结果',snippet:'wrong'}]);await f.settle();
  assert(!f.text(view.tree).includes('过期结果'));
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n).includes('继续想')).props.onClick();await f.settle();
  assert.equal(discussed[0][0].kind,'capture');assert.equal(discussed[0][0].id,'raw-c');
});

test('related queries debounce rapid revisions without polling or model calls', async t=>{
  const f=workspaceFixture(t);f.overrides.memory_related=()=>[];
  const props={memoryId:'a',versionId:'av',revision:0,onOpen(){},onDiscuss(){}};
  const view=f.mount(f.load('src/workspace/RelatedMemories.tsx').default,props);
  for(let i=1;i<5;i++) f.render(view,{...props,revision:i});
  await wait();await f.settle();assert.equal(f.calls.filter(c=>c.name==='memory_related').length,1);
  await wait();await f.settle();assert.equal(f.calls.length,1);
});

test('backup selection stages only, cancel discards it, restore requires the explicit button', async t=>{
  const f=workspaceFixture(t);const restore=[];
  f.overrides.backup_prepare=()=>({id:'staged',memories:2,captures:3});f.overrides.backup_discard=()=>{};
  const props={disabled:false,onBusyChange(){},onRestore:id=>restore.push(id)};
  const view=f.mount(f.load('src/workspace/BackupSettings.tsx').default,props);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='选择备份').props.onClick();await f.settle();
  assert.equal(restore.length,0);
  f.find(view,n=>n.type==='button'&&f.text(n)==='取消').props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='backup_discard').length,1);
  f.find(view,n=>n.type==='button'&&f.text(n)==='选择备份').props.onClick();await f.settle();
  const confirm=f.find(view,n=>n.type==='button'&&f.text(n)==='确认恢复并重启');confirm.props.onClick();confirm.props.onClick();
  f.unmount(view);await f.settle();assert.deepEqual(restore,['staged']);
  assert.equal(f.calls.filter(c=>c.name==='backup_discard').length,1,'unmount must not discard an approved restore');
});
