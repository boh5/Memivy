import { message } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
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
import type { InputSubmission } from "./CaptureForm";
import CollectionEditor from "./CollectionEditor";
import CollectionSuggestions from "./CollectionSuggestions";
import { Modal, MoreMenu } from "./components";
import LanguageRecovery from "./LanguageRecovery";
import Toast, { notify } from "./Toast";
import { useDesktop, useWindowLifecycle, type MainRoute } from "./desktopApi";
import "../base.css";
import "./workspace.css";
import "./desktop.css";
import "./navigation.css";
import "./topbar.css";

type Handoff = { generation: number; target: "query" | "topic" | "view" };

export default function App() {
  const { t } = useTranslation("workspace");
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
  const [settingsInitialPage, setSettingsInitialPage] = useState<"general"|"ai"|"voice">("general");
  useEffect(() => { if (!settingsOpen) setSettingsInitialPage("general"); }, [settingsOpen]);
  useEffect(() => { const open = () => {setSettingsInitialPage("voice"); setSettingsOpen(true);}; window.addEventListener("voice-settings-request", open); return () => window.removeEventListener("voice-settings-request", open); }, []);
  const [windowError, setWindowError] = useNotice();
  const navigationRevision = useResourceVersion([{domain:"memory"},{domain:"navigation"},{domain:"collection"}]);
  const topicsRevision = useResourceVersion([{domain:"discussion"},{domain:"collection"}]);
  const settingsRevision = useResourceVersion([{domain:"settings"}]);
  useResourceBridge(error => setWindowError(errorText(error)));
  const [recallQuick, setRecallQuick] = useState(false), [recallFocus, setRecallFocus] = useState(0);
  const [recallOpen, setRecallOpen] = useState(false);
  const recallTrigger = useRef<HTMLButtonElement>(null);
  const closeRecall = useCallback(() => setRecallOpen(false), []);
  const openRecall = useCallback(() => { setRecallOpen(true); setRecallFocus(v => v + 1); }, []);
  const [topicFocus, setTopicFocus] = useState(0);

  const [pendingReceipt, setPendingReceipt] = useState<{ key: string; receipt: Receipt } | null>(null);
  const [handoff, setHandoff] = useState<Handoff | null>(null), handoffRef = useRef<Handoff | null>(null);
  const [restoreId, setRestoreId] = useState<string | null>(null), [restoreNotice, setRestoreNotice] = useNotice();
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
    const root = document.querySelector<HTMLElement>(".memory-app");
    if (native && root) return installClickRecovery(root);
  }, []);
  useEffect(() => {
    if (!native) return;
    void call<{ message: string; message_code?: "restore_completed" | "restore_rolled_back" | "restore_failed_preserved" | null; error_code?: string | null; previous_backup: string | null } | null>("backup_result")
      .then(result => {
        if (!result) return;
        const notice = result.message_code
          ? message("errors", result.message_code, { error: result.error_code ? errorText({ code: result.error_code }) : "" })
          : result.message;
        setRestoreNotice(result.previous_backup ? message("workspace", "app.restoreBackupWithCopy", { message: notice, path: result.previous_backup }) : notice);
      })
      .catch(e => setRestoreNotice(errorText(e)));
    const off = listen("restore-cancelled", () => setRestoreNotice(message("workspace", "app.restoreCancelled")));
    return () => { void off.then(f => f()); };
  }, []);
  useEffect(() => {
    if (!restoreId) return;
    setRestoreId(null); setRestoreNotice(message("workspace", "app.restorePreparing"));
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
    void call<Topic[]>("library_topics").then(t => {
      if (!alive) return;
      setTopics(t);
      setTopic(current => {
        const updated = t.find(item => item.id === current?.id);
        return current && updated && current.title !== updated.title ? { ...current, title: updated.title } : current;
      });
    })
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
          void call<Detail>("library_detail",{key}).then(d => notify(message("workspace", "app.mergedInto", { title: d.title }), message("workspace", "app.view"), () => openRecord(key))).catch(() => {});
        }
      }),
      listen("desktop-settings", () => setSettingsOpen(true)),
      listen("workspace-close-request", () => {
        if (document.querySelector("dialog[open]")) { setWindowError(message("workspace", "app.closeDialogBeforeWindow")); return; }
        void finishVoiceInputs().then(() => flushDrafts()).then(() => call("workspace_close")).catch(e => setWindowError(errorText(e)));
      }),
      listen<MainRoute>("desktop-route", e => {
        void flushDrafts().then(() => refreshDrafts()).then(() => {
          if (document.querySelector("dialog[open]")) throw { code: "main_dialog_active" };
          const r = e.payload;
          let target: Handoff["target"];

          if (r.settings) { setSettingsInitialPage("ai"); setSettingsOpen(true); target = "view"; }
          else if (r.record) { openRecord(r.record); target = "view"; }
          else if (r.topic) {
            setRecallQuick(r.quick); setTopic(r.topic); setPage("topic"); setTopicFocus(v => v + 1); target = "topic";
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
  const newDiscussion = useCallback(() => {
    void finishVoiceInputs().then(() => flushDrafts()).then(async () => {
      const next = await call<Topic>("discussion_open", { id: uid(), title: t("input.newDiscussion"), context: [], collectionId: page === "collection" ? collectionId : null });
      setRecallOpen(false); setRecallQuick(false); setTopic(next); setPage("topic"); setTopicFocus(value => value + 1);
    }).catch(e => setWindowError(errorText(e)));
  }, [page, collectionId, t]);
  useEffect(() => {
    const keyboard = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229 || e.repeat || document.querySelector("dialog[open]")) return;
      if (!e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "k") { e.preventDefault(); openRecall(); }
      if (e.key.toLowerCase() === "n") { e.preventDefault(); newDiscussion(); }
    };
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  }, [newDiscussion, openRecall]);
  async function submit(input: InputSubmission) {
    const currentTopic = page === "topic" ? topic?.id : undefined;
    const next = await call<Topic>("discussion_submit", { ...input, topicId: currentTopic, collectionId: queryScope, quick: recallQuick });
    if (recallQuick) await desktop.update({ topic_id: next.id });
    if (pageRef.current === "trash") setSelected(null);
    setRecallOpen(false); setTopic(next); setPage("topic"); setTopicFocus(value => value + 1); refresh();
  }
  async function discuss(detail: Detail, related: Source[] = []) {
    const source: Source = detail.current ? { kind: "version", id: detail.current.id } : { kind: "capture", id: detail.key.id };
    const t = await call<Topic>("discussion_open", { id: uid(), title: detail.title, context: [source, ...related], collectionId: page === "collection" ? collectionId : null });
    setRecallQuick(false); setTopic(t); setPage("topic"); setTopicFocus(v => v + 1); refresh();
  }
  function openRecord(key: Key) { setRecallQuick(false); setSelected(key); setPage("library"); setCollectionId(null); }
  function showLibrary() { setRecallQuick(false); if (page === "trash") setSelected(null); setPage("library"); setCollectionId(null); }
  function showCollection(id: string) { setRecallQuick(false); setCollectionId(id); setSelected(null); setPage("collection"); }
  async function archiveCollection(value: Collection, undo = false) {
    if (collectionLock.current) return;
    collectionLock.current = true; setCollectionBusy(true); setWindowError("");
    try {
      await call("navigation_archive_collection", { id: value.id, archived: !undo, expected: value.revision });
      setArchiveConfirm(null);
      notify(undo ? message("workspace", "app.restoredCollection") : message("workspace", "app.removedCollectionNotice"), undo ? undefined : message("workspace", "collection.undo"), undo ? undefined : () => void archiveCollection({ ...value, revision: value.revision + 1 }, true), 8000);
      if (undo) showCollection(value.id); else { setRecallQuick(false); setPage("library"); setCollectionId(null); setSelected(null); }
      refresh();
    } catch (e) { setWindowError(errorText(e)); }
    finally { collectionLock.current = false; setCollectionBusy(false); }
  }
  async function openDesktop() {
    try {
      await flushDrafts();
      if (page === "topic" && topic) await desktop.update({ topic_id: topic.id });
      await call("desktop_open");
    } catch (e) { setWindowError(errorText(e)); }
  }
  const collection = collections.find(c => c.id === collectionId);
  const queryScope = page === "collection" ? collectionId : page === "topic" ? topic?.collection_id || null : null;
  const scopeName = collections.find(c => c.id === queryScope)?.name || t("app.removedCollection");
  const listCollectionId = page === "collection" ? collectionId || undefined : page === "topic" ? topic?.collection_id || undefined : undefined;
  const reading = page === "topic" || !!selected;
  return <div className="app-shell memory-app recall-workspace">
    <WorkspaceQuery session={`query:${recallQuick}`} bar={{ triggerRef:recallTrigger, open:recallOpen,
      scopeLabel:queryScope ? scopeName : undefined, onOpen:openRecall, onClose:closeRecall, onNewDiscussion:newDiscussion,
      scope:queryScope && <div className="recall-scope"><Icon name="folder" size={12} /><span>{t("app.scopedQuestion", { scope: scopeName })}</span><button aria-label={t("app.allMemoriesAria")} onClick={showLibrary}>{t("app.allMemories")}</button></div>,
    }} form={{ quick:recallQuick, sourceApp:recallQuick ? desktop.state?.source_app : "Memivy", draftKey:page === "topic" && topic ? `discussion:${topic.id}` : undefined, presentation:"query", focus:recallOpen ? recallFocus : 0,
      onReady:handoff?.target === "query" ? handoffReady : undefined,
      configured, onSettings:() => {setSettingsInitialPage("ai"); setSettingsOpen(true);}, onSubmit:submit,
    }} />
    <WorkspaceSidebar page={page} topic={topic} topics={topics}
      selected={selected} pins={pins} collections={collections} collectionId={collectionId}
      onReview={() => { setRecallQuick(false); setSelected(null); setPage("review"); setCollectionId(null); }} onPin={openRecord}
      onCollection={showCollection} onNewCollection={() => setCollectionEditor("new")}
      onLibrary={showLibrary} onTrash={() => { setRecallQuick(false); setSelected(null); setPage("trash"); }}
      onTopic={t => { setRecallQuick(false); if (page === "trash") setSelected(null); setTopic(t); setPage("topic"); setTopicFocus(v => v + 1); }}
      onDesktop={() => void openDesktop()} onSettings={() => setSettingsOpen(true)} />
    <main className="main-workspace">
      {!native && <div className="preview-banner">{t("app.previewBanner")}</div>}
      <LanguageRecovery />

      {page === "collection" && collection && <section className="collection-header">
        <div><span className="eyebrow">{t("app.collectionCount", { count: collection.count })}</span><h1>{collection.name}</h1>{collection.description && <p>{collection.description}</p>}</div>
        <div className="toolbar-actions collection-header-actions"><button onClick={() => { if (!configured) {setSettingsInitialPage("ai"); setSettingsOpen(true);} else setSuggestions(collection); }}><Icon name="spark" size={14} />{t("app.recommend")}</button>
          <button onClick={showLibrary}>{t("app.chooseMemories")}</button>
          <MoreMenu><button onClick={() => setCollectionEditor(collection)}>{t("app.editCollection")}</button><button className="danger-text" onClick={() => setArchiveConfirm(collection)}>{t("app.removeCollection")}</button></MoreMenu>
        </div>
      </section>}
      <Toast />
      {restoreNotice && <div className="restore-notice" role="status">{restoreNotice}<button aria-label={t("app.closeRestoreNotice")} onClick={() => setRestoreNotice("")}>×</button></div>}
      <ErrorNotice text={windowError} />
      <div className={`library-layout ${reading ? "has-selection" : ""}`}>
        <MemoryList key={page === "trash" ? "trash" : page === "review" ? "review" : listCollectionId ? `collection:${listCollectionId}` : "library"}
          review={page === "review"} collectionId={listCollectionId} trash={page === "trash"} active={page !== "topic"}
          selected={selected}  onSelect={key => { setRecallQuick(false); setSelected(key); if (page === "topic" && listCollectionId) { setCollectionId(listCollectionId); setPage("collection"); } else if (!["trash", "collection", "review"].includes(page)) setPage("library"); }}
          onCapture={newDiscussion} onRefresh={refresh} />
        {page === "topic" && topic ? <div className="workspace-answer-pane">
          <div className="answer-navigation"><button onClick={() => topic.collection_id ? showCollection(topic.collection_id) : showLibrary()}><Icon name="chevron" size={13} />{t("app.backToMemories")}</button><span>{topic.collection_id ? t("app.topicScoped", { name: collections.find(c => c.id === topic.collection_id)?.name || t("app.removedCollection") }) : null}</span></div>
          <Discussion composerVisible={!recallOpen} key={topic.id} topic={topic} quick={recallQuick} sourceApp={recallQuick ? desktop.state?.source_app : "Memivy"} focus={topicFocus} onReady={handoff?.target === "topic" ? handoffReady : undefined}
             configured={configured} onSettings={() => {setSettingsInitialPage("ai"); setSettingsOpen(true);}} onRefresh={refresh} onOpenRecord={openRecord} />
        </div> : selected ? <MemoryDetail key={keyOf(selected)} record={selected}
          collectionId={page === "collection" ? collectionId || undefined : undefined}
          initialReceipt={pendingReceipt?.key === keyOf(selected) ? pendingReceipt.receipt : null}
           query="" onChanged={(key, receipt) => { if (key && keyOf(key) !== keyOf(selected)) { setPage("library"); setCollectionId(null); } refresh(key, receipt); }} onDiscuss={discuss} onOpenDiscussion={async id => {
             try { const next = await call<Topic>("discussion_topic", { id }); setRecallQuick(false); setTopic(next); setPage("topic"); setTopicFocus(value => value + 1); }
             catch (e) { setWindowError(errorText(e)); }
           }} onBack={() => setSelected(null)} />
          : <section className="memory-detail-pane unselected">
            <Empty title={page === "trash" ? t("app.emptyTrashTitle") : page === "review" ? t("app.emptyReviewTitle") : page === "collection" ? t("app.emptyCollectionTitle") : t("app.emptyLibraryTitle")}
              text={page === "trash" ? t("app.emptyTrashText") : t("app.emptyDefaultText")} />
          </section>}
      </div>
    </main>
    {settingsOpen && <SettingsPanel initialPage={settingsInitialPage} onClose={() => setSettingsOpen(false)} onChanged={refresh}
      onRestore={id => { setSettingsOpen(false); setRestoreId(id); }} />}
    {collectionEditor && <CollectionEditor value={collectionEditor === "new" ? undefined : collectionEditor}
      onSaved={id => { setCollectionEditor(null); showCollection(id); refresh(); }} onClose={() => setCollectionEditor(null)} />}
    {suggestions && <CollectionSuggestions collection={suggestions} onChanged={refresh} onClose={() => setSuggestions(null)} />}
    {archiveConfirm && <Modal title={t("app.removeCollectionTitle")} onClose={() => { if (!collectionBusy) setArchiveConfirm(null); }}>
      <p>{t("app.removeCollectionBody", { name: archiveConfirm.name })}</p>
      <ErrorNotice text={windowError} />
      <div className="action-row"><button className="outline-button" disabled={collectionBusy} onClick={() => setArchiveConfirm(null)}>{t("collection.cancel")}</button>
        <button className="send-button" disabled={collectionBusy} onClick={() => void archiveCollection(archiveConfirm)}>{t("app.removeCollection")}</button></div>
    </Modal>}
  </div>;
}
