import { message, type UiMessage } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, errorText, native } from "./api";
import { voiceBody } from "./voiceSession";
export type VoiceSession = {source:"local"|"service";label?:string;id:string;key:string;base:string;body:string;text:string;applied:boolean;recording:boolean;starting:boolean;processing:boolean;complete:boolean;error:string|null;seconds:number;level:number};
export type VoiceStatus = {source?:"local"|"service";label?:string;local_available?:boolean;enabled:boolean;preload:boolean;shortcut:string;state:string;backend:string|null;error:string|null;downloaded:number;bytes:number;cache:string;available:boolean;session:VoiceSession|null};
const finishers = new Set<() => Promise<void>>();
export async function finishVoiceInputs() { for (const finish of finishers) await finish(); }
const activeSession = (session: VoiceSession) => session.recording || session.starting || session.processing;
export function useVoice(key:string, body:string, update:(body:string)=>void, flush:()=>Promise<unknown>, ready:boolean, selection:()=>[number,number], shortcutTarget=false) {
  const [status,setStatus]=useState<VoiceStatus|null>(null),[error,setError,errorMessage]=useNotice(),[busy,setBusy]=useState(false);
  const current=useRef({key,body,update,flush,ready,selection}); current.current={key,body,update,flush,ready,selection};
  const detached=useRef<string|null>(null),sessionId=useRef<string|null>(null);
  // Draft identity is shared across surfaces; only the initiating input owns a live recording.
  const startedSessionId=useRef<string|null>(null);
  const snapshot=useRef<VoiceStatus|null>(null),previous=useRef(body),alive=useRef(false);
  const operation=useRef<Promise<void>|null>(null),finishing=useRef<Promise<string|null>|null>(null);
  const completion=useRef<{id:string;task:Promise<void>}|null>(null);
  const clearedSessionId=useRef<string|null>(null);
  const revision=useRef(0),reads=useRef(0),acceptedRead=useRef(0);
  const usable=()=>alive.current&&current.current.key===key;
  function hideSession(id:string) {
    if(snapshot.current?.session?.id!==id)return;
    const next={...snapshot.current,session:null};snapshot.current=next;
    if(usable()){setStatus(next);setError("");}
  }
  function applyTranscript(v:VoiceSession) {
    if(v.applied||detached.current===v.id||!v.text.trim())return;
    if(sessionId.current!==v.id){sessionId.current=v.id;previous.current=v.base;}
    const next=voiceBody(current.current.body,previous.current,v.base,v.body);
    if(next!==current.current.body){current.current.body=next;current.current.update(next);}
    previous.current=next;
    if(v.complete)detached.current=v.id;
  }
  async function releaseSession(id:string) {
    if(clearedSessionId.current===id)return;
    await mutate("voice_clear",{id});clearedSessionId.current=id;hideSession(id);
  }
  function completeTranscript(v:VoiceSession) {
    if(completion.current?.id===v.id)return completion.current.task;
    const flushDraft=current.current.flush;
    const task=(async()=>{
      // Keep the original input's flush function even if its surface changes while saving.
      await flushDraft();
      await mutate("voice_applied",{id:v.id});
      try {await releaseSession(v.id);}
      catch(e){if(usable()&&snapshot.current?.session?.id===v.id)setError(message("errors", "voice_cleanup_failed", {error:errorText(e)}));}
    })();
    completion.current={id:v.id,task};
    return task;
  }
  function accept(s:VoiceStatus) {
    if(!usable())return;
    snapshot.current=s;setStatus(s);
    const v=s.session;
    if (!v || v.key!==key || !current.current.ready) return;
    if (activeSession(v) && startedSessionId.current!==v.id) return;
    try {
      applyTranscript(v);
      if(v.complete&&(v.text.trim()||v.applied))void completeTranscript(v).catch(e=>{if(usable()&&snapshot.current?.session?.id===v.id)setError(errorText(e));});
    }catch(e){if(usable())setError(errorText(e));}
  }
  async function refresh() {
    const epoch=revision.current,n=++reads.current;
    const s=await call<VoiceStatus>("voice_status");
    if(epoch===revision.current&&n>acceptedRead.current){acceptedRead.current=n;accept(s);}
    return s;
  }
  async function mutate<T>(name:string,args:Record<string,unknown>) {
    // In-flight polls from before a mutation cannot resurrect a consumed session.
    revision.current++;
    try{return await call<T>(name,args);}finally{revision.current++;}
  }
  async function finish() {
    if(finishing.current)return finishing.current;
    const task=(async()=>{
      if(!native)return null;
      let s=await refresh(),v=s.session;if(!v||v.key!==key)return null;
      if(activeSession(v) && startedSessionId.current!==v.id) {
        throw { code: "voice_draft_exists" };
      }
      const id=v.id;
      if(activeSession(v))await mutate("voice_stop",{id});
      const until=Date.now()+200_000;
      while(v && !v.complete) {
        // An empty microphone attempt must not prevent sending an existing text draft.
        if(v.error==="voice_no_audio"||v.error==="microphone_start_timeout")return id;
        if(v.error)throw { code: v.error };
        if(Date.now()>until)throw { code: "voice_incomplete" };
        await new Promise(resolve=>setTimeout(resolve,120));s=await refresh();v=s.session?.id===id?s.session:null;
      }
      if(v){
        if(!usable()||!current.current.ready)return id; // The persisted session will be recovered on the next mount.
        applyTranscript(v);
        await completeTranscript(v);
      }
      return id;
    })();finishing.current=task;
    try{return await task;}finally{finishing.current=null;}
  }
  async function finishInput() {
    // Closing/submitting during voice_start must wait for the start before stopping.
    if(operation.current)await operation.current;
    return finish();
  }
  useEffect(()=>{
    alive.current=true;let disposed=false,timer:ReturnType<typeof setTimeout>;
    const poll=async()=>{
      try{if(!operation.current)await refresh();}catch(e){if(!disposed)setError(errorText(e));}
      if(!disposed)timer=setTimeout(poll,snapshot.current?.session?.key===key?180:1500);
    };
    if(native)void poll();
    const finishOwnedInput=async()=>{
      if(!native)return;
      if(operation.current)await operation.current;
      const s=await refresh(),v=s.session;
      if(v&&activeSession(v)&&startedSessionId.current!==v.id)return;
      await finish();
    };
    finishers.add(finishOwnedInput);
    return()=>{
      disposed=true;alive.current=false;revision.current++;clearTimeout(timer);finishers.delete(finishOwnedInput);
      const id=startedSessionId.current;startedSessionId.current=null;
      if(native&&id)void call("voice_stop",{id}).catch(()=>{});
    };
  },[key]);
  useEffect(()=>{
    if(!native||!shortcutTarget||!ready)return;
    const take=async()=>{try{if(await call<boolean>("voice_take_shortcut"))await toggle();}catch(e){if(usable())setError(errorText(e));}};
    void take();const event=listen("voice-shortcut",()=>void take());return()=>{void event.then(stop=>stop());};
  },[key,ready,shortcutTarget]);
  async function run(work:()=>Promise<void>){
    if(operation.current)return operation.current;
    setBusy(true);setError("");
    const task=Promise.resolve().then(work).catch(e=>{if(usable())setError(errorText(e));});
    operation.current=task;
    try{await task;}finally{operation.current=null;if(usable())setBusy(false);}
  }
  async function toggle(){await run(async()=>{
    if(!native)throw { code: "native_required" };
    const s=await refresh(),v=s.session;
    if(v?.key===key&&activeSession(v)){await finish();return;}
    if(!s.enabled||!s.available){window.dispatchEvent(new Event("voice-settings-request"));throw { code: "voice_disabled" };}
    if(v){
      if(v.key!==key&&!(v.complete&&v.applied)&&v.error!=="voice_no_audio")throw { code: "voice_draft_exists" };
      if(!v.complete&&v.error!=="voice_no_audio")throw { code: v.error || "voice_incomplete" };
      if(v.key===key)await finish();
      await releaseSession(v.id);
    }
    detached.current=null;
    const base=current.current.body,[start,end]=current.current.selection();previous.current=base;
    await current.current.flush();
    if(!usable())return;
    const started=await mutate<VoiceStatus>("voice_start",{key,base,prefix:base.slice(0,start),suffix:base.slice(end)});
    if(usable()) {startedSessionId.current=started.session?.id ?? null;accept(started);}
    else if(started.session)await call("voice_stop",{id:started.session.id});
  });}
  async function clear(){await run(async()=>{
    const v=snapshot.current?.session;if(!v||v.key!==key)return;
    await current.current.flush();await releaseSession(v.id);await refresh();
  });}
  async function retry(){await run(async()=>{
    const v=snapshot.current?.session;if(v?.key!==key)return;
    if(v.complete){
      applyTranscript(v);
      if(completion.current?.id===v.id){await completion.current.task.catch(()=>{});completion.current=null;}
      await completeTranscript(v);
    } else {await mutate("voice_retry",{id:v.id});startedSessionId.current=v.id;await refresh();}
  });}
  async function consumed(id:string|null){
    if(!id)return;
    // A completed capture stays successful even if removing temporary audio fails.
    try{
      // Native clear checks this exact ID; a later recording must survive an old submit acknowledgement.
      await releaseSession(id);
    }
    catch(e){if(usable())setError(message("errors", "voice_cleanup_failed", { error: errorText(e) }));}
  }
  const session=status?.session?.key===key?status.session:null;
  const active=!!session&&activeSession(session),owned=active&&startedSessionId.current===session?.id;
  return {session,error,conflicted:typeof errorMessage === "object" && errorMessage.ns === "errors" && errorMessage.key === "voice_draft_conflict",busy,status,toggle,finish:()=>finishInput(),clear,retry,consumed,active,owned,foreign:active&&!owned,
    recover:()=>run(async()=>{if(session){const next=current.current.body+(current.current.body?"\n":"")+session.text;current.current.body=next;current.current.update(next);previous.current=next;detached.current=session.id;await completeTranscript(session);}})};
}
