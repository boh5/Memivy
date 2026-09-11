import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, errorText, native } from "./api";
import { voiceBody } from "./voiceSession";
export type VoiceSession = {id:string;key:string;base:string;body:string;text:string;applied:boolean;recording:boolean;starting:boolean;processing:boolean;complete:boolean;error:string|null;seconds:number;level:number};
export type VoiceStatus = {enabled:boolean;preload:boolean;shortcut:string;state:string;backend:string|null;error:string|null;downloaded:number;bytes:number;cache:string;available:boolean;session:VoiceSession|null};
const finishers = new Set<() => Promise<void>>();
export async function finishVoiceInputs() { for (const finish of finishers) await finish(); }
export function useVoice(key:string, body:string, update:(body:string)=>void, flush:()=>Promise<unknown>, ready:boolean, selection:()=>[number,number]) {
  const [status,setStatus]=useState<VoiceStatus|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState(false);
  const current=useRef({key,body,update,flush,ready,selection}); current.current={key,body,update,flush,ready,selection};
  const detached=useRef<string|null>(null),sessionId=useRef<string|null>(null);
  const snapshot=useRef<VoiceStatus|null>(null),previous=useRef(body),alive=useRef(false);
  const operation=useRef<Promise<void>|null>(null),finishing=useRef<Promise<void>|null>(null);
  const revision=useRef(0),reads=useRef(0),acceptedRead=useRef(0);
  const usable=()=>alive.current&&current.current.key===key;
  function accept(s:VoiceStatus) {
    if(!usable())return;
    snapshot.current=s;setStatus(s);
    const v=s.session;
    if (!v || v.key!==key || !current.current.ready || detached.current===v.id || v.applied) return;
    if(sessionId.current!==v.id){sessionId.current=v.id;previous.current=v.base;}
    try {
      const next=voiceBody(current.current.body,previous.current,v.base,v.body);
      if (next!==current.current.body) {current.current.body=next;current.current.update(next);}
      previous.current=next;
      if(v.complete){
        detached.current=v.id;
        void current.current.flush().then(()=>call("voice_applied",{id:v.id})).catch(e=>{if(usable())setError(errorText(e));});
      }
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
      if(!native)return;
      let s=await refresh(),v=s.session;if(!v||v.key!==key)return;
      await mutate("voice_stop",{id:v.id});
      const until=Date.now()+200_000;
      while(v && !v.complete) {
        if(v.error)throw new Error(v.error);
        if(Date.now()>until)throw new Error("转写仍在处理中，录音和草稿已保留。");
        await new Promise(resolve=>setTimeout(resolve,120));s=await refresh();v=s.session?.key===key?s.session:null;
      }
      if(v){
        if(!usable()||!current.current.ready)return; // The persisted session will be recovered on the next mount.
        accept(s);
        if(!v.applied&&detached.current!==v.id)voiceBody(current.current.body,previous.current,v.base,v.body);
        await current.current.flush();await call("voice_applied",{id:v.id});
      }
    })();finishing.current=task;
    try{await task;}finally{finishing.current=null;}
  }
  async function finishInput() {
    // Closing/submitting during voice_start must wait for the start before stopping.
    if(operation.current)await operation.current;
    await finish();
  }
  useEffect(()=>{
    alive.current=true;let disposed=false,timer:ReturnType<typeof setTimeout>;
    const poll=async()=>{
      try{if(!operation.current)await refresh();}catch(e){if(!disposed)setError(errorText(e));}
      if(!disposed)timer=setTimeout(poll,snapshot.current?.session?.key===key?180:1500);
    };
    if(native)void poll();
    finishers.add(finishInput);
    return()=>{
      disposed=true;alive.current=false;revision.current++;clearTimeout(timer);finishers.delete(finishInput);
      const v=snapshot.current?.session;
      if(native&&v?.key===key)void call("voice_stop",{id:v.id}).catch(()=>{});
    };
  },[key]);
  useEffect(()=>{
    if(!native||key!=="quick_capture"||!ready)return;
    const take=async()=>{try{if(await call<boolean>("voice_take_shortcut"))await toggle();}catch(e){if(usable())setError(errorText(e));}};
    void take();const event=listen("voice-shortcut",()=>void take());return()=>{void event.then(stop=>stop());};
  },[key,ready]);
  async function run(work:()=>Promise<void>){
    if(operation.current)return operation.current;
    setBusy(true);setError("");
    const task=Promise.resolve().then(work).catch(e=>{if(usable())setError(errorText(e));});
    operation.current=task;
    try{await task;}finally{operation.current=null;if(usable())setBusy(false);}
  }
  async function toggle(){await run(async()=>{
    if(!native)throw new Error("请在 Memivy 桌面应用中使用麦克风。");
    const s=await refresh(),v=s.session;
    if(v?.key===key&&(v.recording||v.starting||v.processing)){await finish();return;}
    if(!s.enabled||!s.available){window.dispatchEvent(new Event("voice-settings-request"));throw new Error("在设置中启用并下载语音模型后，即可开始。");}
    if(v){
      if(v.key!==key&&!(v.complete&&v.applied))throw new Error("另一个输入框有录音草稿，请先在那里完成。");
      if(!v.complete)throw new Error(v.error||"请先完成上一段转写");
      if(v.key===key)await finish();
      await mutate("voice_clear",{id:v.id});
    }
    detached.current=null;
    const base=current.current.body,[start,end]=current.current.selection();previous.current=base;
    await current.current.flush();
    if(!usable())return;
    const started=await mutate<VoiceStatus>("voice_start",{key,base,prefix:base.slice(0,start),suffix:base.slice(end)});
    if(usable())accept(started);
    else if(started.session)await call("voice_stop",{id:started.session.id});
  });}
  async function clear(){await run(async()=>{
    const v=snapshot.current?.session;if(!v||v.key!==key)return;
    await current.current.flush();await mutate("voice_clear",{id:v.id});await refresh();
  });}
  async function retry(){await run(async()=>{
    const v=snapshot.current?.session;if(v?.key===key){await mutate("voice_retry",{id:v.id});await refresh();}
  });}
  async function consumed(){
    const v=snapshot.current?.session;if(v?.key!==key)return;
    // A completed capture stays successful even if removing temporary audio fails.
    try{await mutate("voice_clear",{id:v.id});snapshot.current=null;if(usable())setStatus(null);}
    catch(e){if(usable())setError(`内容已保存，暂存录音清理失败：${errorText(e)}`);}
  }
  const session=status?.session?.key===key?status.session:null;
  return {session,error,busy,status,toggle,finish:finishInput,clear,retry,consumed,active:!!session&&(session.recording||session.starting||session.processing),
    recover:()=>{if(session){const next=current.current.body+(current.current.body?"\n":"")+session.text;current.current.body=next;current.current.update(next);previous.current=next;detached.current=session.id;setError("");}}};
}
