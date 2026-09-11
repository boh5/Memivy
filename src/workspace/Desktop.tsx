import { useResourceBridge } from "./resources";
import { useCallback, useEffect, useRef, useState, type PointerEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import icon from "../../design-demo/brand/memivy-icon.svg";
import { Icon } from "../ui";
import { call, errorText, native, type Key, type Topic } from "./api";
import { ErrorNotice } from "./components";
import { useDesktop, useWindowLifecycle } from "./desktopApi";
import { flushDrafts } from "./useDraft";
import CaptureForm from "./CaptureForm";
import Discussion from "./Discussion";
import { installClickRecovery } from "./clickRecovery";
import "../prototype.css";
import "./workspace.css";
import "./desktop.css";

export default function Desktop() {
  const desktop = useDesktop(), state = desktop.state;
  const [error, setError] = useState(""), [saved, setSaved] = useState<Key | null>(null);
  const busy = useRef(false), current = useRef(state), root = useRef<HTMLDivElement>(null), composing = useRef(false);
  const drag = useRef<{ x: number; y: number; moved: boolean; tail: Promise<unknown> } | null>(null);
  const dragEnd = useRef<Promise<unknown>>(Promise.resolve()), suppressLeafClick = useRef(false);
  const dismissing = useRef(false);
  const pendingDismiss = useRef<{ reason: string; generation: number } | null>(null);
  current.current = state;
  useWindowLifecycle(setError);
  useResourceBridge(error => setError(errorText(error)));
  useEffect(() => { if (native && root.current) return installClickRecovery(root.current); }, []);
  useEffect(() => {
    if (!native || !root.current) return;
    let open = false, tail = Promise.resolve<unknown>(undefined);
    const observer = new MutationObserver(() => {
      const next = !!root.current?.querySelector("dialog[open]");
      if (next !== open) { open = next; tail = tail.then(() => call("desktop_modal", { open: next })).catch(e => setError(errorText(e))); }
    });
    observer.observe(root.current, { subtree: true, childList: true, attributes: true, attributeFilter: ["open"] });
    return () => { observer.disconnect(); void tail.then(() => call("desktop_modal", { open: false })); };
  }, []);
  const dismiss = useCallback(async (reason: string, generation = current.current?.generation) => {
    const requestedAt = Date.now();
    if (generation === undefined || dismissing.current || document.querySelector("dialog[open]")) return;
    if (busy.current) { pendingDismiss.current = { reason, generation }; return; }
    dismissing.current = true;
    try { await flushDrafts(); if (native) await call("desktop_dismiss", { generation, reason, requestedAt }); }
    catch (e) { setError(errorText(e)); }
    finally { dismissing.current = false; }
  }, []);
  function changeBusy(value: boolean) {
    busy.current = value;
    if (!value && pendingDismiss.current) {
      const pending = pendingDismiss.current; pendingDismiss.current = null;
      void dismiss(pending.reason, pending.generation);
    }
  }
  useEffect(() => {
    if (!native) return;
    const events = [
      listen("desktop-blur", () => { if (!composing.current) void dismiss("blur"); }),
      listen<number>("desktop-dismiss-request", e => void dismiss("explicit", e.payload)),
      listen("workspace-close-request", () => void dismiss("explicit")),
    ];
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, [dismiss]);
  async function change(patch: Parameters<typeof desktop.update>[0]) {
    if (busy.current) return;
    try { await flushDrafts(); await desktop.update(patch); setError(""); }
    catch (e) { setError(errorText(e)); }
  }
  async function expand(record: Key | null = null, settings = false) {
    if (busy.current) return;
    try { await flushDrafts(); await call("desktop_expand", { record, settings }); }
    catch (e) { setError(errorText(e)); }
  }
  async function ask(question: string, id: string) {
    if (!current.current?.configured) throw "先在主窗口连接一个模型，问题草稿会保留。";
    const topic = await call<Topic>("discussion_ask", { id, topicId: id, question, context: [] });
    await desktop.update({ topic_id: topic.id });
  }
  function reportReady() {
    if (native && current.current?.expanded) void call("desktop_ready", { generation: current.current.generation }).catch(() => {});
  }
  function pointerDown(e: PointerEvent<HTMLElement>) {
    const control = (e.target as HTMLElement).closest("button, input, textarea, a");
    if (e.button !== 0 || (control && control !== e.currentTarget)) return;
    suppressLeafClick.current = false;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, moved: false, tail: native ? call("desktop_drag", { phase: "start" }) : Promise.resolve() };
  }
  function pointerMove(e: PointerEvent<HTMLElement>) {
    const d = drag.current;
    if (!d) return;
    if (Math.hypot(e.clientX - d.x, e.clientY - d.y) > 5) d.moved = true;
    if (d.moved && native) d.tail = d.tail.then(() => call("desktop_drag", { phase: "move" }));
  }
  function pointerUp(e: PointerEvent<HTMLElement>) {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
    suppressLeafClick.current = d.moved || e.type !== "pointerup";
    dragEnd.current = d.tail.then(async () => {
      if (native) await call("desktop_drag", { phase: "end" });
    }).catch(e => setError(errorText(e)));
  }
  return <div ref={root} className={`formal-desktop ${state?.expanded ? "is-open" : ""}`}
    onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }}
    onKeyDown={e => { if (e.key === "Escape" && !e.repeat && !e.nativeEvent.isComposing && !composing.current && e.keyCode !== 229 && !document.querySelector("dialog[open]")) { e.preventDefault(); void dismiss("explicit"); } }}>
    {!state?.expanded ? <div className="desktop-rest">{state?.receipt && <div className="desktop-toast" role="status"><span>已存到本机</span><button onClick={() => void expand(state.last_memory ? { kind: "memory", id: state.last_memory } : null)}>查看</button></div>}<button className={`desktop-leaf ${saved ? "has-saved" : ""}`} aria-label={saved ? "已存到本机，打开 Memivy 快捷入口" : "打开 Memivy 快捷入口"}
      onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp}
      onClick={() => { if (!suppressLeafClick.current) void dragEnd.current.then(() => call("desktop_open")).catch(e => setError(errorText(e))); suppressLeafClick.current = false; }}>
      <img src={icon} alt="" draggable={false} />{saved && <span className="leaf-check"><Icon name="check" size={11} /></span>}
    </button></div> : <section className="desktop-panel" aria-label="Memivy 快捷入口">
      <header className="desktop-toolbar" onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={pointerUp}>
        <img src={icon} alt="" draggable={false} /><strong>Memivy</strong><span className="desktop-drag-space" />
        <button className={`icon-button ${state.pinned ? "selected" : ""}`} aria-label={state.pinned ? "取消固定窗口" : "固定窗口"} aria-pressed={state.pinned} title={state.pinned ? "已固定 · 点击外部仍保留" : "固定窗口"} onClick={() => void change({ pinned: !state.pinned })}><Icon name="pin" size={15} /></button>
        <button className="icon-button" aria-label="在主窗口继续" title="在主窗口继续" onClick={() => void expand()}><Icon name="expand" size={15} /></button>
        <button className="icon-button" aria-label="收起快捷窗口" title="收起 · Esc" onClick={() => void dismiss("explicit")}><Icon name="close" size={16} /></button>
      </header>
      <div className="desktop-content">
        {state.mode === "ask" && state.topic ? <>
          <div className="desktop-topic-nav"><button onClick={() => void change({ mode: "capture" })}>记一下</button><span>问一问</span><button onClick={() => void change({ clear_topic: true })}>新话题</button></div>
          <Discussion key={state.topic.id} compact topic={state.topic} configured={state.configured} onSettings={() => void expand(null, true)} onRefresh={() => {}} onOpenRecord={key => void expand(key)} onReady={reportReady} onBusy={changeBusy} />
        </> : <CaptureForm key={state.mode} quick sourceApp={state.source_app} mode={state.mode} focus={state.generation} onMode={mode => void change({ mode })} onAsk={ask}
          onBusy={changeBusy} onReady={reportReady} onEdit={() => setSaved(null)}
          onSaved={key => { setSaved(key); setTimeout(() => void dismiss("saved"), 0); }} />}
        {saved && <div className="desktop-saved" role="status"><Icon name="check" size={13} /><span>已存到本机</span><button onClick={() => void expand(saved)}>查看</button></div>}
        {state.mode === "ask" && !state.configured && !state.topic && <button className="connect-model-link" onClick={() => void expand(null, true)}>连接模型后即可提问</button>}
        <ErrorNotice text={error || desktop.error || state.error || ""} />
      </div>
    </section>}
  </div>;
}
