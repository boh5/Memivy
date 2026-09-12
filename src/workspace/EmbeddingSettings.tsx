import {formatNumber} from "../i18n/format";
import { useEffect, useRef, useState } from "react";
import { call, errorText } from "./api";
import { ErrorNotice } from "./components";
import {useTranslation} from "react-i18next";
import {useNotice} from "../i18n/react";
import {translateCatalog} from "../i18n";
type Status = {enabled:boolean;preparing:boolean;paused:boolean;state:string;downloaded:number;bytes:number;processed:number;total:number;failed:number;error:string|null};
type EmbeddingStatusLabelKey = "embedding.status.clearing" | "embedding.status.notDownloaded" | "embedding.status.downloading" | "embedding.status.warming" | "embedding.status.indexing" | "embedding.status.ready" | "embedding.status.disabled" | "embedding.status.paused" | "embedding.status.failed";
const labels = {
 clearing:"embedding.status.clearing",
 not_downloaded:"embedding.status.notDownloaded",
 downloading:"embedding.status.downloading",
 warming:"embedding.status.warming",
 indexing:"embedding.status.indexing",
 ready:"embedding.status.ready",
 disabled:"embedding.status.disabled",
 paused:"embedding.status.paused",
 failed:"embedding.status.failed",
} as const satisfies Record<string,EmbeddingStatusLabelKey>;
const statusLabelKey=(state:string):EmbeddingStatusLabelKey|undefined=>(labels as Record<string,EmbeddingStatusLabelKey>)[state];
export default function EmbeddingSettings(){
  const {t}=useTranslation('settings');
  const [status,setStatus]=useState<Status|null>(null),[error,setError]=useNotice(),[busy,setBusy]=useState(false);
  const reads = useRef(0), acting = useRef(false);
  useEffect(()=>{let disposed=false;const refresh=()=>{ if(acting.current)return; const request=++reads.current; return call<Status>("embedding_status").then(s=>{if(!disposed && request===reads.current)setStatus(s);}).catch(e=>{if(!disposed && request===reads.current)setError(errorText(e));}); };refresh();const timer=setInterval(refresh,1000);return()=>{disposed=true;clearInterval(timer);};},[]);
  async function act(action:string){if(acting.current)return;acting.current=true;const request=++reads.current;setBusy(true);setError("");try{await call("embedding_control",{action});const next=await call<Status>("embedding_status");if(request===reads.current)setStatus(next);}catch(e){setError(errorText(e));}finally{acting.current=false;setBusy(false);}}
  const disabled=busy||status?.state==="clearing";
  const preparing=!!status&&(status.preparing||status.state==="downloading"||status.state==="indexing"||status.state==="warming");
  const statusError=status?.error?translateCatalog(status.error,{ns:"errors"}):"";
  return <section className="settings-section">
    <h3>{t('embedding.title')}</h3>
    <p>{t('embedding.description')}</p>
    <div className="setting-line"><div><strong>Qwen3-Embedding · 0.6B Q8</strong><p>{t('embedding.modelDescription')}</p></div></div>
    {status&&<>
      <p role="status">{statusLabelKey(status.state)?t(statusLabelKey(status.state)!):status.state}{status.state==="downloading"?t('embedding.downloadAmount',{downloaded:formatNumber(status.downloaded/1_000_000),total:formatNumber(status.bytes/1_000_000)}):status.state==="indexing"||status.state==="ready"?t('embedding.indexed',{processed:status.processed,total:status.total}):""}</p>
      {status.state==="downloading"&&<progress style={{width:"100%",accentColor:"#FFD02F"}} max={status.bytes} value={status.downloaded} aria-label={t('embedding.downloadProgress')}/>}
      {status.failed>0&&<p>{t('embedding.failedMemories',{count:status.failed})}</p>}
      <ErrorNotice text={statusError}/>
      <div className="action-row">
        {status.paused?<button className="send-button" disabled={disabled} onClick={()=>void act("resume")}>{t('actions.resume')}</button>:status.error||status.failed>0?<button className="send-button" disabled={disabled} onClick={()=>void act("retry")}>{t('actions.retry')}</button>:preparing?<button className="outline-button" disabled={disabled} onClick={()=>void act("pause")}>{t('actions.pause')}</button>:!status.enabled?<button className="send-button" disabled={disabled} onClick={()=>void act("enable")}>{status.state==="not_downloaded"?t('embedding.downloadAndEnable'):t('embedding.enable')}</button>:<button className="outline-button" disabled={disabled} onClick={()=>void act("disable")}>{t('embedding.disable')}</button>}
        {(preparing||status.paused)&&<button className="outline-button" disabled={disabled} onClick={()=>void act("cancel")}>{t('actions.cancelPreparation')}</button>}
        {!preparing&&!status.paused&&status.downloaded>0&&<><button className="outline-button" disabled={disabled} onClick={()=>void act("rebuild")}>{t('actions.rebuildVectorIndex')}</button><button className="outline-button" disabled={disabled} onClick={()=>void act("clear")}>{t('embedding.clear')}</button></>}
      </div>
    </>}
    <ErrorNotice text={error}/>
  </section>;
}
