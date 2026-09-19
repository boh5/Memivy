import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
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
 const code=ts.transpileModule(fs.readFileSync('src/nativeIpc.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 vm.runInNewContext(code,{module,exports:module.exports,document,require:()=>({invoke})});
 return {...module.exports,document};
}
test('update preparation drains pending calls, blocks new edits, permits durable drafts and resumes',async()=>{
 let finish;const calls=[];
 const ipc=ipcFixture((name)=>{calls.push(name);return name==='library_edit'?new Promise(resolve=>{finish=resolve}):Promise.resolve()});
 const edit=ipc.invoke('library_edit');let drained=false;
 const frozen=ipc.freezeForUpdate().then(()=>{drained=true});await Promise.resolve();
 assert.equal(drained,false);assert.equal(ipc.document.body.inert,true);
 await assert.rejects(ipc.invoke('library_capture'),e=>e.code==='update_busy');
 await ipc.invoke('draft_write');
 finish();await edit;await frozen;assert.equal(drained,true);
 ipc.resumeUpdate();await ipc.invoke('library_capture');assert.equal(ipc.document.body.inert,false);
 assert.deepEqual(calls,['library_edit','draft_write','library_capture']);
});
test('install request cannot deadlock its own draft flush',async()=>{
 const ipc=ipcFixture(()=>new Promise(()=>{}));
 void ipc.invoke('update_install');
 await ipc.freezeForUpdate();ipc.resumeUpdate();
});

import {workspaceFixture} from './helpers/workspace.mjs';
test('initial updater status errors stay visible and retry restores status and live events',async t=>{
 const f=workspaceFixture(t,{native:true});let reads=0;
 const status={phase:'idle',currentVersion:'0.1.2',version:null,notes:null,downloaded:0,total:null,error:null};
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
 f.emit('update-status',{...status,phase:'ready',version:'0.1.3'});await f.settle();
 assert(f.text(view.tree).includes('Restart and install'));
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
 const status=()=>({phase,currentVersion:'0.1.2',version:'0.1.3',notes:'Synthetic release',downloaded:0,total:null,error:null});
 f.overrides.update_status=async()=>status();
 f.overrides.update_download=async()=>{phase='ready';f.emit('update-status',status())};
 f.overrides.update_install=async()=>{};
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 let view=f.mount(View,{onClose:()=>closed++});await f.settle();
 assert(f.text(view.tree).includes('0.1.3'));
 f.find(view,n=>n.type==='button'&&f.text(n)==='Download update').props.onClick();await f.settle();
 assert.equal(f.calls.filter(c=>c.name==='update_install').length,0);
 f.unmount(view);view=f.mount(View,{onClose:()=>closed++});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='Restart and install').props.onClick();await f.settle();
 assert.equal(closed,1);assert.equal(f.calls.filter(c=>c.name==='update_install').length,1);
});
test('a late initial status cannot hide a completed download event',async t=>{
 const f=workspaceFixture(t,{native:true});let initial;
 f.overrides.update_status=()=>new Promise(resolve=>{initial=resolve});
 const View=f.load('src/workspace/UpdateSettings.tsx').default;
 const view=f.mount(View,{onClose:()=>{}});await f.settle();
 const status={phase:'ready',currentVersion:'0.1.2',version:'0.1.3',notes:null,downloaded:10,total:10,error:null};
 f.emit('update-status',status);initial({...status,phase:'downloading'});await f.settle();
 assert(f.text(view.tree).includes('Restart and install'));
 assert(!f.text(view.tree).includes('Downloading and verifying'));
});
