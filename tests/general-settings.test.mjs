import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('restore general settings handles swapped shortcuts and only writes general preferences',async t=>{
 const f=workspaceFixture(t);
 const {restoreGeneralSettings}=f.load('src/workspace/restoreGeneralSettings.ts');
 const state={desktop:'Alt+KeyR',voice:'Alt+KeyM',visible:false,login:false,language:'en',model:'custom',mcp:false};
 const calls=[];
 const call=async(name,args)=>{
  calls.push(name);
  if(name==='desktop_state')return {shortcut:state.desktop,visible:state.visible};
  if(name==='voice_status')return {shortcut:state.voice};
  if(name==='voice_control'){
   assert.equal(args.action,'shortcut');
   assert.notEqual(args.value,state.desktop);
   state.voice=args.value;
  }else if(name==='desktop_update'){
   assert.notEqual(args.patch.shortcut,state.voice);
   assert.deepEqual(Object.keys(args.patch).sort(),['shortcut','visible']);
   state.desktop=args.patch.shortcut;state.visible=args.patch.visible;
  }else if(name==='desktop_login')state.login=args.enabled;
  else assert.fail(`unexpected mutation ${name}`);
 };
 await restoreGeneralSettings(call,async language=>{state.language=language});
 assert.deepEqual(state,{desktop:'Alt+KeyM',voice:'Alt+KeyR',visible:true,login:true,language:'system',model:'custom',mcp:false});
 assert.deepEqual(calls,['desktop_state','voice_status','voice_control','desktop_update','voice_control','desktop_login']);
});

test('failed desktop default registration restores the displaced voice binding',async t=>{
 const f=workspaceFixture(t);
 const {restoreGeneralSettings}=f.load('src/workspace/restoreGeneralSettings.ts');
 let voice='Alt+KeyM';
 await assert.rejects(restoreGeneralSettings(async(name,args)=>{
  if(name==='desktop_state')return {shortcut:'Alt+KeyS',visible:false};
  if(name==='voice_status')return {shortcut:voice};
  if(name==='voice_control'){voice=args.value;return;}
  if(name==='desktop_update'){if(args.patch.shortcut==='Alt+KeyM')throw new Error('shortcut unavailable');return;}
  assert.fail(`unexpected mutation ${name}`);
 },async()=>assert.fail('language should not change after failure')),/shortcut unavailable/);
 assert.equal(voice,'Alt+KeyM');
});

for(const rollbackFails of [false,true])test(`voice default registration failure rolls back both bindings; rollback failure=${rollbackFails}`,async t=>{
 const f=workspaceFixture(t);
 const {restoreGeneralSettings}=f.load('src/workspace/restoreGeneralSettings.ts');
 let desktop='Alt+KeyR',voice='Alt+KeyM',visible=false;
 await assert.rejects(restoreGeneralSettings(async(name,args)=>{
  if(name==='desktop_state')return {shortcut:desktop,visible};
  if(name==='voice_status')return {shortcut:voice};
  if(name==='desktop_update'){desktop=args.patch.shortcut;visible=args.patch.visible;return;}
  if(name==='voice_control'){
   if(args.value==='Alt+KeyR')throw new Error('voice default unavailable');
   if(rollbackFails&&args.value==='Alt+KeyM')throw new Error('rollback failed');
   voice=args.value;return;
  }
  assert.fail(`unexpected mutation ${name}`);
 },async()=>assert.fail('language should not change')),rollbackFails?{code:'general_reset_partial'}:/voice default unavailable/);
 assert.equal(desktop,'Alt+KeyR');assert.equal(visible,false);
 if(!rollbackFails)assert.equal(voice,'Alt+KeyM');
});

for(const failure of ['login','language'])test(`general reset reports partial completion when ${failure} fails after shortcuts are restored`,async t=>{
 const f=workspaceFixture(t);
 const {restoreGeneralSettings}=f.load('src/workspace/restoreGeneralSettings.ts');
 const state={desktop:'Alt+KeyS',voice:'Alt+KeyV',visible:false,login:false,language:'en'};
 await assert.rejects(restoreGeneralSettings(async(name,args)=>{
  if(name==='desktop_state')return {shortcut:state.desktop,visible:state.visible};
  if(name==='voice_status')return {shortcut:state.voice};
  if(name==='voice_control'){state.voice=args.value;return;}
  if(name==='desktop_update'){state.desktop=args.patch.shortcut;state.visible=args.patch.visible;return;}
  if(name==='desktop_login'){if(failure==='login')throw Error('login failed');state.login=true;return;}
  assert.fail(`unexpected call ${name}`);
 },async()=>{if(failure==='language')throw Error('language failed');state.language='system';}),{code:'general_reset_partial'});
 assert.deepEqual(state,{desktop:'Alt+KeyM',voice:'Alt+KeyR',visible:true,login:failure!=='login',language:'en'});
});
