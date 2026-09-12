import {formatNumber} from "../i18n/format";
import { useEffect,useRef,useState } from "react";
import { Icon } from "../ui";
import { call,errorText,native } from "./api";
import { shortcutLabel } from "./desktopApi";
import type { VoiceStatus } from "./useVoice";
import "./voice.css";
import {ErrorNotice} from "./components";
import {useTranslation} from "react-i18next";
import {useNotice} from "../i18n/react";
import {translateCatalog} from "../i18n";
type VoiceStatusLabelKey = "voice.status.unloaded" | "voice.status.loading" | "voice.status.ready" | "voice.status.failed" | "voice.status.downloading";
const labels = {
 unloaded:"voice.status.unloaded",
 loading:"voice.status.loading",
 ready:"voice.status.ready",
 failed:"voice.status.failed",
 downloading:"voice.status.downloading",
} as const satisfies Record<string,VoiceStatusLabelKey>;
const statusLabelKey=(state:string):VoiceStatusLabelKey|undefined=>(labels as Record<string,VoiceStatusLabelKey>)[state];
export default function VoiceSettings({controlsOnly=false,shortcutOnly=false}:{controlsOnly?:boolean;shortcutOnly?:boolean}={}){
 const {t}=useTranslation('settings');
 const [status,setStatus]=useState<VoiceStatus|null>(null),[error,setError]=useNotice(),[busy,setBusy]=useState(false),[recording,setRecording]=useState(false);
 const acting=useRef(false),reads=useRef(0);
 useEffect(()=>{let alive=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{if(!acting.current){const n=++reads.current;try{const s=await call<VoiceStatus>("voice_status");if(alive&&n===reads.current)setStatus(s);}catch(e){if(alive)setError(errorText(e));}}if(alive)timer=setTimeout(poll,1000);};void poll();return()=>{alive=false;clearTimeout(timer);};},[]);
 async function act(action:string,value?:string){if(acting.current)return;acting.current=true;reads.current++;setBusy(true);setError("");try{await call("voice_control",{action,value:value??null});setStatus(await call<VoiceStatus>("voice_status"));}catch(e){setError(errorText(e));}finally{acting.current=false;setBusy(false);}}
 const downloading=status?.state==="downloading",loading=status?.state==="loading",percent=status?Math.min(100,Math.round(status.downloaded/status.bytes*100)):0,statusError=status?.error?translateCatalog(status.error,{ns:"errors"}):"",sessionError=status?.session?.error?translateCatalog(status.session.error,{ns:"errors"}):"";
 const shortcutControls=<><div className="setting-line"><div><strong>{t('voice.shortcutTitle')}</strong><p>{t('voice.shortcutDescription')}</p></div><button className="outline-button" disabled={busy||!native} onClick={e=>{e.currentTarget.focus();setRecording(true)}} onBlur={()=>setRecording(false)} onKeyDown={e=>{if(!recording)return;e.preventDefault();e.stopPropagation();if(e.key==='Escape'){setRecording(false);return}if(e.repeat||['Meta','Shift','Control','Alt'].includes(e.key)||e.nativeEvent.isComposing)return;setRecording(false);void act('shortcut',[...(e.ctrlKey?['Control']:[]),...(e.altKey?['Alt']:[]),...(e.shiftKey?['Shift']:[]),...(e.metaKey?['Super']:[]),e.code].join('+'))}}>{recording?t('voice.pressShortcut'):status?.shortcut?shortcutLabel(status.shortcut):t('voice.setShortcut')}</button></div>{status?.shortcut&&<button className="voice-text-button" disabled={busy} onClick={()=>void act('shortcut','')}>{t('voice.removeShortcut')}</button>}<ErrorNotice text={error}/></>;
 if(shortcutOnly)return <section className="settings-section">{shortcutControls}</section>;
 if(controlsOnly)return <section className="settings-section voice-settings controls-only"><div className="action-row">{!status?.local_available||downloading?<button className="outline-button" disabled={busy||!native||!status} onClick={()=>void act(downloading?'pause':'download')}>{downloading?t('voice.pauseDownload'):status?.downloaded?t('voice.resumeDownloadVerify'):t('voice.downloadModel')}</button>:<><button className="outline-button" disabled={busy||!native||status.source==='service'||!status.enabled||loading} onClick={()=>void act(status.state==='ready'?'unload':'load')}>{loading?t('voice.loadingEllipsis'):status.state==='ready'?t('voice.releaseRuntimeMemory'):t('voice.loadModel')}</button><button className="model-text-button" disabled={busy||!native} onClick={()=>void act('download')}>{t('voice.verifyRepairModel')}</button></>}</div>{downloading&&<progress aria-label={t('voice.downloadProgress')} max={100} value={percent}/>}<details><summary>{t('voice.localRuntimeOptions')}</summary><label className="checkbox-label"><input type="checkbox" checked={!!status?.preload} disabled={busy||!native} onChange={e=>void act('preload',String(e.target.checked))}/>{t('voice.preloadTitle')}</label><p className="field-help">{t('voice.preloadDescription')}</p></details><ErrorNotice text={error||statusError||sessionError}/></section>;
 return <section className="settings-section voice-settings" id="voice-settings">
   <div className="voice-heading"><div><h3>{t('voice.title')}</h3><p>{t('voice.description')}</p></div><span className="voice-local"><Icon name="leaf" size={13}/>{t('voice.localRecognition')}</span></div>
   <div className="voice-model-card">
     <div className="voice-model-heading"><div className="voice-model-icon"><Icon name="mic" size={22}/></div><div><strong>Qwen3-ASR <span>0.6B · Q8</span></strong><p>{t('voice.modelDescription')}</p></div>
        <button role="switch" aria-label={t('voice.enableLabel')} aria-checked={!!status?.enabled} className="voice-switch" disabled={busy||!native||!status} onClick={()=>void act(status?.enabled?"disable":"enable")}><i/></button>
     </div>
      <div className="voice-model-status" role="status"><span className={`voice-status-dot ${status?.state==="ready"?"ready":""}`}/><span>{!status?t('voice.readingStatus'):!status.available&&!downloading?t('voice.notDownloaded'):statusLabelKey(status.state)?t(statusLabelKey(status.state)!):status.state}</span>{status?.backend&&<span className="voice-backend">{status.backend}</span>}<span className="voice-model-size">1.02 GB</span></div>
     {downloading&&<div className="voice-download"><progress aria-label={t('voice.downloadProgress')} max={100} value={percent}/><span>{t('voice.downloadAmount',{percent,downloaded:formatNumber((status?.downloaded??0)/1e6)})}</span></div>}
     <div className="voice-model-actions">
       {!status?.available||downloading?<button className="send-button" disabled={busy||!native||!status} onClick={()=>void act(downloading?"pause":"download")}><Icon name={downloading?"stop":"download"} size={15}/>{downloading?t('voice.pauseDownload'):status?.downloaded?t('voice.resumeDownload'):t('voice.downloadModel')}</button>:<button className={status.state==="ready"?"outline-button":"send-button"} disabled={busy||loading||!status.enabled} onClick={()=>void act(status.state==="ready"?"unload":"load")}><Icon name={status.state==="ready"?"stop":"play"} size={14}/>{loading?t('voice.loadingEllipsis'):status.state==="ready"?t('voice.releaseModel'):t('voice.loadModel')}</button>}
       <span>{status?.state==="ready"?t('voice.idleRelease'):t('voice.onDemandGpu')}</span>
       {status?.available&&status.state==="failed"&&<button className="outline-button" disabled={busy||loading} onClick={()=>void act("download")}>{t('voice.verifyRepairModel')}</button>}
     </div>
   </div>
   <div className="setting-line"><div><strong>{t('voice.preloadTitle')}</strong><p>{t('voice.preloadMainDescription')}</p></div><input type="checkbox" aria-label={t('voice.preloadLabel')} checked={!!status?.preload} disabled={busy||!status?.enabled||!native} onChange={e=>void act("preload",String(e.target.checked))}/></div>
    <div className="setting-line"><div><strong>{t('voice.shortcutTitle')}</strong><p>{t('voice.shortcutDescription')}</p></div><button className={`outline-button shortcut-recorder ${recording?"recording":""}`} disabled={busy||!native||!status?.enabled} onClick={e=>{e.currentTarget.focus();setRecording(true);}} onBlur={()=>setRecording(false)} onKeyDown={e=>{
     if(!recording)return;e.preventDefault();e.stopPropagation();if(e.key==="Escape"){setRecording(false);return;}if(e.repeat||["Meta","Shift","Control","Alt"].includes(e.key)||e.nativeEvent.isComposing)return;
     const text=[...(e.ctrlKey?["Control"]:[]),...(e.altKey?["Alt"]:[]),...(e.shiftKey?["Shift"]:[]),...(e.metaKey?["Super"]:[]),e.code].join("+");setRecording(false);void act("shortcut",text);
   }}>{recording?t('voice.pressShortcut'):status?.shortcut?shortcutLabel(status.shortcut):t('voice.setShortcut')}</button></div>
   {status?.shortcut&&<button className="voice-text-button" disabled={busy} onClick={()=>void act("shortcut","")}>{t('voice.removeShortcut')}</button>}
   <p className="voice-footnote">{t('voice.footnote')}</p>
   <details className="voice-cache"><summary>{t('voice.cacheTitle')}</summary><p>{t('voice.cacheDescription')}</p><code>{status?.cache||t('voice.cacheReading')}</code><p>{t('voice.runtimeDescription')}</p></details>
   {(error||statusError||sessionError)&&<div className="voice-error" role="alert">{error||statusError||sessionError}</div>}
 </section>;
}
