import {useState} from "react";
import {call,errorText,native} from "./api";
import {Modal,ErrorNotice} from "./components";
import type {Models,EmbeddingStatus} from "./modelTypes";
import type {VoiceStatus} from "./useVoice";
export default function ModelStorage({models,embedding,voice,onRefresh,onBusy}:{models:Models|null;embedding:EmbeddingStatus|null;voice:VoiceStatus|null;onRefresh:()=>Promise<void>;onBusy:(b:boolean)=>void}){
 const [clear,setClear]=useState<'embedding'|'voice'|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const sizes={embedding:embedding?.downloaded??0,voice:voice?.downloaded??0};
 async function remove(){if(!clear||busy)return;setBusy(true);onBusy(true);setError('');try{await call('models_clear',{kind:clear});await onRefresh();setClear(null)}catch(e){setError(errorText(e))}finally{setBusy(false);onBusy(false)}}
 return <details><summary>管理本地模型文件</summary><p className="field-help">开发版、安装版与各资料库共用模型权重。删除文件不删除记忆；其他资料库再次使用时需要重新下载。</p>{(['embedding','voice'] as const).map(k=><div className="setting-line" key={k}><div><strong>{k==='embedding'?'Qwen3-Embedding':'Qwen3-ASR'}</strong><p>{(sizes[k]/1e6).toFixed(0)} MB · {sizes[k]?'共用模型文件':'尚未下载'}</p></div><button className="outline-button" disabled={!sizes[k]||busy||!native} onClick={()=>setClear(k)}>删除文件</button></div>)}<code className="model-cache-path">{voice?.cache??'~/Library/Caches/com.memivy.app/models/'}</code><ErrorNotice text={error}/>{clear&&<Modal title="删除已下载的模型？" onClose={()=>{if(!busy)setClear(null)}}><p>将释放约 {(sizes[clear]/1e6).toFixed(0)} MB。其他 Memivy 资料库也会受到影响，再次使用时需要重新下载。</p><p>{models?.[clear].source==='local'?'当前使用此本地模型的能力会关闭。':'当前使用模型服务的能力不受影响。'}记忆、原话和草稿保留。</p><ErrorNotice text={error}/><div className="action-row"><button className="outline-button" disabled={busy} onClick={()=>setClear(null)}>取消</button><button className="send-button" disabled={busy} onClick={()=>void remove()}>{busy?'删除中…':'删除模型文件'}</button></div></Modal>}</details>
}
