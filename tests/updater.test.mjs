import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import { compileFixture } from './helpers/compile.mjs';
import {updateManifest,prepareUpdate} from '../scripts/prepare-update.mjs';
import path from 'node:path';
import os from 'node:os';
const signature=Buffer.from('untrusted comment: signature\nsynthetic\ntrusted comment: timestamp\nsynthetic').toString('base64');
test('manifest pins stable public release assets and keeps the complete encoded signature',()=>{
 const result=updateManifest({version:'1.2.3',notes:'Release notes',signature:signature+'\n'});
 assert.equal(result.platforms['darwin-aarch64'].signature,signature);
 assert.equal(result.platforms['darwin-aarch64'].url,'https://github.com/boh5/memivy/releases/download/v1.2.3/Memivy.app.tar.gz');
 assert.throws(()=>updateManifest({version:'1.2.3-beta',notes:'',signature}));
 assert.throws(()=>updateManifest({version:'1.2.3',notes:'',signature:'wrong'}));
});
test('release assets require an update archive and signature before publishing a manifest',t=>{
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'memivy-update-assets-'));t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
 const target=path.join(root,'out');fs.writeFileSync(path.join(root,'Memivy.app.tar.gz.sig'),signature);
 assert.throws(()=>prepareUpdate(root,target,'1.2.3','notes'));
 assert.equal(fs.existsSync(path.join(target,'latest.json')),false);
 fs.writeFileSync(path.join(root,'Memivy.app.tar.gz'),'synthetic archive');
 prepareUpdate(root,target,'1.2.3','notes');
 assert.equal(JSON.parse(fs.readFileSync(path.join(target,'latest.json'))).version,'1.2.3');
});
function ipcFixture(invoke) {
 const module={exports:{}};const document={body:{inert:false}};
 const code=compileFixture('src/nativeIpc.ts');
 vm.runInNewContext(code,{module,exports:module.exports,document,require:()=>({invoke})});
 return {...module.exports,document};
}
test('update preparation drains pending calls, blocks new edits, permits durable drafts and resumes',async()=>{
 let finish;const calls=[];
 const ipc=ipcFixture((name)=>{calls.push(name);return name==='library_edit'?new Promise(resolve=>{finish=resolve}):Promise.resolve()});
 const edit=ipc.invoke('library_edit');let drained=false;
 const frozen=ipc.freezeForUpdate().then(()=>{drained=true});await Promise.resolve();
 assert.equal(drained,false);assert.equal(ipc.document.body.inert,true);
 await assert.rejects(ipc.invoke('discussion_submit'),e=>e.code==='update_busy');
 await ipc.invoke('draft_write');
 finish();await edit;await frozen;assert.equal(drained,true);
 ipc.resumeUpdate();await ipc.invoke('discussion_submit');assert.equal(ipc.document.body.inert,false);
 assert.deepEqual(calls,['library_edit','draft_write','discussion_submit']);
});
test('install request cannot deadlock its own draft flush',async()=>{
 const ipc=ipcFixture(()=>new Promise(()=>{}));
 void ipc.invoke('update_install');
 await ipc.freezeForUpdate();ipc.resumeUpdate();
});

import {workspaceFixture} from './helpers/workspace.mjs';
test('initial updater status errors stay visible and retry restores status and live events',async t=>{
 const f=workspaceFixture(t,{native:true});let reads=0;
 const status={revision:0,automatic:true,phase:'idle',currentVersion:'0.1.2',version:null,notes:null,downloaded:0,total:null,error:null};
 f.overrides.update_status=async()=>{if(++reads===1)throw 'Status could not be read';return status};
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const {ErrorNotice}=f.load('src/workspace/components.tsx');
 const view=f.mount(View,{onClose(){}});await f.settle();
 assert.equal(f.find(view,n=>n.type===ErrorNotice).props.text,'Status could not be read');
 f.find(view,n=>n.type==='button'&&f.text(n)==='Retry').props.onClick();await f.settle();
 assert.equal(reads,2);
 assert(f.text(view.tree).includes('0.1.2'));
 assert.equal(f.find(view,n=>n.type===ErrorNotice).props.text,'');
 assert(!f.calls.some(c=>c.name==='update_check'||c.name==='update_download'||c.name==='update_install'));
 f.emit('update-status',{...status,revision:status.revision+1,phase:'ready',version:'0.1.3'});await f.settle();
 assert(f.text(view.tree).includes('Restart and update'));
});
test('a timed-out flush cannot unfreeze a later installation attempt',async t=>{
 const pending=[];let frozen=false,resumes=0;
 const f=workspaceFixture(t,{native:true,modules:{
  '../nativeIpc':{freezeForUpdate:async()=>{frozen=true},resumeUpdate:()=>{frozen=false;resumes++}},
  './useDraft':{flushDrafts:()=>new Promise((resolve,reject)=>pending.push({resolve,reject})),refreshDrafts:async()=>{}},
  './useVoice':{finishVoiceInputs:async()=>{}},
 }});
 f.overrides.desktop_ready=async()=>{};f.overrides.desktop_exit_ready=async()=>{};
 const {useWindowLifecycle}=f.load('src/workspace/desktopApi.ts');
 f.mount(()=>{useWindowLifecycle(()=>{});return null});await f.settle();
 f.emit('update-exit-request',1);await f.settle();assert.equal(pending.length,1);
 f.emit('update-resumed',1);assert.equal(frozen,false);
 f.emit('update-exit-request',2);await f.settle();assert.equal(frozen,true);
 const previousResumes=resumes;f.emit('update-resumed',1);assert.equal(frozen,true);assert.equal(resumes,previousResumes);pending[0].reject(Error('late failure'));await f.settle();
 assert.equal(frozen,true);assert.equal(resumes,previousResumes);
 pending[1].resolve();await f.settle();
 const acks=f.calls.filter(c=>c.name==='desktop_exit_ready');
 assert.equal(acks.length,1);assert.equal(acks[0].args.id,2);assert.equal(acks[0].args.error,false);
});
test('update UI separates download from installation and preserves status on reopening',async t=>{
 let phase='available',closed=0;
 const f=workspaceFixture(t,{native:true,modules:{'react-dom':{flushSync:fn=>fn()}}});
 const status=()=>({revision:phase==='ready'?1:0,automatic:true,phase,currentVersion:'0.1.2',version:'0.1.3',notes:'Synthetic release',downloaded:0,total:null,error:null});
 f.overrides.update_status=async()=>status();
 f.overrides.update_download=async()=>{phase='ready';f.emit('update-status',status())};
 f.overrides.update_install=async()=>{};
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 let view=f.mount(View,{onClose:()=>closed++});await f.settle();
 assert(f.text(view.tree).includes('0.1.3'));
 f.find(view,n=>n.type==='button'&&f.text(n)==='Download update').props.onClick();await f.settle();
 assert.equal(f.calls.filter(c=>c.name==='update_install').length,0);
 f.unmount(view);view=f.mount(View,{onClose:()=>closed++});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='Restart and update').props.onClick();await f.settle();
 assert.equal(closed,1);assert.equal(f.calls.filter(c=>c.name==='update_install').length,1);
});
test('a late initial status cannot hide a completed download event',async t=>{
 const f=workspaceFixture(t,{native:true});let initial;
 f.overrides.update_status=()=>new Promise(resolve=>{initial=resolve});
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const view=f.mount(View,{onClose:()=>{}});await f.settle();
 const status={revision:2,automatic:true,phase:'ready',currentVersion:'0.1.2',version:'0.1.3',notes:null,downloaded:10,total:10,error:null};
 f.emit('update-status',status);initial({...status,revision:status.revision-1,phase:'downloading'});await f.settle();
 assert(f.text(view.tree).includes('Restart and update'));
 assert(!f.text(view.tree).includes('Downloading and verifying'));
});

test('automatic update preference changes only after a successful save and survives reopening',async t=>{
 const f=workspaceFixture(t,{native:true});
 let status={revision:0,automatic:true,phase:'idle',currentVersion:'0.1.5',version:null,notes:null,downloaded:0,total:null,error:null};
 f.overrides.update_status=async()=>status;
 f.overrides.update_set_automatic=async()=>{throw {code:'update_preferences_save_failed'}};
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const {ErrorNotice}=f.load('src/workspace/components.tsx');
 let view=f.mount(View,{onClose(){}});await f.settle();
 const toggle=()=>f.find(view,n=>n.props.role==='switch');
 assert.equal(toggle().props.checked,true);
 toggle().props.onChange({target:{checked:false}});await f.settle();
 assert.equal(toggle().props.checked,true);
 assert(f.find(view,n=>n.type===ErrorNotice).props.text.includes('Could not save'));
 f.overrides.update_set_automatic=async({automatic})=>{status={...status,revision:status.revision+1,automatic};f.emit('update-status',status)};
 toggle().props.onChange({target:{checked:false}});await f.settle();
 assert.equal(toggle().props.checked,false);
 f.unmount(view);view=f.mount(View,{onClose(){}});await f.settle();
 assert.equal(toggle().props.checked,false);
 assert(!f.calls.some(c=>['update_check','update_download','update_install'].includes(c.name)));
});

test('a queued older update event cannot overwrite a saved preference or restart-ready state',async t=>{
 const f=workspaceFixture(t,{native:true});
 const status={revision:1,automatic:true,phase:'downloading',currentVersion:'0.1.5',version:'0.1.6',notes:null,downloaded:5,total:10,error:null};
 f.overrides.update_status=async()=>status;
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const view=f.mount(View,{onClose(){}});await f.settle();
 f.emit('update-status',{...status,revision:3,automatic:false,phase:'ready',downloaded:10});
 f.emit('update-status',{...status,revision:2,automatic:true});await f.settle();
 assert.equal(f.find(view,n=>n.props.role==='switch').props.checked,false);
 assert(f.text(view.tree).includes('Restart and update'));
 assert(!f.text(view.tree).includes('Downloading and verifying'));
});

test('the ready notice waits for a click and dismisses only its own version',async t=>{
 const f=workspaceFixture(t,{native:true});
 const status={revision:0,automatic:true,phase:'downloading',currentVersion:'0.1.5',version:'0.1.6',notes:null,downloaded:0,total:10,error:null};
 f.overrides.update_status=async()=>status;
 let finish;
 f.overrides.update_install=()=>new Promise(resolve=>{finish=resolve});
 const View=f.load('src/workspace/UpdateNotice.tsx').default;
 const view=f.mount(View);await f.settle();
 assert.equal(view.tree,null);
 f.emit('update-status',{...status,revision:status.revision+1,phase:'ready'});await f.settle();
 assert(f.text(view.tree).includes('0.1.6'));
 assert.equal(f.calls.filter(c=>c.name==='update_install').length,0);
 f.find(view,n=>n.type==='button'&&f.text(n)==='Later').props.onClick();await f.settle();
 assert.equal(view.tree,null);
 f.emit('update-status',{...status,revision:2,phase:'ready',automatic:false});await f.settle();
 assert.equal(view.tree,null);
 f.emit('update-status',{...status,revision:3,phase:'ready',version:'0.1.7'});await f.settle();
 const button=f.find(view,n=>n.type==='button'&&f.text(n)==='Restart and update');
 button.props.onClick();button.props.onClick();await f.settle();
 assert.equal(f.calls.filter(c=>c.name==='update_install').length,1);
 assert.equal(f.find(view,n=>n.type==='button'&&f.text(n)==='Restart and update').props.disabled,true);
 finish();await f.settle();
});

test('disabled development updates expose neither automation controls nor a ready notice',async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.update_status=async()=>({revision:0,automatic:true,phase:'disabled',currentVersion:'0.1.5',version:null,notes:null,downloaded:0,total:null,error:null});
 const Settings=f.load('src/workspace/UpdateSettings.tsx').default;
 const Notice=f.load('src/workspace/UpdateNotice.tsx').default;
 const settings=f.mount(Settings,{onClose(){}}),notice=f.mount(Notice);await f.settle();
 assert(f.text(settings.tree).includes('installed release app'));
 assert.equal(f.nodes(settings.tree).filter(n=>n.props.role==='switch').length,0);
 assert.equal(notice.tree,null);
});

for(const phase of ['idle','available'])test(`manual update ${phase==='idle'?'checks':'downloads'} do not lock the settings dialog`,async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.update_status=async()=>({revision:0,automatic:true,phase,currentVersion:'0.1.5',version:'0.1.6',notes:null,downloaded:0,total:null,error:null});
 let finish;const locks=[];
 const command=phase==='idle'?'update_check':'update_download';
 f.overrides[command]=()=>new Promise(resolve=>{finish=resolve});
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const view=f.mount(View,{onClose(){},onBusyChange:busy=>locks.push(busy)});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)===(phase==='idle'?'Check for updates':'Download update')).props.onClick();await f.settle();
 assert.equal(f.calls.filter(c=>c.name===command).length,1);
 assert.deepEqual(locks,[]);
 f.unmount(view);finish();await f.settle();
 assert.deepEqual(locks,[]);
});
