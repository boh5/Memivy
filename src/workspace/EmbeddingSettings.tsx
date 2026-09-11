import { useEffect, useRef, useState } from "react";
import { call, errorText } from "./api";
import { ErrorNotice } from "./components";
type Status = {enabled:boolean;preparing:boolean;paused:boolean;state:string;downloaded:number;bytes:number;processed:number;total:number;failed:number;error:string|null};
const labels:Record<string,string>={clearing:"正在清理模型与索引",not_downloaded:"尚未下载",downloading:"正在下载模型",warming:"正在校验并加载模型",indexing:"正在建立索引",ready:"语义检索已就绪",disabled:"已关闭，模型和索引保留",paused:"已暂停",failed:"准备未完成"};
export default function EmbeddingSettings(){
  const [status,setStatus]=useState<Status|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState(false);
  const reads = useRef(0), acting = useRef(false);
  useEffect(()=>{let disposed=false;const refresh=()=>{ if(acting.current)return; const request=++reads.current; return call<Status>("embedding_status").then(s=>{if(!disposed && request===reads.current)setStatus(s);}).catch(e=>{if(!disposed && request===reads.current)setError(errorText(e));}); };refresh();const timer=setInterval(refresh,1000);return()=>{disposed=true;clearInterval(timer);};},[]);
  async function act(action:string){if(acting.current)return;acting.current=true;const request=++reads.current;setBusy(true);setError("");try{await call("embedding_control",{action});const next=await call<Status>("embedding_status");if(request===reads.current)setStatus(next);}catch(e){setError(errorText(e));}finally{acting.current=false;setBusy(false);}}
  const disabled=busy||status?.state==="clearing";
  const preparing=!!status&&(status.preparing||status.state==="downloading"||status.state==="indexing"||status.state==="warming");
  return <section className="settings-section">
    <h3>语义检索</h3>
    <p>让所有搜索入口更懂含义，支持多语言和中英混写。模型在本机运行，问答仍使用下方配置的生成模型。</p>
    <div className="setting-line"><div><strong>Qwen3-Embedding · 0.6B Q8</strong><p>下载约 640 MB，索引另外占用本地空间。模型统一保存在 ~/Library/Caches/com.memivy.app/models/，所有 Memivy 资料库共用。</p></div></div>
    {status&&<>
      <p role="status">{labels[status.state]??status.state}{status.state==="downloading"?` · ${(status.downloaded/1_000_000).toFixed(0)} / ${(status.bytes/1_000_000).toFixed(0)} MB`:status.state==="indexing"||status.state==="ready"?` · 已索引 ${status.processed} / ${status.total} 条`:""}</p>
      {status.state==="downloading"&&<progress style={{width:"100%",accentColor:"#FFD02F"}} max={status.bytes} value={status.downloaded} aria-label="模型下载进度"/>}
      {status.failed>0&&<p>{status.failed} 条记忆编码失败，这些内容仍可用字面检索。</p>}
      <ErrorNotice text={status.error??""}/>
      <div className="action-row">
        {status.paused?<button className="send-button" disabled={disabled} onClick={()=>void act("resume")}>继续</button>:status.error||status.failed>0?<button className="send-button" disabled={disabled} onClick={()=>void act("retry")}>重试</button>:preparing?<button className="outline-button" disabled={disabled} onClick={()=>void act("pause")}>暂停</button>:!status.enabled?<button className="send-button" disabled={disabled} onClick={()=>void act("enable")}>{status.state==="not_downloaded"?"下载并启用":"开启语义检索"}</button>:<button className="outline-button" disabled={disabled} onClick={()=>void act("disable")}>关闭语义检索</button>}
        {(preparing||status.paused)&&<button className="outline-button" disabled={disabled} onClick={()=>void act("cancel")}>取消准备</button>}
        {!preparing&&!status.paused&&status.downloaded>0&&<><button className="outline-button" disabled={disabled} onClick={()=>void act("rebuild")}>重建向量索引</button><button className="outline-button" disabled={disabled} onClick={()=>void act("clear")}>清理模型与索引</button></>}
      </div>
    </>}
    <ErrorNotice text={error}/>
  </section>;
}
