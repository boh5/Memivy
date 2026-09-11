import {useEffect,useRef,useState} from "react";
import {call,errorText,native} from "./api";
import {Icon} from "../ui";
import {Modal,ErrorNotice} from "./components";
import DesktopSettings from "./DesktopSettings";
import BackupSettings from "./BackupSettings";
import McpSettings from "./McpSettings";
import VoiceSettings from "./VoiceSettings";
import ModelCapability from "./ModelCapability";
import ModelStorage from "./ModelStorage";
import {emptyBinding,indexLabel,modelNames,modelHints,type Models,type Binding,type Kind,type ConnectionDraft,type EmbeddingStatus} from "./modelTypes";
import type {VoiceStatus} from "./useVoice";
import "./settings.css";
import logo from "../../design-demo/brand/memivy-icon.svg";
type Page='ai'|'desktop'|'data'|'mcp'|Kind;
const tabs=[['ai','spark','AI 与模型'],['desktop','desktop','快捷入口'],['data','database','数据与存储'],['mcp','link','外部连接']] as const;
export default function SettingsPanel({onClose,onRestore,onChanged,initialPage="ai"}:{initialPage?:Page;onClose:()=>void;onRestore:(id:string)=>void;onChanged:()=>void}){
 const [page,setPage]=useState<Page>(initialPage),[models,setModels]=useState<Models|null>(null),[embedding,setEmbedding]=useState<EmbeddingStatus|null>(null),[voice,setVoice]=useState<VoiceStatus|null>(null),[drafts,setDrafts]=useState<Partial<Record<Kind,Binding>>>({}),[error,setError]=useState(''),[busy,setBusy]=useState(false),[backupBusy,setBackupBusy]=useState(false),[mcpBusy,setMcpBusy]=useState(false),[repair,setRepair]=useState(false),[notice,setNotice]=useState('');
 const [connectionDrafts,setConnectionDrafts]=useState<Partial<Record<Kind,ConnectionDraft|null>>>({});
 const live=useRef(true),poll=useRef(0);const locked=busy||backupBusy||mcpBusy;
 useEffect(()=>{live.current=true;void call<Models>('models_load').then(m=>{if(live.current){setModels(m);if(initialPage in modelNames){const k=initialPage as Kind;setDrafts({[k]:m[k]??{...emptyBinding(),source:'service',connection:''}})}}}).catch(e=>{if(live.current)setError(errorText(e))});return()=>{live.current=false;poll.current++}},[]);
 async function refresh(){const n=++poll.current;const results=await Promise.allSettled([call<EmbeddingStatus>('embedding_status'),call<VoiceStatus>('voice_status')]);if(!live.current||n!==poll.current)return;const [e,v]=results;if(e.status==='fulfilled')setEmbedding(e.value);if(v.status==='fulfilled')setVoice(v.value);if(e.status==='rejected'||v.status==='rejected')setError('部分模型状态读取失败，记录与基础搜索仍可使用。');}
 useEffect(()=>{if(!notice)return;const timer=setTimeout(()=>setNotice(""),3000);return()=>clearTimeout(timer)},[notice]);
 useEffect(()=>{void refresh();const t=setInterval(()=>void refresh(),1500);return()=>clearInterval(t)},[]);
 function navigate(next:Page){if(locked)return;setPage(next);setError('');setNotice('');if(models&&next in modelNames){const k=next as Kind;setDrafts(old=>({...old,[k]:old[k]??models[k]??{...emptyBinding(),source:'service',connection:''}}));}}
 function saved(m:Models){setModels(m);setNotice('设置已保存在本机');onChanged();}
 const active=page in modelNames?'ai':page;
 const title=page in modelNames?modelNames[page as Kind]:tabs.find(t=>t[0]===page)?.[2]??'设置';
 const sub=page in modelNames?modelHints[page as Kind]:page==='ai'?'为不同的事，选择合适的模型。':page==='desktop'?'在想法出现的时候，随手记下来。':page==='data'?'记忆留在本机，备份由你保管。':'让其他 AI 使用你在 Memivy 留下的记忆。';
 return <Modal title="设置" className="model-settings" onClose={()=>{if(!locked)onClose()}}><div className="settings-shell"><aside className="settings-nav"><h2><img src={logo} alt=""/>设置</h2><nav aria-label="设置分类">{tabs.map(([id,icon,label])=><button key={id} className={active===id?'active':''} aria-current={active===id?'page':undefined} disabled={locked} onClick={()=>navigate(id)}><Icon name={icon}/>{label}</button>)}</nav><p>你的记忆，你来做主<br/><small>Memivy</small></p></aside><main className="settings-main"><header className="model-page-header">{(page in modelNames)&&<button className="icon-button" aria-label="返回 AI 与模型" disabled={locked} onClick={()=>navigate('ai')}><Icon name="back"/></button>}<div><h2>{title}</h2><p>{sub}</p></div></header>
 {!native&&<p className="model-preview-note">浏览器为只读预览。请在 macOS 应用中配置模型。</p>}
 {page==='ai'&&<><div className="settings-page"><div className="model-capabilities">{(Object.keys(modelNames) as Kind[]).map(k=>{const b=models?.[k],enabled=k==='llm'?!!b:k==='embedding'?!!(embedding?.enabled||embedding?.preparing||embedding?.paused):!!voice?.enabled;const status=k==='embedding'?indexLabel(embedding):k==='voice'?(voice?.session?.error?'转写未完成':voice?.error?'需要检查':enabled?'已启用':'未开启'):enabled?'已配置':'未配置';const local=b?.source==='local';return <section key={k} className="model-capability"><div className="model-cap-icon"><Icon name={k==='llm'?'spark':k==='embedding'?'search':'mic'} size={23}/></div><div className="model-cap-body"><h3>{modelNames[k]}{enabled&&<span className="model-badge">{local?'本机运行':'模型服务'}</span>}</h3><p>{modelHints[k]}</p><small>{enabled?(local?k==='embedding'?'Qwen3-Embedding · 0.6B':'Qwen3-ASR · 0.6B':b?.model):k==='llm'?'连接你自己的模型服务':`推荐本地模型 · 下载约 ${k==='embedding'?'640 MB':'1.02 GB'}`}</small></div><div className="model-cap-side"><small>{models?status:'读取设置…'}</small><button className="model-text-button" disabled={!models||locked} onClick={()=>navigate(k)}>{enabled?'管理':'设置'} →</button></div></section>})}</div><p className="model-subtle"><Icon name="lock" size={14}/>不连接模型，也可以记录、编辑和使用基础搜索。</p></div><footer className="model-footer"><span>连接信息只保存在这台 Mac</span></footer></>}
 {page in modelNames&&models&&drafts[page as Kind]&&<ModelCapability connectionDraft={connectionDrafts[page as Kind]} setConnectionDraft={d=>setConnectionDrafts(old=>({...old,[page]:d}))} key={page} kind={page as Kind} models={models} draft={drafts[page as Kind]!} setDraft={d=>setDrafts(old=>({...old,[page]:d}))} embedding={embedding} voice={voice} onRefresh={refresh} onModels={saved} onReconcile={setModels} onSaved={m=>{setConnectionDrafts(old=>({...old,[page]:null}));saved(m);setDrafts(old=>({...old,[page]:m[page as Kind]??undefined}));setPage('ai')}} onDesktop={()=>navigate('desktop')} onBusy={setBusy}/>}
 {page==='desktop'&&<div className="settings-page"><DesktopSettings/><VoiceSettings shortcutOnly/></div>}
 {page==='data'&&<div className="settings-page"><BackupSettings disabled={busy||mcpBusy} onBusyChange={setBackupBusy} onRestore={onRestore}/><ModelStorage models={models} embedding={embedding} voice={voice} onRefresh={refresh} onBusy={setBusy}/><details><summary>搜索维护</summary><div className="setting-line"><div><strong>修复基础搜索索引</strong><p>从当前记忆重新生成，正文保持原样。</p></div><button className="outline-button" disabled={locked||!native} onClick={()=>setRepair(true)}>修复</button></div><button className="model-text-button" onClick={()=>navigate('embedding')}>管理语义索引 →</button></details></div>}
 {page==='mcp'&&<div className="settings-page"><McpSettings onBusyChange={setMcpBusy}/></div>}
 {notice&&<p className="model-notice" role="status">{notice}</p>}{error&&<div className="model-error"><ErrorNotice text={error}/><button className="model-text-button" disabled={locked} onClick={()=>void call<Models>('models_load').then(m=>{setModels(m);setError('')}).catch(e=>setError(errorText(e)))}>重新读取设置</button></div>}
 </main></div>
 {repair&&<Modal title="修复基础搜索索引？" onClose={()=>{if(!busy)setRepair(false)}}><p>从当前记忆重新生成索引，正文和草稿保持原样。</p><div className="action-row"><button className="outline-button" disabled={busy} onClick={()=>setRepair(false)}>取消</button><button className="send-button" disabled={busy} onClick={()=>{setBusy(true);void call('library_rebuild').then(()=>{setNotice('基础搜索索引已修复');setRepair(false)}).catch(e=>setError(errorText(e))).finally(()=>setBusy(false))}}>开始修复</button></div></Modal>}
 </Modal>;
}
