import test from 'node:test';
import assert from 'node:assert/strict';
import {voiceBody} from '../src/workspace/voiceSession.ts';
import {workspaceFixture} from './helpers/workspace.mjs';

test('transcript appends in order and never overwrites another editor',()=>{
 assert.equal(voiceBody('原有文字','原有文字','原有文字','原有文字今天开会'),'原有文字今天开会');
 assert.equal(voiceBody('第一句','第一句','','第一句 第二句'),'第一句 第二句');
 assert.equal(voiceBody('第一句 第二句','第一句','','第一句 第二句'),'第一句 第二句');
 assert.throws(()=>voiceBody('其他窗口的修改','第一句','','第一句 第二句'),{code:"voice_draft_conflict"});
});
const status=session=>({enabled:true,preload:false,shortcut:'',state:'ready',backend:'Metal GPU',error:null,available:true,downloaded:1019141728,bytes:1019141728,cache:'isolated',session});
const session=patch=>({id:'voice-1',key:'capture',base:'原有文字',body:'原有文字',text:'',recording:true,starting:false,processing:false,complete:false,error:null,seconds:2,level:.1,...patch});

test('submit waits for the final transcript and durable draft before capture',async t=>{
 const f=workspaceFixture(t,{native:true});let stopped=false,finished=false,saved=null;
 f.overrides.voice_status=()=>status(stopped?session({body:'原有文字最后一句',text:'最后一句',recording:false,complete:true}):session({}));
 f.overrides.voice_stop=()=>{stopped=true;};f.overrides.voice_clear=()=>{finished=true;};
 f.overrides.library_capture=({request})=>{saved=request.text;return {memory_id:'saved'};};
 f.db.set('capture',{key:'capture',request_id:'draft-1',title:'',body:'原有文字',expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{mode:'capture',focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}});
 await f.settle();
 f.find(form,n=>n.props.className==='send-button').props.onClick();
 for(let i=0;i<25&&!saved;i++){await new Promise(r=>setTimeout(r,20));await f.settle();}
 assert.equal(saved,'原有文字最后一句');assert(finished);
 assert(f.calls.findIndex(c=>c.name==='voice_stop')<f.calls.findIndex(c=>c.name==='library_capture'));
});

test('failed tail blocks capture and retains the draft and audio session',async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.voice_status=()=>status(session({recording:false,error:'模型处理失败',body:'原有文字'}));
 f.overrides.voice_stop=()=>{};
 f.db.set('capture',{key:'capture',request_id:'draft-1',title:'',body:'原有文字',expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{mode:'capture',focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}});
 await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
 assert(!f.calls.some(c=>c.name==='library_capture'||c.name==='voice_clear'));
 assert.equal(f.db.get('capture').body,'原有文字');
});

test('unmount stops only the recording owned by this input',async t=>{
 const f=workspaceFixture(t,{native:true});f.overrides.voice_status=()=>status(session({key:'quick_capture'}));f.overrides.voice_stop=()=>{};
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{mode:'capture',focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}});
 await f.settle();f.unmount(form);assert(!f.calls.some(c=>c.name==='voice_stop'));
});

test('manual edits after completion remain authoritative across later status reads',async t=>{
 const f=workspaceFixture(t,{native:true});let applied=false;
 f.overrides.voice_status=()=>status(session({recording:false,complete:true,applied,body:'原有文字识别结果',text:'识别结果'}));
 f.overrides.voice_applied=()=>{applied=true;};f.overrides.voice_stop=()=>{};
 f.db.set('capture',{key:'capture',request_id:'draft-1',title:'',body:'原有文字',expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{mode:'capture',focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}});
 await f.settle();await new Promise(r=>setTimeout(r,200));await f.settle();
 f.find(form,n=>n.type==='textarea').props.onChange({target:{value:'我手动改好了'}});await f.settle();
 await new Promise(r=>setTimeout(r,220));await f.settle();
 assert.equal(f.find(form,n=>n.type==='textarea').props.value,'我手动改好了');
 assert.equal(f.db.get('capture').body,'我手动改好了');assert(applied);
});

test('voice control distinguishes recording from final transcription',t=>{
 const f=workspaceFixture(t,{native:true});
 const Button=f.load('src/workspace/VoiceInput.tsx').VoiceButton;
 const recording=f.mount(Button,{disabled:false,voice:{active:true,busy:false,session:session({}),toggle(){}}});
 assert.equal(f.find(recording,n=>n.type==='button').props['aria-label'],'停止录音并转写');
 assert(f.find(recording,n=>n.props.className==='voice-stop'));
 const processing=f.mount(Button,{disabled:false,voice:{active:true,busy:false,session:session({recording:false,processing:true}),toggle(){}}});
 const button=f.find(processing,n=>n.type==='button');
 assert.equal(button.props['aria-label'],'正在转写');
 assert.equal(button.props.disabled,true);
 assert(f.find(processing,n=>n.props.className==='voice-spinner'));
});

function voiceHook(f, key='capture', base='原有文字') {
 let body=base;
 const useVoice=f.load('src/workspace/useVoice.ts').useVoice;
 const hook=f.mount(()=>useVoice(key,body,next=>{body=next;},async()=>{},true,()=>[body.length,body.length]));
 return {hook,body:()=>body};
}
test('a status response after unmount cannot change the draft',async t=>{
 const f=workspaceFixture(t,{native:true});let resolve;
 f.overrides.voice_status=()=>new Promise(r=>{resolve=r;});
 const v=voiceHook(f);f.unmount(v.hook);
 resolve(status(session({body:'迟到的转写',text:'迟到的转写'})));await f.settle();
 assert.equal(v.body(),'原有文字');
});
test('a status read started before clear cannot restore the removed session',async t=>{
 const timers=[];
 const f=workspaceFixture(t,{native:true,timers:{setTimeout:(fn,ms)=>{timers.push({fn,ms});return timers.length;},clearTimeout(){}}});
 let state=status(session({recording:false,complete:true,applied:true})),resolve;
 f.overrides.voice_status=()=>state;
 f.overrides.voice_clear=()=>{state=status(null);};
 const v=voiceHook(f);await f.settle();
 f.overrides.voice_status=()=>new Promise(r=>{resolve=r;});
 timers.find(x=>x.ms===180).fn();await f.settle();
 f.overrides.voice_status=()=>state;
 await v.hook.tree.clear();await f.settle();
 resolve(status(session({})));await f.settle();
 assert.equal(v.hook.tree.session,null);
 assert.equal(v.hook.tree.active,false);
});
test('closing an input waits for an in-flight microphone start and then stops it',async t=>{
 const f=workspaceFixture(t,{native:true});let state=status(null),resolveStart;
 f.overrides.voice_status=()=>state;
 f.overrides.voice_start=()=>new Promise(r=>{resolveStart=()=>{state=status(session({}));r(state);};});
 f.overrides.voice_stop=()=>{state=status(session({recording:false,complete:true}));};
 const v=voiceHook(f);await f.settle();
 const start=v.hook.tree.toggle();await f.settle();
 const finish=f.load('src/workspace/useVoice.ts').finishVoiceInputs();await f.settle();
 resolveStart();await start;await finish;await f.settle();
 assert(f.calls.some(c=>c.name==='voice_stop'));
 assert.equal(state.session.recording,false);
});

test('temporary audio cleanup failure does not hide a successful capture',async t=>{
 const f=workspaceFixture(t,{native:true});let saved=0;
 f.overrides.voice_status=()=>status(session({recording:false,complete:true,applied:true}));
 f.overrides.voice_stop=()=>{};f.overrides.voice_clear=()=>{throw Error('disk unavailable');};
 f.overrides.library_capture=()=>({memory_id:'saved'});
 f.db.set('capture',{key:'capture',request_id:'draft-1',title:'',body:'原有文字',expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{mode:'capture',focus:0,onSaved(){saved++;},onAsk(){},onMode(){},onEdit(){}});
 await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
 assert.equal(saved,1);assert(!f.db.has('capture'));
});
test('a conflicting transcript cannot be discarded by starting another recording',async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.voice_status=()=>status(session({recording:false,complete:true,applied:false,body:'原有文字识别结果',text:'识别结果'}));
 f.overrides.voice_stop=()=>{};
 const v=voiceHook(f,'capture','另一个编辑器修改的内容');await f.settle();
 await v.hook.tree.toggle();await f.settle();
 assert(!f.calls.some(c=>c.name==='voice_clear'||c.name==='voice_start'||c.name==='voice_applied'));
 assert.equal(v.body(),'另一个编辑器修改的内容');
});
test('a failed installed model exposes repair and keeps downloads pausable',async t=>{
 const f=workspaceFixture(t,{native:true});let state={...status(null),state:'failed'};
 f.overrides.voice_status=()=>state;
 f.overrides.voice_control=()=>{state={...state,state:'downloading'};};
 const view=f.mount(f.load('src/workspace/VoiceSettings.tsx').default);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='校验并修复模型').props.onClick();await f.settle();
 assert(f.calls.some(c=>c.name==='voice_control'&&c.args.action==='download'));
 assert(f.find(view,n=>n.type==='button'&&f.text(n)==='暂停下载'));
});
