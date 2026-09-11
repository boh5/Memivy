import { finishVoiceInputs } from "./useVoice";
import { useResourceBridge, useResourceVersion } from "./resources";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "../ui";
import { call, errorText, keyOf, native, uid, type Collection, type Page, type Row, type Detail, type Key, type Receipt, type Settings, type Source, type Topic } from "./api";
import { Empty, ErrorNotice } from "./components";
import { flushDrafts, refreshDrafts } from "./useDraft";
import { installClickRecovery } from "./clickRecovery";
import MemoryDetail from "./MemoryDetail";
import MemoryList from "./MemoryList";
import WorkspaceQuery from "./WorkspaceQuery";
import WorkspaceSidebar, { type WorkspacePage } from "./WorkspaceSidebar";
import SettingsPanel from "./Settings";
import Discussion from "./Discussion";
import CaptureForm from "./CaptureForm";
import CollectionEditor from "./CollectionEditor";
import CollectionSuggestions from "./CollectionSuggestions";
import { Modal, MoreMenu } from "./components";
import CaptureDialog from "./CaptureDialog";
import Toast, { notify } from "./Toast";
import { useDesktop, useWindowLifecycle, type MainRoute } from "./desktopApi";
import "../prototype.css";
import "./workspace.css";
import "./desktop.css";
import "./navigation.css";
import "./topbar.css";

type Handoff = { generation: number; target: "capture" | "query" | "topic" | "view" };

export default function App() {
  const [collections, setCollections] = useState<Collection[]>([]), [pins, setPins] = useState<Row[]>([]);
  const [collectionId, setCollectionId] = useState<string | null>(null);
  const [collectionEditor, setCollectionEditor] = useState<Collection | "new" | null>(null);
  const [suggestions, setSuggestions] = useState<Collection | null>(null), [archiveConfirm, setArchiveConfirm] = useState<Collection | null>(null);
  const [collectionBusy, setCollectionBusy] = useState(false);
  const collectionLock = useRef(false);
  const [page, setPage] = useState<WorkspacePage>("library");
  const [selected, setSelected] = useState<Key | null>(null);
  const [topic, setTopic] = useState<Topic | null>(null), [topics, setTopics] = useState<Topic[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false), [configured, setConfigured] = useState(false);
  const [settingsInitialPage, setSettingsInitialPage] = useState<"ai"|"voice">("ai");
  useEffect(() => { if (!settingsOpen) setSettingsInitialPage("ai"); }, [settingsOpen]);
  useEffect(() => { const open = () => {setSettingsInitialPage("voice"); setSettingsOpen(true);}; window.addEventListener("voice-settings-request", open); return () => window.removeEventListener("voice-settings-request", open); }, []);
  const [windowError, setWindowError] = useState("");
  const navigationRevision = useResourceVersion([{domain:"memory"},{domain:"navigation"},{domain:"collection"}]);
  const topicsRevision = useResourceVersion([{domain:"discussion"},{domain:"collection"}]);
  const settingsRevision = useResourceVersion([{domain:"settings"}]);
  useResourceBridge(error => setWindowError(errorText(error)));
  const [captureOpen, setCaptureOpen] = useState(false), [captureQuick, setCaptureQuick] = useState(false);
  const [recallQuick, setRecallQuick] = useState(false), [recallFocus, setRecallFocus] = useState(0);
  const [recallOpen, setRecallOpen] = useState(false);
  const recallTrigger = useRef<HTMLButtonElement>(null);
  const closeRecall = useCallback(() => setRecallOpen(false), []);
  const openRecall = useCallback(() => { setRecallOpen(true); setRecallFocus(v => v + 1); }, []);
  const [captureFocus, setCaptureFocus] = useState(0), [topicFocus, setTopicFocus] = useState(0);

  const [pendingReceipt, setPendingReceipt] = useState<{ key: string; receipt: Receipt } | null>(null);
  const [handoff, setHandoff] = useState<Handoff | null>(null), handoffRef = useRef<Handoff | null>(null);
  const [restoreId, setRestoreId] = useState<string | null>(null), [restoreNotice, setRestoreNotice] = useState("");
  const pageRef = useRef(page); pageRef.current = page;
  const desktop = useDesktop();
  useWindowLifecycle(setWindowError);
  const refresh = useCallback((key?: Key, receipt?: Receipt) => {
    if (key) setSelected(key);
    if (key && receipt) setPendingReceipt(receipt.action === "undo" ? null : { key: keyOf(key), receipt });
  }, []);
  const handoffReady = useCallback(() => {
    const pending = handoffRef.current;
    if (!pending) return;
    handoffRef.current = null; setHandoff(null);
    void call("desktop_handoff_ready", { generation: pending.generation }).catch(e => setWindowError(errorText(e)));
  }, []);
  useEffect(() => {
    const root = document.querySelector<HTMLElement>(".formal-app");
    if (native && root) return installClickRecovery(root);
  }, []);
  useEffect(() => {
    if (!native) return;
    void call<{ message: string; previous_backup: string | null } | null>("backup_result")
      .then(result => { if (result) setRestoreNotice(result.message + (result.previous_backup ? ` 副本：${result.previous_backup}` : "")); })
      .catch(e => setRestoreNotice(errorText(e)));
    const off = listen("restore-cancelled", () => setRestoreNotice("恢复已取消，当前记忆库保留。请核对窗口提示后重试。"));
    return () => { void off.then(f => f()); };
  }, []);
  useEffect(() => {
    if (!restoreId) return;
    setRestoreId(null); setRestoreNotice("正在保存草稿并准备重启恢复…");
    void call("backup_restore", { id: restoreId }).catch(e => setRestoreNotice(errorText(e)));
  }, [restoreId]);
  useEffect(() => {
    let alive = true;
    void call<Collection[]>("navigation_collections").then(c => { if (alive) setCollections(c); }).catch(e => { if (alive) setWindowError(errorText(e)); });
    void call<Page>("library_query", { query: { query: "", trash: false, pinned: true, limit: 100 } }).then(p => { if (alive) setPins(p.items); }).catch(e => { if (alive) setWindowError(errorText(e)); });
    return () => { alive = false; };
  }, [navigationRevision]);
  useEffect(() => {
    let alive = true;
    void call<Topic[]>("library_topics").then(t => { if (alive) setTopics(t); })
      .catch(e => { if (alive) setWindowError(errorText(e)); });
    return () => { alive = false; };
  }, [topicsRevision]);
  useEffect(() => {
    let alive = true;
    void call<Settings>("workspace_settings").then(s => { if (alive) setConfigured(s.configured); })
      .catch(e => { if (alive) setWindowError(errorText(e)); });
    return () => { alive = false; };
  }, [settingsRevision]);
  useEffect(() => {
    if (!native) return;
    const events = [
      listen<Receipt>("organization-complete", e => {
        const receipt = e.payload;
        if (receipt.status === "applied" && receipt.action === "merge" && receipt.memory_id) {
          const key: Key = {kind:"memory",id:receipt.memory_id};
          void call<Detail>("library_detail",{key}).then(d => notify(`已补充到《${d.title}》`,"查看",() => { setSelected(key); setPage("library"); setCollectionId(null); })).catch(() => {});
        }
      }),
      listen("desktop-settings", () => setSettingsOpen(true)),
      listen("workspace-close-request", () => {
        if (document.querySelector("dialog[open]")) { setWindowError("请先完成或关闭当前对话框，再关闭主窗口。"); return; }
        void finishVoiceInputs().then(() => flushDrafts()).then(() => call("workspace_close")).catch(e => setWindowError(errorText(e)));
      }),
      listen<MainRoute>("desktop-route", e => {
        void flushDrafts().then(() => refreshDrafts()).then(() => {
          if (document.querySelector("dialog[open]")) throw "请先完成或关闭主窗口中的对话框，再展开快捷窗口。";
          const r = e.payload;
          let target: Handoff["target"];

          if (r.settings) { setSettingsOpen(true); target = "view"; }
          else if (r.record) { setSelected(r.record); setPage("library"); target = "view"; }
          else if (r.mode === "ask" && r.topic) {
            setTopic(r.topic); setPage("topic"); setTopicFocus(v => v + 1); target = "topic";
          } else if (r.mode === "capture") {
            setRecallOpen(false); setCaptureQuick(r.quick); setCaptureOpen(true); setCaptureFocus(v => v + 1); target = "capture";
          } else {
            // A new question from the desktop has no collection scope. Existing
            // scoped discussions take the topic branch above.
            if (pageRef.current === "trash") setSelected(null);
            setPage("library"); setCollectionId(null);
            setRecallOpen(true); setRecallQuick(r.quick); setRecallFocus(v => v + 1); target = "query";
          }
          const pending = { generation: r.generation, target };
          handoffRef.current = pending; setHandoff(pending); refresh();
        }).catch(error => {
          setWindowError(errorText(error));
          void call("desktop_handoff_ready", { generation: e.payload.generation, failed: true });
        });
      }),
    ];
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, [refresh]);
  useEffect(() => {
    if (handoff?.target === "view") {
      const frame = requestAnimationFrame(handoffReady);
      return () => cancelAnimationFrame(frame);
    }
  }, [handoff, handoffReady]);
  const newCapture = useCallback(() => {
    // The modal must return to a visible control, not the textarea we hide.
    if (recallOpen) recallTrigger.current?.focus();
    setRecallOpen(false); setCaptureQuick(false); setCaptureOpen(true); setCaptureFocus(v => v + 1);
  }, [recallOpen]);
  useEffect(() => {
    const keyboard = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229 || e.repeat || document.querySelector("dialog[open]")) return;
      if (!e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "k") { e.preventDefault(); openRecall(); }
      if (e.key.toLowerCase() === "n") { e.preventDefault(); newCapture(); }
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [newCapture, openRecall]);
  async function ask(question: string, id: string) {
    if (!configured) { recallTrigger.current?.focus(); setRecallOpen(false); setSettingsOpen(true); throw "连接模型后，AI 就能从记忆中查找并回答。你的问题已保留。"; }
    const t = await call<Topic>("discussion_ask", { id, topicId: id, question, context: [], collectionId: queryScope });
    if (recallQuick) await desktop.update({ topic_id: t.id });
    if (pageRef.current === "trash") setSelected(null);
    setRecallOpen(false); setTopic(t); setPage("topic"); setTopicFocus(v => v + 1); refresh();
  }
  async function discuss(detail: Detail, related: Source[] = []) {
    const source: Source = detail.current ? { kind: "version", id: detail.current.id } : { kind: "capture", id: detail.key.id };
    const t = await call<Topic>("discussion_open", { id: uid(), title: detail.title, context: [source, ...related], collectionId: page === "collection" ? collectionId : null });
    setTopic(t); setPage("topic"); setTopicFocus(v => v + 1); refresh();
  }
  async function openCaptured(key: Key) {
    try {
      const jobs = await call<Array<{receipt:Receipt|null}>>("organization_jobs",{key});
      const receipt = jobs[0]?.receipt;
      if(receipt?.status === "applied" && receipt.memory_id) { openRecord({kind:"memory",id:receipt.memory_id}); return; }
    } catch { /* The saved original remains a valid fallback. */ }
    openRecord(key);
  }
  function openRecord(key: Key) { setSelected(key); setPage("library"); setCollectionId(null); }
  function showLibrary() { if (page === "trash") setSelected(null); setPage("library"); setCollectionId(null); }
  function showCollection(id: string) { setCollectionId(id); setSelected(null); setPage("collection"); }
  async function archiveCollection(value: Collection, undo = false) {
    if (collectionLock.current) return;
    collectionLock.current = true; setCollectionBusy(true); setWindowError("");
    try {
      await call("navigation_archive_collection", { id: value.id, archived: !undo, expected: value.revision });
      setArchiveConfirm(null);
      notify(undo ? "已恢复专题" : "已移除专题，记忆仍保留", undo ? undefined : "撤销", undo ? undefined : () => void archiveCollection({ ...value, revision: value.revision + 1 }, true), 8000);
      if (undo) showCollection(value.id); else { setPage("library"); setCollectionId(null); setSelected(null); }
      refresh();
    } catch (e) { setWindowError(errorText(e)); }
    finally { collectionLock.current = false; setCollectionBusy(false); }
  }
  async function openDesktop() {
    try {
      await flushDrafts();
      if (page === "topic" && topic) await desktop.update({ topic_id: topic.id });
      await call("desktop_open", { mode: page === "topic" ? "ask" : undefined });
    } catch (e) { setWindowError(errorText(e)); }
  }
  const collection = collections.find(c => c.id === collectionId);
  const queryScope = page === "collection" ? collectionId : page === "topic" ? topic?.collection_id || null : null;
  const scopeName = collections.find(c => c.id === queryScope)?.name || "已移除的专题";
  const listCollectionId = page === "collection" ? collectionId || undefined : page === "topic" ? topic?.collection_id || undefined : undefined;
  const reading = page === "topic" || !!selected;
  return <div className="app-shell formal-app recall-workspace">
    <WorkspaceQuery session={`query:${recallQuick}`} bar={{ triggerRef:recallTrigger, open:recallOpen,
      scopeLabel:queryScope ? scopeName : undefined, onOpen:openRecall, onClose:closeRecall, onCapture:newCapture,
      scope:queryScope && <div className="recall-scope"><Icon name="folder" size={12} /><span>仅在 {scopeName} 中提问</span><button aria-label="改为从全部记忆提问" onClick={showLibrary}>全部记忆</button></div>,
    }} form={{ quick:recallQuick, mode:"ask", presentation:"query", focus:recallOpen ? recallFocus : 0,
      onReady:handoff?.target === "query" ? handoffReady : undefined,
      onMode:() => {}, onEdit:() => {}, onSaved:() => {}, onAsk:ask,
    }} />
    <WorkspaceSidebar page={page} topic={topic} topics={topics} configured={configured}
      selected={selected} pins={pins} collections={collections} collectionId={collectionId}
      onReview={() => { setSelected(null); setPage("review"); setCollectionId(null); }} onPin={openRecord}
      onCollection={showCollection} onNewCollection={() => setCollectionEditor("new")}
      onLibrary={showLibrary} onTrash={() => { setSelected(null); setPage("trash"); }}
      onTopic={t => { if (page === "trash") setSelected(null); setTopic(t); setPage("topic"); setTopicFocus(v => v + 1); }}
      onDesktop={() => void openDesktop()} onSettings={() => setSettingsOpen(true)} />
    <main className="main-workspace">
      {!native && <div className="preview-banner">浏览器布局预览 · 固定示例；真实保存和编辑请使用 Mac 应用</div>}

      {page === "collection" && collection && <section className="collection-header">
        <div><span className="eyebrow">专题 · {collection.count} 条记忆</span><h1>{collection.name}</h1><p>{collection.description || "把相关的记忆汇集在这里，慢慢形成自己的思路。"}</p></div>
        <div className="toolbar-actions collection-header-actions"><button onClick={() => { if (!configured) setSettingsOpen(true); else setSuggestions(collection); }}><Icon name="spark" size={14} />AI 推荐</button>
          <button onClick={showLibrary}>去挑选记忆</button>
          <MoreMenu><button onClick={() => setCollectionEditor(collection)}>编辑专题</button><button className="danger-text" onClick={() => setArchiveConfirm(collection)}>移除专题</button></MoreMenu>
        </div>
      </section>}
      <Toast />
      {restoreNotice && <div className="restore-notice" role="status">{restoreNotice}<button aria-label="关闭恢复提示" onClick={() => setRestoreNotice("")}>×</button></div>}
      <ErrorNotice text={windowError} />
      <div className={`library-layout ${reading ? "has-selection" : ""}`}>
        <MemoryList key={page === "trash" ? "trash" : page === "review" ? "review" : listCollectionId ? `collection:${listCollectionId}` : "library"}
          review={page === "review"} collectionId={listCollectionId} trash={page === "trash"} active={page !== "topic"}
          selected={selected}  onSelect={key => { setSelected(key); if (page === "topic" && listCollectionId) { setCollectionId(listCollectionId); setPage("collection"); } else if (!["trash", "collection", "review"].includes(page)) setPage("library"); }}
          onCapture={newCapture} onRefresh={refresh} />
        {page === "topic" && topic ? <div className="workspace-answer-pane">
          <div className="answer-navigation"><button onClick={() => topic.collection_id ? showCollection(topic.collection_id) : showLibrary()}><Icon name="chevron" size={13} />返回记忆</button><span>{topic.collection_id ? `专题 · ${collections.find(c => c.id === topic.collection_id)?.name || "已移除"}` : "从记忆中查找 · 有来源的回答"}</span></div>
          <Discussion key={topic.id} topic={topic} focus={topicFocus} onReady={handoff?.target === "topic" ? handoffReady : undefined}
             configured={configured} onSettings={() => setSettingsOpen(true)} onRefresh={refresh} onOpenRecord={openRecord} />
        </div> : selected ? <MemoryDetail key={keyOf(selected)} record={selected}
          collectionId={page === "collection" ? collectionId || undefined : undefined}
          initialReceipt={pendingReceipt?.key === keyOf(selected) ? pendingReceipt.receipt : null}
           query="" onChanged={(key, receipt) => { if (key && keyOf(key) !== keyOf(selected)) { setPage("library"); setCollectionId(null); } refresh(key, receipt); }} onDiscuss={discuss} onBack={() => setSelected(null)} />
          : <section className="memory-detail-pane unselected">
            <Empty title={page === "trash" ? "留一份余地" : page === "review" ? "和过去的自己，再聊一聊" : page === "collection" ? "让相关的想法，慢慢连起来" : "让留下的想法，再次用得上"}
              text={page === "trash" ? "选一条已删除的记忆，查看内容或恢复。" : "选一条记忆慢慢读，或在上方问问过去的自己。"} />
          </section>}
      </div>
    </main>
    {captureOpen && <CaptureDialog key={`capture:${captureQuick}`} quick={captureQuick} sourceApp={desktop.state?.source_app}
      focus={captureFocus} onReady={handoff?.target === "capture" ? handoffReady : () => {}}
      onSaved={key => { notify("已记下", "查看", () => void openCaptured(key)); setCaptureOpen(false); refresh(); }} onClose={() => setCaptureOpen(false)} />}
    {collectionEditor && <CollectionEditor value={collectionEditor === "new" ? undefined : collectionEditor} onClose={() => setCollectionEditor(null)} onSaved={id => { setCollectionEditor(null); showCollection(id); refresh(); }} />}
    {suggestions && <CollectionSuggestions collection={suggestions} onClose={() => setSuggestions(null)} onChanged={refresh} />}
    {archiveConfirm && <Modal title="移除这个专题？" onClose={() => { if (!collectionLock.current) setArchiveConfirm(null); }}><p>“{archiveConfirm.name}”中的记忆和讨论都会保留。专题内的讨论仍限定原专题，不会转为全库问答。</p><ErrorNotice text={windowError} /><div className="action-row"><button className="outline-button" disabled={collectionBusy} onClick={() => setArchiveConfirm(null)}>取消</button><button className="send-button" disabled={collectionBusy} onClick={() => void archiveCollection(archiveConfirm)}>移除专题</button></div></Modal>}
    {settingsOpen && <SettingsPanel initialPage={settingsInitialPage} onClose={() => setSettingsOpen(false)} onChanged={refresh}
      onRestore={id => { setSettingsOpen(false); setRestoreId(id); }} />}
  </div>;
}
