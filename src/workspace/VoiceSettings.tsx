import { useEffect,useRef,useState } from "react";
import { call,errorText,native } from "./api";
import ShortcutSetting from "./ShortcutSetting";
import type { VoiceStatus } from "./useVoice";
import "./voice.css";
import {ErrorNotice} from "./components";
import {useTranslation} from "react-i18next";
import {useNotice} from "../i18n/react";
import {translateCatalog} from "../i18n";
export default function VoiceSettings({shortcutOnly=false,onBusyChange}:{shortcutOnly?:boolean;onBusyChange?:(busy:boolean)=>void}={}){
 const {t}=useTranslation('settings');
 const [status,setStatus]=useState<VoiceStatus|null>(null),[error,setError]=useNotice(),[busy,setBusy]=useState(false);
 const acting=useRef(false),reads=useRef(0);
 useEffect(()=>{let alive=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{if(!acting.current){const n=++reads.current;try{const s=await call<VoiceStatus>("voice_status");if(alive&&n===reads.current)setStatus(s);}catch(e){if(alive)setError(errorText(e));}}if(alive)timer=setTimeout(poll,1000);};void poll();return()=>{alive=false;clearTimeout(timer);};},[]);
 async function act(action:string,value?:string){if(acting.current)return;acting.current=true;reads.current++;setBusy(true);onBusyChange?.(true);setError("");try{await call("voice_control",{action,value:value??null});setStatus(await call<VoiceStatus>("voice_status"));}catch(e){setError(errorText(e));}finally{acting.current=false;setBusy(false);onBusyChange?.(false);}}
 const downloading=status?.state==="downloading",loading=status?.state==="loading",percent=status?Math.min(100,Math.round(status.downloaded/status.bytes*100)):0,statusError=status?.error?translateCatalog(status.error,{ns:"errors"}):"",sessionError=status?.session?.error?translateCatalog(status.session.error,{ns:"errors"}):"";
 const shortcutControls=<ShortcutSetting label={t('voice.shortcutTitle')} value={status?.shortcut??''} disabled={busy||!native||!status} onChange={value=>void act('shortcut',value)}/>;
 if(shortcutOnly)return <>{shortcutControls}<ErrorNotice text={error}/></>;
 return <section className="settings-section voice-settings controls-only"><div className="action-row">{!status?.local_available||downloading?<button className="outline-button" disabled={busy||!native||!status} onClick={()=>void act(downloading?'pause':'download')}>{downloading?t('voice.pauseDownload'):status?.downloaded?t('voice.resumeDownloadVerify'):t('voice.downloadModel')}</button>:<><button className="outline-button" disabled={busy||!native||status.source==='service'||!status.enabled||loading} onClick={()=>void act(status.state==='ready'?'unload':'load')}>{loading?t('voice.loadingEllipsis'):status.state==='ready'?t('voice.releaseRuntimeMemory'):t('voice.loadModel')}</button><button className="model-text-button" disabled={busy||!native} onClick={()=>void act('download')}>{t('voice.verifyRepairModel')}</button></>}</div>{downloading&&<progress aria-label={t('voice.downloadProgress')} max={100} value={percent}/>}<details><summary>{t('voice.localRuntimeOptions')}</summary><div className="setting-line"><strong>{t('voice.preloadTitle')}</strong><input type="checkbox" role="switch" className="settings-switch" aria-label={t('voice.preloadTitle')} checked={!!status?.preload} disabled={busy||!native} onChange={e=>void act('preload',String(e.target.checked))}/></div><p className="field-help">{t('voice.preloadDescription')}</p></details><ErrorNotice text={error||statusError||sessionError}/></section>;
}
