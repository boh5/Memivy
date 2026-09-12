import {formatNumber} from "../i18n/format";
import {useState} from "react";
import {call,errorText,native} from "./api";
import {Modal,ErrorNotice} from "./components";
import type {Models,EmbeddingStatus} from "./modelTypes";
import type {VoiceStatus} from "./useVoice";
import {useTranslation} from "react-i18next";
import {useNotice} from "../i18n/react";
export default function ModelStorage({models,embedding,voice,onRefresh,onBusy}:{models:Models|null;embedding:EmbeddingStatus|null;voice:VoiceStatus|null;onRefresh:()=>Promise<void>;onBusy:(b:boolean)=>void}){
 const {t}=useTranslation('settings');
 const [clear,setClear]=useState<'embedding'|'voice'|null>(null),[busy,setBusy]=useState(false),[error,setError]=useNotice();
 const sizes={embedding:embedding?.downloaded??0,voice:voice?.downloaded??0};
 async function remove(){if(!clear||busy)return;setBusy(true);onBusy(true);setError('');try{await call('models_clear',{kind:clear});await onRefresh();setClear(null)}catch(e){setError(errorText(e))}finally{setBusy(false);onBusy(false)}}
 return <details><summary>{t('storage.title')}</summary><p className="field-help">{t('storage.description')}</p>{(['embedding','voice'] as const).map(k=><div className="setting-line" key={k}><div><strong>{k==='embedding'?'Qwen3-Embedding':'Qwen3-ASR'}</strong><p>{formatNumber(sizes[k]/1e6)} MB · {t(sizes[k]?'storage.sharedFiles':'storage.notDownloaded')}</p></div><button className="outline-button" disabled={!sizes[k]||busy||!native} onClick={()=>setClear(k)}>{t('actions.deleteFile')}</button></div>)}<code className="model-cache-path">{voice?.cache||'~/Library/Caches/com.memivy.app/models/'}</code><ErrorNotice text={error}/>{clear&&<Modal title={t('storage.deleteDialogTitle')} onClose={()=>{if(!busy)setClear(null)}}><p>{t('storage.deleteDialogDescription',{size:formatNumber(sizes[clear]/1e6)})}</p><p>{t(models?.[clear].source==='local'?'storage.localModelDisabled':'storage.serviceUnaffected')}{t('storage.contentRetained')}</p><ErrorNotice text={error}/><div className="action-row"><button className="outline-button" disabled={busy} onClick={()=>setClear(null)}>{t('actions.cancel')}</button><button className="send-button" disabled={busy} onClick={()=>void remove()}>{busy?t('storage.deleting'):t('storage.deleteModelFile')}</button></div></Modal>}</details>
}
