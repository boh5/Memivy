import { useEffect,useRef,useState } from "react";
import { Icon } from "../ui";
import { call,errorText,native } from "./api";
import { shortcutLabel } from "./desktopApi";
import type { VoiceStatus } from "./useVoice";
import "./voice.css";
import {ErrorNotice} from "./components";
const labels:Record<string,string>={unloaded:"尚未加载",loading:"正在加载模型",ready:"模型已就绪",failed:"加载未完成",downloading:"正在下载"};
export default function VoiceSettings({controlsOnly=false,shortcutOnly=false}:{controlsOnly?:boolean;shortcutOnly?:boolean}={}){
 const [status,setStatus]=useState<VoiceStatus|null>(null),[error,setError]=useState(""),[busy,setBusy]=useState(false),[recording,setRecording]=useState(false);
 const acting=useRef(false),reads=useRef(0);
 useEffect(()=>{let alive=true;let timer:ReturnType<typeof setTimeout>;const poll=async()=>{if(!acting.current){const n=++reads.current;try{const s=await call<VoiceStatus>("voice_status");if(alive&&n===reads.current)setStatus(s);}catch(e){if(alive)setError(errorText(e));}}if(alive)timer=setTimeout(poll,1000);};void poll();return()=>{alive=false;clearTimeout(timer);};},[]);
 async function act(action:string,value?:string){if(acting.current)return;acting.current=true;reads.current++;setBusy(true);setError("");try{await call("voice_control",{action,value:value??null});setStatus(await call<VoiceStatus>("voice_status"));}catch(e){setError(errorText(e));}finally{acting.current=false;setBusy(false);}}
 const downloading=status?.state==="downloading",loading=status?.state==="loading",percent=status?Math.min(100,Math.round(status.downloaded/status.bytes*100)):0;
 const shortcutControls=<><div className="setting-line"><div><strong>语音快捷键</strong><p>按一下打开快捷入口并录音，再按一下结束。</p></div><button className="outline-button" disabled={busy||!native} onClick={e=>{e.currentTarget.focus();setRecording(true)}} onBlur={()=>setRecording(false)} onKeyDown={e=>{if(!recording)return;e.preventDefault();e.stopPropagation();if(e.key==='Escape'){setRecording(false);return}if(e.repeat||['Meta','Shift','Control','Alt'].includes(e.key)||e.nativeEvent.isComposing)return;setRecording(false);void act('shortcut',[...(e.ctrlKey?['Control']:[]),...(e.altKey?['Alt']:[]),...(e.shiftKey?['Shift']:[]),...(e.metaKey?['Super']:[]),e.code].join('+'))}}>{recording?'请按快捷键…':status?.shortcut?shortcutLabel(status.shortcut):'设置快捷键'}</button></div>{status?.shortcut&&<button className="voice-text-button" disabled={busy} onClick={()=>void act('shortcut','')}>移除语音快捷键</button>}<ErrorNotice text={error}/></>;
 if(shortcutOnly)return <section className="settings-section">{shortcutControls}</section>;
 if(controlsOnly)return <section className="settings-section voice-settings controls-only"><div className="action-row">{!status?.local_available||downloading?<button className="outline-button" disabled={busy||!native||!status} onClick={()=>void act(downloading?'pause':'download')}>{downloading?'暂停下载':status?.downloaded?'继续下载 / 校验模型':'下载模型'}</button>:<><button className="outline-button" disabled={busy||!native||status.source==='service'||!status.enabled||loading} onClick={()=>void act(status.state==='ready'?'unload':'load')}>{loading?'加载中…':status.state==='ready'?'释放运行内存':'加载模型'}</button><button className="model-text-button" disabled={busy||!native} onClick={()=>void act('download')}>校验并修复模型</button></>}</div>{downloading&&<progress aria-label="语音模型下载进度" max={100} value={percent}/>}<details><summary>本地运行选项</summary><label className="checkbox-label"><input type="checkbox" checked={!!status?.preload} disabled={busy||!native} onChange={e=>void act('preload',String(e.target.checked))}/>启动时预加载</label><p className="field-help">按需加载，闲置 1 分钟后释放运行内存。关闭功能不删除模型文件。</p></details><ErrorNotice text={error||status?.error||''}/></section>;
 return <section className="settings-section voice-settings" id="voice-settings">
   <div className="voice-heading"><div><h3>语音输入</h3><p>开口记下想法，也可以直接提问。</p></div><span className="voice-local"><Icon name="leaf" size={13}/>本机识别</span></div>
   <div className="voice-model-card">
     <div className="voice-model-heading"><div className="voice-model-icon"><Icon name="mic" size={22}/></div><div><strong>Qwen3-ASR <span>0.6B · Q8</span></strong><p>中文、英文及多语言 · 无需 API 密钥</p></div>
       <button role="switch" aria-label="启用语音输入" aria-checked={!!status?.enabled} className="voice-switch" disabled={busy||!native||!status} onClick={()=>void act(status?.enabled?"disable":"enable")}><i/></button>
     </div>
     <div className="voice-model-status" role="status"><span className={`voice-status-dot ${status?.state==="ready"?"ready":""}`}/><span>{!status?"读取模型状态…":!status.available&&!downloading?"模型尚未下载":labels[status.state]||status.state}</span>{status?.backend&&<span className="voice-backend">{status.backend}</span>}<span className="voice-model-size">1.02 GB</span></div>
     {downloading&&<div className="voice-download"><progress aria-label="语音模型下载进度" max={100} value={percent}/><span>{percent}% · {((status?.downloaded??0)/1e6).toFixed(0)} MB</span></div>}
     <div className="voice-model-actions">
       {!status?.available||downloading?<button className="send-button" disabled={busy||!native||!status} onClick={()=>void act(downloading?"pause":"download")}><Icon name={downloading?"stop":"download"} size={15}/>{downloading?"暂停下载":status?.downloaded?"继续下载":"下载模型"}</button>:<button className={status.state==="ready"?"outline-button":"send-button"} disabled={busy||loading||!status.enabled} onClick={()=>void act(status.state==="ready"?"unload":"load")}><Icon name={status.state==="ready"?"stop":"play"} size={14}/>{loading?"正在加载…":status.state==="ready"?"释放模型":"加载模型"}</button>}
       <span>{status?.state==="ready"?"闲置 1 分钟后自动释放":"按需加载，优先使用 GPU"}</span>
       {status?.available&&status.state==="failed"&&<button className="outline-button" disabled={busy||loading} onClick={()=>void act("download")}>校验并修复模型</button>}
     </div>
   </div>
   <div className="setting-line"><div><strong>启动时预加载</strong><p>打开 Memivy 时准备模型，第一次开口少等一会。</p></div><input type="checkbox" aria-label="启动时预加载语音模型" checked={!!status?.preload} disabled={busy||!status?.enabled||!native} onChange={e=>void act("preload",String(e.target.checked))}/></div>
   <div className="setting-line"><div><strong>语音快捷键</strong><p>按一下打开快捷入口并录音，再按一下结束。</p></div><button className={`outline-button shortcut-recorder ${recording?"recording":""}`} disabled={busy||!native||!status?.enabled} onClick={e=>{e.currentTarget.focus();setRecording(true);}} onBlur={()=>setRecording(false)} onKeyDown={e=>{
     if(!recording)return;e.preventDefault();e.stopPropagation();if(e.key==="Escape"){setRecording(false);return;}if(e.repeat||["Meta","Shift","Control","Alt"].includes(e.key)||e.nativeEvent.isComposing)return;
     const text=[...(e.ctrlKey?["Control"]:[]),...(e.altKey?["Alt"]:[]),...(e.shiftKey?["Shift"]:[]),...(e.metaKey?["Super"]:[]),e.code].join("+");setRecording(false);void act("shortcut",text);
   }}>{recording?"请按快捷键…":status?.shortcut?shortcutLabel(status.shortcut):"设置快捷键"}</button></div>
   {status?.shortcut&&<button className="voice-text-button" disabled={busy} onClick={()=>void act("shortcut","")}>移除语音快捷键</button>}
   <p className="voice-footnote">录音时按停顿分段转写，停止后可修改文字，再由你提交。单次最长 5 分钟。</p>
   <details className="voice-cache"><summary>模型存储与运行</summary><p>语音与语义检索共用 Memivy 模型缓存；开发版、安装版与资料库共用权重。关闭功能不会删除模型；缓存被清理后可重新下载。</p><code>{status?.cache||"正在读取模型目录…"}</code><p>GPU 同时处理音频编码和文字解码；不可用时回退 CPU。录音和转写期间暂停新的后台向量索引批次。暂存录音在提交或移除后清理。</p></details>
   {(error||status?.error)&&<div className="voice-error" role="alert">{error||status?.error}</div>}
 </section>;
}
