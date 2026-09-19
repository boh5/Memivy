import {useEffect,useRef,useState} from "react";
import {call,errorText,native} from "./api";
import {Icon} from "../ui";
import {Modal,ErrorNotice} from "./components";
import GeneralSettings from "./GeneralSettings";
import BackupSettings from "./BackupSettings";
import McpSettings from "./McpSettings";
import ModelCapability from "./ModelCapability";
import ModelStorage from "./ModelStorage";
import ModelOverview from "./ModelOverview";
import {emptyBinding,modelNames,type Models,type Binding,type Kind,type EmbeddingStatus} from "./modelTypes";
import type {VoiceStatus} from "./useVoice";
import {useTranslation} from "react-i18next";
import {message} from "../i18n/messages";
import {useNotice} from "../i18n/react";
import "./settings.css";
import logo from "../../design-demo/brand/memivy-icon.svg";
type Page='general'|'ai'|'desktop'|'data'|'mcp'|Kind;
const tabs=[['general','settings','tabs.general'],['ai','spark','tabs.ai'],['data','database','tabs.data'],['mcp','link','tabs.mcp']] as const;
export default function SettingsPanel({onClose,onRestore,onChanged,initialPage="general"}:{initialPage?:Page;onClose:()=>void;onRestore:(id:string)=>void;onChanged:()=>void}){
 const {t}=useTranslation('settings');
 const [page,setPage]=useState<Page>(initialPage),[models,setModels]=useState<Models|null>(null),[embedding,setEmbedding]=useState<EmbeddingStatus|null>(null),[voice,setVoice]=useState<VoiceStatus|null>(null),[drafts,setDrafts]=useState<Partial<Record<Kind,Binding>>>({}),[error,setError]=useNotice(),[busy,setBusy]=useState(false),[backupBusy,setBackupBusy]=useState(false),[mcpBusy,setMcpBusy]=useState(false),[repair,setRepair]=useState(false),[notice,setNotice]=useNotice();
 const live=useRef(true),poll=useRef(0);const locked=busy||backupBusy||mcpBusy;
 useEffect(()=>{live.current=true;void call<Models>('models_load').then(m=>{if(live.current){setModels(m);if(initialPage in modelNames){const k=initialPage as Kind;setDrafts({[k]:m[k]??{...emptyBinding(),source:'service'}})}}}).catch(e=>{if(live.current)setError(errorText(e))});return()=>{live.current=false;poll.current++}},[]);
 async function refresh(){const n=++poll.current;const results=await Promise.allSettled([call<EmbeddingStatus>('embedding_status'),call<VoiceStatus>('voice_status')]);if(!live.current||n!==poll.current)return;const [e,v]=results;if(e.status==='fulfilled')setEmbedding(e.value);if(v.status==='fulfilled')setVoice(v.value);if(e.status==='rejected'||v.status==='rejected')setError(message('settings','status.partialModelReadFailed'));}
 useEffect(()=>{if(!notice)return;const timer=setTimeout(()=>setNotice(''),3000);return()=>clearTimeout(timer)},[notice,setNotice]);
 const pollsModels=page==='ai'||page==='data'||page in modelNames;
 useEffect(()=>{if(!pollsModels)return;void refresh();const t=setInterval(()=>void refresh(),1500);return()=>{clearInterval(t);poll.current++}},[pollsModels]);
 function navigate(next:Page){if(locked)return;setPage(next);setError('');setNotice('');if(models&&next in modelNames){const k=next as Kind;setDrafts(old=>({...old,[k]:old[k]??models[k]??{...emptyBinding(),source:'service'}}));}}
 function saved(m:Models){setModels(m);setNotice(message('settings','status.savedLocally'));onChanged();}
 const active=page in modelNames?'ai':page==='desktop'?'general':page;
 const tab=tabs.find(item=>item[0]===active);
 const title=page in modelNames?t(modelNames[page as Kind]):t(tab?.[2]??'title');
 return <Modal title={t('title')} className="model-settings" onClose={()=>{if(!locked)onClose()}}><div className="settings-shell"><aside className="settings-nav"><h2><img src={logo} alt=""/>{t('title')}</h2><nav aria-label={t('nav.categoryLabel')}>{tabs.map(([id,icon,label])=><button key={id} className={active===id?'active':''} aria-current={active===id?'page':undefined} disabled={locked} onClick={()=>navigate(id)}><Icon name={icon}/>{t(label)}</button>)}</nav></aside><main className="settings-main"><header className="model-page-header">{(page in modelNames)&&<button className="icon-button" aria-label={t('actions.backToModels')} disabled={locked} onClick={()=>navigate('ai')}><Icon name="back"/></button>}<div><h2>{title}</h2></div></header>
 {!native&&<p className="model-preview-note">{t('preview.readOnly')}</p>}
 {page==='ai'&&<div className="settings-page"><ModelOverview models={models} embedding={embedding} voice={voice} disabled={locked} onConfigure={navigate}/></div>}
 {page in modelNames&&models&&drafts[page as Kind]&&<ModelCapability key={page} kind={page as Kind} models={models} draft={drafts[page as Kind]!} setDraft={d=>setDrafts(old=>({...old,[page]:d}))} embedding={embedding} voice={voice} onRefresh={refresh} onModels={saved} onReconcile={setModels} onSaved={m=>{saved(m);setDrafts(old=>({...old,[page]:m[page as Kind]??undefined}));setPage('ai')}} onDesktop={()=>navigate('desktop')} onBusy={setBusy}/>}
 {(page==='general'||page==='desktop')&&<GeneralSettings onClose={onClose} focusEntry={page==='desktop'} onBusyChange={setBusy}/>}
 {page==='data'&&<div className="settings-page"><BackupSettings disabled={busy||mcpBusy} onBusyChange={setBackupBusy} onRestore={onRestore}/><ModelStorage models={models} embedding={embedding} voice={voice} onRefresh={refresh} onBusy={setBusy}/><details><summary>{t('searchMaintenance.title')}</summary><div className="setting-line"><div><strong>{t('searchMaintenance.rebuildTitle')}</strong><p>{t('searchMaintenance.rebuildDescription')}</p></div><button className="outline-button" disabled={locked||!native} onClick={()=>setRepair(true)}>{t('actions.repair')}</button></div><button className="model-text-button" onClick={()=>navigate('embedding')}>{t('actions.manageSemanticIndex')} →</button></details></div>}
 {page==='mcp'&&<div className="settings-page"><McpSettings onBusyChange={setMcpBusy}/></div>}
 {notice&&<p className="model-notice" role="status">{notice}</p>}{error&&<div className="model-error"><ErrorNotice text={error}/><button className="model-text-button" disabled={locked} onClick={()=>void call<Models>('models_load').then(m=>{setModels(m);setError('')}).catch(e=>setError(errorText(e)))}>{t('actions.reloadSettings')}</button></div>}
 </main></div>
 {repair&&<Modal title={t('searchMaintenance.dialogTitle')} onClose={()=>{if(!busy)setRepair(false)}}><p>{t('searchMaintenance.dialogDescription')}</p><div className="action-row"><button className="outline-button" disabled={busy} onClick={()=>setRepair(false)}>{t('actions.cancel')}</button><button className="send-button" disabled={busy} onClick={()=>{setBusy(true);void call('library_rebuild').then(()=>{setNotice(message('settings','status.searchIndexRepaired'));setRepair(false)}).catch(e=>setError(errorText(e))).finally(()=>setBusy(false))}}>{t('searchMaintenance.start')}</button></div></Modal>}
 </Modal>;
}
