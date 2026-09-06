import { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import logo from '../design-demo/brand/memivy-logo.svg';
import icon from '../design-demo/brand/memivy-icon.svg';
import '../design-demo/styles.css';
import './prototype.css';

type Capture = {id:string;text:string;source_app:string;created_at:number;ai_state:string};
type SearchPage = {items:Capture[];elapsed_ms:number;strategy:string};
type Diagnostics = {sqlite_version:string;fts5:boolean;journal_mode:string;synchronous:number;count:number;database_path:string;mcp_enabled:boolean;shortcut_error:string|null;last_focus_ms:number|null;last_commit_ms:number|null};
const failure = (e:unknown) => typeof e === 'string' ? e : '操作未完成，请重试';

function CaptureWindow() {
  const [draft,setDraft] = useState('');
  const [source,setSource] = useState('');
  const [attach,setAttach] = useState(false);
  const [error,setError] = useState('');
  const [saving,setSaving] = useState(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const busy = useRef(false);
  const composing = useRef(false);
  const dismissing = useRef(false);
  const requestId = useRef(crypto.randomUUID());
  const trace = (event:string) => { void invoke('capture_trace',{event,atMs:performance.now()}); };
  const dismiss = () => {
    if(dismissing.current) return;
    dismissing.current=true;
    trace('dismiss');
    void invoke('capture_hide').catch(e=>{dismissing.current=false;setError(failure(e));});
  };
  useEffect(() => {
    const ready = () => {
      dismissing.current=false;
      void invoke<string>('capture_context').then(setSource);
      requestAnimationFrame(() => { textarea.current?.focus(); void invoke('capture_ready'); });
    };
    // Only an explicit opening moves DOM focus. Re-activation by a mouse click
    // must leave that click's target alone instead of re-focusing the textarea.
    const focused = () => {
      if(document.activeElement===textarea.current) void invoke('capture_ready');
    };
    const escape = (event:KeyboardEvent) => {
      if(event.key!=='Escape' || event.isComposing || composing.current || event.keyCode===229) return;
      event.preventDefault();
      dismiss();
    };
    const listener = listen('capture-open', ready);
    window.addEventListener('focus', focused);
    window.addEventListener('keydown', escape);
    return () => { window.removeEventListener('focus',focused); window.removeEventListener('keydown',escape); void listener.then(unlisten => unlisten()); };
  },[]);
  const save = async () => {
    if (busy.current || composing.current || !draft.trim()) return;
    busy.current = true; setSaving(true); setError('');
    try {
      const started=performance.now();
      await invoke('capture_save', {requestId:requestId.current,text:draft,attachSource:attach});
      const submitRoundtripMs=performance.now()-started;
      setDraft(''); requestId.current=crypto.randomUUID(); setAttach(false);
      try { await invoke('capture_hide',{submitRoundtripMs}); }
      catch { setError('原话已保存，但窗口未能收起，可按 Esc 重试'); }
    } catch(e) { setError(`${failure(e)}，输入仍保留。`); }
    finally { busy.current=false; setSaving(false); }
  };
  return <section className="capture-dialog native-capture">
    <header className="dialog-header"><span className="capture-brand"><img src={icon}/>Memivy <span className="phase-tag">阶段 1</span></span><button aria-label="收起捕捉" onPointerDown={()=>trace('esc-down')}
      // WebKit 219670: IME + tap-to-click can deliver pointerup before pointerdown
      // and omit click. This reversible dismiss action accepts primary release;
      // click remains for keyboard/assistive activation, guarded against duplicates.
      onPointerUp={e=>{trace('esc-up');if(e.button===0)dismiss();}} onClick={()=>{trace('esc-click');dismiss();}}>Esc</button></header>
    <div className="capture-body"><h2>先留住这一刻。</h2>
      <textarea ref={textarea} className="capture-input" aria-label="原话输入" placeholder="一句想法，一段刚刚说过的话……" value={draft} disabled={saving}
        onFocus={()=>trace('input-focus')} onBlur={()=>trace('input-blur')}
        onChange={e => {setDraft(e.target.value); requestId.current=crypto.randomUUID();}}
        onCompositionStart={() => {composing.current=true;}} onCompositionEnd={() => {composing.current=false;}}
        onKeyDown={e => {if(e.nativeEvent.isComposing || composing.current || e.keyCode===229) return;
          if(e.key==='Enter' && !e.shiftKey){e.preventDefault(); void save();}
        }}/>
      <label className="attach-toggle"><input type="checkbox" checked={attach} disabled={saving} onChange={e=>{setAttach(e.target.checked);requestId.current=crypto.randomUUID();}}/>附带来源应用：{source || '未知应用'}</label>
      {error && <p className="prototype-error" role="alert">{error}</p>}
    </div><footer className="dialog-footer"><div className="capture-hint">Enter 保存 · Shift+Enter 换行<span>样机只保存原话，AI 整理尚未启用</span></div><button className="button primary" disabled={saving||!draft.trim()} onClick={() => void save()}>{saving?'正在保存…':'保存原话'}</button></footer>
  </section>;
}

function MainWindow() {
  const [query,setQuery] = useState('');
  const [page,setPage] = useState<SearchPage>({items:[],elapsed_ms:0,strategy:'recent'});
  const [selected,setSelected] = useState<string|null>(null);
  const [diag,setDiag] = useState<Diagnostics|null>(null);
  const [error,setError] = useState('');
  const [receipt,setReceipt] = useState('');
  const [verifying,setVerifying] = useState(false);
  useEffect(() => {
    let active=true;
    const refresh = async () => {try {
      const [next,info] = await Promise.all([invoke<SearchPage>('capture_search',{query}),invoke<Diagnostics>('diagnostics')]);
      if(active){setPage(next);setDiag(info);setError('');}
    } catch(e){if(active)setError(failure(e));}};
    const timer=setTimeout(()=>void refresh(),120);
    const poll=setInterval(()=>void refresh(),1500);
    const changed=listen<Capture>('capture-saved',event=>{setSelected(event.payload.id);setReceipt('原话已保存 · 等待后续整理');void refresh();});
    return ()=>{active=false;clearTimeout(timer);clearInterval(poll);void changed.then(f=>f());};
  },[query]);
  const current=page.items.find(item=>item.id===selected) || page.items[0];
  const toggle = async () => {try {await invoke('set_mcp_enabled',{enabled:!diag?.mcp_enabled});setDiag(await invoke('diagnostics'));}catch(e){setError(failure(e));}};
  return <div className="prototype-shell"><div className="prototype-banner">阶段 1 · 技术风险样机 <span>独立测试数据 · AI 整理尚未启用</span></div><div className="workspace">
    <aside className="sidebar"><div className="brand"><img src={logo} alt="Memivy"/></div><div className="sidebar-heading"><h1>留下的原话</h1><span className="count">{diag?.count||0}</span></div><p className="sidebar-subtitle">先记下来，稍后再整理。</p>
      <button className="button primary capture-trigger" onClick={()=>void invoke('show_capture').catch(e=>setError(failure(e)))}>记一下 <kbd>⌃⌥ M</kbd></button>
      <input className="prototype-search" type="search" aria-label="搜索原话" placeholder="搜索原话或来源…" value={query} onChange={e=>setQuery(e.target.value)}/>
      <div className="memory-list">{page.items.map(item=><button key={item.id} className={`memory-card ${item.id===current?.id?'selected':''}`} onClick={()=>setSelected(item.id)}><h2>{item.text.split('\n')[0].slice(0,50)}</h2><p>{item.text.slice(0,130)}</p><small>{item.source_app} · {new Date(item.created_at).toLocaleString('zh-CN')}</small></button>)}{!page.items.length&&<p className="prototype-empty">{query?'没有匹配的原话':'从一句话开始。'}</p>}</div>
      <div className="sidebar-footer"><button className="button secondary" onClick={()=>void toggle()}>MCP {diag?.mcp_enabled?'已开启':'已关闭'}</button></div></aside>
    <main className="main-pane"><div className="prototype-notice" role="status">{receipt||'这里只展示原话，尚未生成正式记忆。'}</div>{error&&<p className="prototype-error" role="alert">{error}</p>}{diag?.shortcut_error&&<p className="prototype-error">{diag.shortcut_error}</p>}
      {current?<article id="memory-detail"><span className="context-chip">已保存 · 等待整理</span><h1>原话</h1><div className="memory-prose prototype-raw">{current.text}</div><footer className="memory-footnote">来源：{current.source_app} · {new Date(current.created_at).toLocaleString('zh-CN')}<br/>记录 ID：{current.id}</footer></article>:<div className="prototype-empty">捕捉小窗和 MCP 保存的原话会出现在这里。</div>}
      <details className="prototype-diagnostics"><summary>样机验证信息</summary><button className="button secondary" disabled={verifying} onClick={()=>{setVerifying(true);void invoke('verify_external_capture').catch(e=>setError(failure(e))).finally(()=>setVerifying(false));}}>{verifying?'已安排，请切到待测应用…':'10 秒后测试跨应用唤起'}</button><pre>{diag?JSON.stringify({...diag,search_ms:page.elapsed_ms,search_strategy:page.strategy},null,2):'正在读取…'}</pre></details>
    </main></div></div>;
}

createRoot(document.getElementById('root')!).render(new URLSearchParams(location.search).get('window')==='capture'?<CaptureWindow/>:<MainWindow/>);
