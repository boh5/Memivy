import test from 'node:test';
import assert from 'node:assert/strict';
import {voiceBody} from '../src/workspace/voiceSession.ts';
import {workspaceFixture} from './helpers/workspace.mjs';

test('transcript appends in order and never overwrites another editor',()=>{
 assert.equal(voiceBody("Original text","Original text","Original text","Original text meeting today"),"Original text meeting today");
 assert.equal(voiceBody("First sentence","First sentence",'',"First sentence second sentence"),"First sentence second sentence");
 assert.equal(voiceBody("First sentence second sentence","First sentence",'',"First sentence second sentence"),"First sentence second sentence");
 assert.throws(()=>voiceBody("Edits from another window","First sentence",'',"First sentence second sentence"),{code:"voice_draft_conflict"});
});
const status=session=>({enabled:true,preload:false,shortcut:'',state:'ready',source:'local',local_available:true,backend:'Metal GPU',error:null,available:true,downloaded:1019141728,bytes:1019141728,cache:'isolated',session});
const session=patch=>({id:'voice-1',key:'input',base:"Original text",body:"Original text",text:'',recording:true,starting:false,processing:false,complete:false,error:null,seconds:2,level:.1,...patch});

test('submit waits for the final transcript and durable draft before capture',async t=>{
 const f=workspaceFixture(t,{native:true});let started=false,stopped=false,finished=false,saved=null;
 f.overrides.voice_status=()=>status(!started?null:stopped?session({body:"Original text last sentence",text:"Last sentence",recording:false,complete:true}):session({}));
 f.overrides.voice_start=()=>{started=true;return f.overrides.voice_status();};
 f.overrides.voice_stop=()=>{stopped=true;};f.overrides.voice_clear=()=>{finished=true;};
 f.overrides.discussion_submit=input=>{saved=input.text;return f.topic;};
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async input=>{f.calls.push({name:'discussion_submit',args:input});return f.overrides.discussion_submit?.(input);}});
 await f.settle();
 await formVoice(f,form).toggle();await f.settle();
 f.find(form,n=>n.props.className==='send-button').props.onClick();
 for(let i=0;i<25&&!saved;i++){await new Promise(r=>setTimeout(r,20));await f.settle();}
 assert.equal(saved,"Original text last sentence");assert(finished);
 assert(f.calls.findIndex(c=>c.name==='voice_stop')<f.calls.findIndex(c=>c.name==='discussion_submit'));
});

test('failed tail blocks capture and retains the draft and audio session',async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.voice_status=()=>status(session({recording:false,error:"Model processing failed",body:"Original text"}));
 f.overrides.voice_stop=()=>{};
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async input=>{f.calls.push({name:'discussion_submit',args:input});return f.overrides.discussion_submit?.(input);}});
 await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
 assert(!f.calls.some(c=>c.name==='discussion_submit'||c.name==='voice_clear'));
 assert.equal(f.db.get('input').body,"Original text");
});

test('unmount stops only the recording owned by this input',async t=>{
 const f=workspaceFixture(t,{native:true});f.overrides.voice_status=()=>status(session({key:'quick_input'}));f.overrides.voice_stop=()=>{};
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async input=>{f.calls.push({name:'discussion_submit',args:input});return f.overrides.discussion_submit?.(input);}});
 await f.settle();f.unmount(form);assert(!f.calls.some(c=>c.name==='voice_stop'));
});

test('manual edits after completion remain authoritative across later status reads',async t=>{
 const f=workspaceFixture(t,{native:true});let applied=false;
 f.overrides.voice_status=()=>status(session({recording:false,complete:true,applied,body:"Original text transcript",text:"Transcript"}));
 f.overrides.voice_applied=()=>{applied=true;};f.overrides.voice_stop=()=>{};
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async input=>{f.calls.push({name:'discussion_submit',args:input});return f.overrides.discussion_submit?.(input);}});
 await f.settle();await new Promise(r=>setTimeout(r,200));await f.settle();
 f.find(form,n=>n.type==='textarea').props.onChange({target:{value:"I edited this manually"}});await f.settle();
 await new Promise(r=>setTimeout(r,220));await f.settle();
 assert.equal(f.find(form,n=>n.type==='textarea').props.value,"I edited this manually");
 assert.equal(f.db.get('input').body,"I edited this manually");assert(applied);
});

test('voice control distinguishes recording from final transcription',t=>{
 const f=workspaceFixture(t,{native:true});
 const Button=f.load('src/workspace/VoiceInput.tsx').VoiceButton;
 const recording=f.mount(Button,{disabled:false,voice:{active:true,busy:false,session:session({}),toggle(){}}});
 assert.equal(f.find(recording,n=>n.type==='button').props['aria-label'],"Stop recording and transcribe");
 assert(f.find(recording,n=>n.props.className==='voice-stop'));
 const processing=f.mount(Button,{disabled:false,voice:{active:true,busy:false,session:session({recording:false,processing:true}),toggle(){}}});
 const button=f.find(processing,n=>n.type==='button');
 assert.equal(button.props['aria-label'],"Transcribing");
 assert.equal(button.props.disabled,true);
 assert(f.find(processing,n=>n.props.className==='voice-spinner'));
});

function voiceHook(f, key='input', base="Original text", flush=async()=>{}) {
 let body=base;
 const useVoice=f.load('src/workspace/useVoice.ts').useVoice;
 const hook=f.mount(()=>useVoice(key,body,next=>{body=next;},flush,true,()=>[body.length,body.length]));
 return {hook,body:()=>body};
}
function formVoice(f,form) {
 return f.find(form,n=>n.type===f.load('src/workspace/VoiceInput.tsx').VoiceButton).props.voice;
}
function controlledTimers() {
 const timers=[];
 return {
  timers:{setTimeout:(fn,ms)=>{const timer={fn,ms,active:true};timers.push(timer);return timer;},clearTimeout:timer=>{if(timer)timer.active=false;}},
  async advance(ms){for(const timer of [...timers])if(timer.active&&timer.ms===ms){timer.active=false;await timer.fn();}}
 };
}

test('a hidden composer sharing the draft cannot stop or apply the visible recording',async t=>{
 const clock=controlledTimers(),f=workspaceFixture(t,{native:true,timers:clock.timers});
 const key='discussion:00000000-0000-4000-8000-000000000001';let state=status(null);
 f.overrides.voice_status=()=>state;
 f.overrides.voice_start=()=>state=status(session({key,base:'',body:'',text:''}));
 f.overrides.voice_stop=()=>state=status(session({key,base:'',body:"Transcript",text:"Transcript",recording:false,complete:true}));
 f.db.set(key,{key,request_id:'draft-1',title:'',body:'',expected_version:null});
 const Form=f.load('src/workspace/CaptureForm.tsx').default;
 const visible=f.mount(Form,{draftKey:key,visible:true,onSubmit:async()=>{}});
 const hidden=f.mount(Form,{draftKey:key,visible:false,onSubmit:async()=>{}});
 await f.settle();await formVoice(f,visible).toggle();await f.settle();
 state=status({...state.session,text:"Segment being transcribed",body:"Segment being transcribed"});
 await clock.advance(1500);await f.settle();
 assert.equal(state.session.recording,true);
 assert(!f.calls.some(c=>c.name==='voice_stop'||c.name==='voice_applied'));
 assert.equal(formVoice(f,hidden).foreign,true);
 f.unmount(hidden);assert(!f.calls.some(c=>c.name==='voice_stop'));
 // Hiding the initiating surface still stops and preserves its own recording.
 f.render(visible,{...visible.props,visible:false});await f.settle();
 assert.equal(state.session.recording,false);
 assert.equal(f.calls.filter(c=>c.name==='voice_stop').length,1);
 await clock.advance(120);await f.settle();
 assert.equal(f.db.get(key).body,"Transcript");
});

test('another window with the same draft cannot stop, apply, or consume an active recording',async t=>{
 const owner=workspaceFixture(t,{native:true}),foreign=workspaceFixture(t,{native:true});
 let state=status(null);
 for(const f of [owner,foreign]) {
  f.overrides.voice_status=()=>state;
  f.overrides.voice_start=()=>state=status(session({}));
  f.overrides.voice_stop=()=>state=status(session({recording:false,complete:true,text:"Done",body:"Original text complete"}));
  f.overrides.voice_clear=()=>state=status(null);
 }
 const a=voiceHook(owner),b=voiceHook(foreign);await owner.settle();await foreign.settle();
 await a.hook.tree.toggle();await owner.settle();
 state=status({...state.session,text:"New text",body:"Original text new text"});
 await foreign.load('src/workspace/useVoice.ts').finishVoiceInputs();await foreign.settle();
 assert.equal(b.body(),"Original text");
 await assert.rejects(b.hook.tree.finish(),{code:'voice_draft_exists'});
 await b.hook.tree.consumed(null);
 foreign.unmount(b.hook);
 assert(!foreign.calls.some(c=>['voice_stop','voice_applied','voice_clear'].includes(c.name)));
 assert.equal(state.session.recording,true);
 owner.unmount(a.hook);
 assert.equal(owner.calls.filter(c=>c.name==='voice_stop').length,1);
 assert.equal(state.session.recording,false);
});

test('unmount during microphone start stops its returned session without changing the draft',async t=>{
 const f=workspaceFixture(t,{native:true});let state=status(null),resolveStart;
 f.overrides.voice_status=()=>state;
 f.overrides.voice_start=()=>new Promise(resolve=>{resolveStart=()=>{state=status(session({body:"Late text",text:"Late text"}));resolve(state);};});
 f.overrides.voice_stop=()=>state=status({...state.session,recording:false,complete:true});
 const v=voiceHook(f);await f.settle();const starting=v.hook.tree.toggle();await f.settle();
 f.unmount(v.hook);resolveStart();await starting;await f.settle();
 assert.equal(f.calls.filter(c=>c.name==='voice_stop').length,1);
 assert.equal(v.body(),"Original text");assert(!f.calls.some(c=>c.name==='voice_applied'));
});

test('empty audio leaves an existing draft sendable and cannot replace selected text',async t=>{
 const f=workspaceFixture(t,{native:true});let saved=null;
 f.overrides.voice_status=()=>status(session({recording:false,error:'voice_no_audio',base:"Preserve original",body:'',text:''}));
 f.overrides.voice_clear=()=>{};
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Preserve original",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async input=>{saved=input.text;}});
 await f.settle();assert.equal(f.find(form,n=>n.type==='textarea').props.value,"Preserve original");
 f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
 assert.equal(saved,"Preserve original");assert(!f.calls.some(c=>c.name==='voice_applied'));
});

test('empty audio can be recorded again while retained unrecognized audio requires retry or removal',async t=>{
 const f=workspaceFixture(t,{native:true});let state=status(session({recording:false,error:'voice_no_audio'}));
 f.overrides.voice_status=()=>state;
 f.overrides.voice_clear=()=>state=status(null);
 f.overrides.voice_start=()=>state=status(session({id:'voice-2'}));
 f.overrides.voice_stop=()=>{};
 const v=voiceHook(f);await f.settle();await v.hook.tree.toggle();await f.settle();
 assert.equal(state.session.id,'voice-2');
 assert.equal(f.calls.filter(c=>c.name==='voice_start').length,1);
 state=status(session({id:'voice-2',recording:false,error:'voice_no_transcription'}));
 await v.hook.tree.toggle();await f.settle();
 assert.equal(state.session.error,'voice_no_transcription');
 assert.equal(f.calls.filter(c=>c.name==='voice_start').length,1);
 assert.equal(f.calls.filter(c=>c.name==='voice_clear').length,1);
});

test('a no-audio attempt cannot block recording into a different draft',async t=>{
 const f=workspaceFixture(t,{native:true});let state=status(session({key:'input',recording:false,error:'voice_no_audio'}));
 f.overrides.voice_status=()=>state;
 f.overrides.voice_clear=({id})=>{assert.equal(id,'voice-1');state=status(null);};
 f.overrides.voice_start=args=>{assert.equal(args.key,'quick_input');assert.equal(args.base,"Quick draft");return state=status(session({id:'quick-recording',...args,body:args.base,text:''}));};
 f.overrides.voice_stop=()=>{};
 const v=voiceHook(f,'quick_input',"Quick draft");await f.settle();await v.hook.tree.toggle();await f.settle();
 assert.equal(state.session.id,'quick-recording');assert.equal(v.body(),"Quick draft");
 assert.equal(f.calls.filter(c=>c.name==='voice_clear').length,1);
});

test('stopping voice waits for durable text before applying and clearing audio without sending',async t=>{
 const clock=controlledTimers(),f=workspaceFixture(t,{native:true,timers:clock.timers});let state=status(null),saveDraft,submissions=0;
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 f.overrides.voice_status=()=>state;
 f.overrides.voice_start=()=>state=status(session({}));
 f.overrides.voice_stop=()=>state=status(session({recording:false,complete:true,text:"Transcript",body:"Original text transcript"}));
 f.overrides.draft_write=({draft})=>new Promise(resolve=>{saveDraft=()=>{f.db.set(draft.key,structuredClone(draft));resolve();};});
 f.overrides.voice_applied=({id})=>{assert.equal(f.db.get('input').body,"Original text transcript");assert.equal(id,state.session.id);state=status({...state.session,applied:true});};
 f.overrides.voice_clear=({id})=>{assert.equal(id,state.session.id);assert.equal(state.session.applied,true);state=status(null);};
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async()=>{submissions++;}});
 await f.settle();await formVoice(f,form).toggle();await f.settle();
 const stopping=formVoice(f,form).toggle();await f.settle();await clock.advance(120);await f.settle();
 assert(saveDraft);assert.equal(f.find(form,n=>n.type==='textarea').props.value,"Original text transcript");
 assert(!f.calls.some(c=>c.name==='voice_applied'||c.name==='voice_clear'));
 saveDraft();await stopping;await f.settle();
 assert.equal(state.session,null);assert.equal(formVoice(f,form).session,null);assert.equal(submissions,0);
 assert.equal(f.db.get('input').body,"Original text transcript");
 assert.equal(f.calls.filter(c=>c.name==='voice_applied').length,1);assert.equal(f.calls.filter(c=>c.name==='voice_clear').length,1);
});

for(const stage of ['flush','voice_applied','voice_clear']) {
 test(`failed ${stage} retains completed audio and an explicit retry safely clears it`,async t=>{
  const clock=controlledTimers(),f=workspaceFixture(t,{native:true,timers:clock.timers});let fail=true,durable='',v;
  let state=status(session({recording:false,complete:true,text:"Transcript",body:"Original text transcript"}));
  f.overrides.voice_status=()=>state;
  f.overrides.voice_applied=()=>{if(stage==='voice_applied'&&fail)throw Error('apply unavailable');assert.equal(durable,"Original text transcript");state=status({...state.session,applied:true});};
  f.overrides.voice_clear=()=>{if(stage==='voice_clear'&&fail)throw Error('cleanup unavailable');assert.equal(durable,"Original text transcript");assert.equal(state.session.applied,true);state=status(null);};
  v=voiceHook(f,'input',"Original text",async()=>{if(stage==='flush'&&fail)throw Error('draft unavailable');durable=v.body();});
  await f.settle();assert.equal(state.session.id,'voice-1');assert(v.hook.tree.error);
  if(stage==='flush')assert(!f.calls.some(c=>c.name==='voice_applied'||c.name==='voice_clear'));
  if(stage==='voice_applied')assert(!f.calls.some(c=>c.name==='voice_clear'));
  const attempts=f.calls.filter(c=>c.name==='voice_applied'||c.name==='voice_clear').length;
  await clock.advance(180);await f.settle();assert.equal(f.calls.filter(c=>c.name==='voice_applied'||c.name==='voice_clear').length,attempts,'polling does not retry a failed save/cleanup loop');
  fail=false;await v.hook.tree.retry();await f.settle();
  assert.equal(state.session,null);assert.equal(v.hook.tree.session,null);assert.equal(v.hook.tree.error,'');
 });
}

test('manual conflict recovery persists the appended text before automatic audio cleanup',async t=>{
 const f=workspaceFixture(t,{native:true});let state=status(session({recording:false,complete:true,text:"Transcript",body:"Original text transcript"})),durable='',v;
 f.overrides.voice_status=()=>state;
 f.overrides.voice_applied=()=>{assert.equal(durable,"Edits from another window\nTranscript");state=status({...state.session,applied:true});};
 f.overrides.voice_clear=()=>{assert.equal(state.session.applied,true);state=status(null);};
 v=voiceHook(f,'input',"Edits from another window",async()=>{durable=v.body();});await f.settle();
 assert.equal(v.hook.tree.conflicted,true);assert(!f.calls.some(c=>c.name==='voice_applied'||c.name==='voice_clear'));
 await v.hook.tree.recover();await f.settle();
 assert.equal(durable,"Edits from another window\nTranscript");assert.equal(state.session,null);
});

test('late automatic cleanup cannot hide or clear a new session from another window',async t=>{
 const clock=controlledTimers(),f=workspaceFixture(t,{native:true,timers:clock.timers});let acknowledge;
 let state=status(session({id:'old-completed',recording:false,complete:true,text:"Old result",body:"Original text old result"}));
 f.overrides.voice_status=()=>state;
 f.overrides.voice_applied=()=>new Promise(resolve=>{acknowledge=resolve;});
 f.overrides.voice_clear=({id})=>{if(state.session?.id===id)state=status(null);};
 const v=voiceHook(f);await f.settle();assert(acknowledge);
 state=status(session({id:'new-active',body:"Another input field",text:'',recording:true}));
 await clock.advance(180);await f.settle();
 acknowledge();await f.settle();
 assert.equal(state.session.id,'new-active');assert.equal(v.hook.tree.session.id,'new-active');
 assert.deepEqual(f.calls.filter(c=>c.name==='voice_clear').map(c=>c.args.id),['old-completed']);
});

test('empty results never display transcription complete or offer retry without audio',t=>{
 const f=workspaceFixture(t,{native:true}),Feedback=f.load('src/workspace/VoiceInput.tsx').VoiceFeedback;
 for(const error of ['voice_no_audio','voice_no_transcription',null]) {
  const view=f.mount(Feedback,{voice:{session:session({recording:false,complete:true,text:'  ',error}),active:false,clear(){},retry(){}}});
  assert(!f.text(view.tree).includes("Transcription complete"));
  const retry=f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==="Retry");
  assert.equal(retry,error==='voice_no_transcription');
  if(error==='voice_no_audio')assert(!f.text(view.tree).includes("Recording retained"));
 }
});
test('a status response after unmount cannot change the draft',async t=>{
 const f=workspaceFixture(t,{native:true});let resolve;
 f.overrides.voice_status=()=>new Promise(r=>{resolve=r;});
 const v=voiceHook(f);f.unmount(v.hook);
 resolve(status(session({body:"Late transcript",text:"Late transcript"})));await f.settle();
 assert.equal(v.body(),"Original text");
});
test('a status read started before clear cannot restore the removed session',async t=>{
 const timers=[];
 const f=workspaceFixture(t,{native:true,timers:{setTimeout:(fn,ms)=>{timers.push({fn,ms});return timers.length;},clearTimeout(){}}});
 let state=status(session({recording:false,error:'voice_no_transcription'})),resolve;
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
 f.overrides.discussion_submit=()=>f.topic;
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async()=>{saved++;}});
 await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
 assert.equal(saved,1);assert(!f.db.has('input'));
});

test('a delayed submit only clears its finished audio and preserves a later unrecognized recording',async t=>{
 for(const initialAudio of [false,true]) {
  const clock=controlledTimers(),f=workspaceFixture(t,{native:true,timers:clock.timers});let acknowledge;
  let state=status(initialAudio?session({id:'submitted-audio',recording:false,complete:true,applied:true,body:"Original text",text:"Original text"}):null);
  f.overrides.voice_status=()=>state;
  f.overrides.voice_clear=({id})=>{if(state.session?.id===id)state=status(null);};
  f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
  const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:()=>new Promise(resolve=>{acknowledge=resolve;})});
  await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();await f.settle();
  assert(acknowledge,'submission must be waiting for its acknowledgement');
  // The native session is global: another window can finish a new attempt while submission is pending.
  state=status(session({id:'later-retained-audio',recording:false,error:'voice_no_transcription',text:''}));
  acknowledge();await f.settle();
  assert.equal(state.session?.id,'later-retained-audio');
  assert.equal(state.session.error,'voice_no_transcription');
  const cleared=f.calls.filter(c=>c.name==='voice_clear').map(c=>c.args.id);
  assert.deepEqual(cleared,initialAudio?['submitted-audio']:[]);
  assert(!f.db.has('input'),'the acknowledged input itself still completes normally');
 }
});

test('a conflicting transcript cannot be discarded by starting another recording',async t=>{
 const f=workspaceFixture(t,{native:true});
 f.overrides.voice_status=()=>status(session({recording:false,complete:true,applied:false,body:"Original text transcript",text:"Transcript"}));
 f.overrides.voice_stop=()=>{};
 const v=voiceHook(f,'input',"Content changed in another editor");await f.settle();
 await v.hook.tree.toggle();await f.settle();
 assert(!f.calls.some(c=>c.name==='voice_clear'||c.name==='voice_start'||c.name==='voice_applied'));
 assert.equal(v.body(),"Content changed in another editor");
});
test('a failed installed model exposes repair and keeps downloads pausable',async t=>{
 const f=workspaceFixture(t,{native:true});let state={...status(null),state:'failed'};
 f.overrides.voice_status=()=>state;
 f.overrides.voice_control=()=>{state={...state,state:'downloading'};};
 const view=f.mount(f.load('src/workspace/VoiceSettings.tsx').default);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Check model files").props.onClick();await f.settle();
 assert(f.calls.some(c=>c.name==='voice_control'&&c.args.action==='download'));
 assert(f.find(view,n=>n.type==='button'&&f.text(n)==="Pause download"));
});

test('unresponsive microphone shows device recovery and dismisses without retrying transcription',async t=>{
 const f=workspaceFixture(t,{native:true});let cleared=false;
 const Feedback=f.load('src/workspace/VoiceInput.tsx').VoiceFeedback;
 const view=f.mount(Feedback,{voice:{active:false,busy:false,error:null,conflicted:false,session:session({recording:false,seconds:0,error:'microphone_start_timeout'}),clear(){cleared=true;},retry(){throw new Error('no audio to retry');}}});
 await f.settle();
 const alert=f.find(view,n=>n.props.role==='alert');
 assert(alert);
 const dismiss=f.find(view,n=>n.type==='button');
 assert.equal(dismiss.props['aria-label'],"Dismiss notification");
 assert.equal(f.nodes(view.tree).filter(n=>n.type==='button').length,1);
 dismiss.props.onClick();assert(cleared);
});


test('device startup timeout allows existing text to submit and clears only that session',async t=>{
 const f=workspaceFixture(t,{native:true});let saved=null,cleared=null;
 f.overrides.voice_status=()=>status(session({recording:false,seconds:0,error:'microphone_start_timeout'}));
 f.overrides.voice_clear=({id})=>{cleared=id;};
 f.db.set('input',{key:'input',request_id:'draft-1',title:'',body:"Original text",expected_version:null});
 const form=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{focus:0,onSubmit:async input=>{saved=input.text;return f.topic;}});
 await f.settle();f.find(form,n=>n.props.className==='send-button').props.onClick();
 for(let i=0;i<25&&!cleared;i++){await new Promise(r=>setTimeout(r,20));await f.settle();}
 assert.equal(saved,"Original text");assert.equal(cleared,'voice-1');
 assert(!f.calls.some(c=>c.name==='voice_retry'));
});
