import { translateCatalog } from "../i18n";
import { message, renderMessage, type UiMessage } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import { expireQueries, useResourceVersion } from "./resources";
import Markdown from "./Markdown";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "../ui";
import { call, fullDate, errorText, uid, native, type Message, type Source, type SourceEvidence, type Topic, type Key } from "./api";
import { ErrorNotice, Modal, MoreMenu } from "./components";
import { useDraft } from "./useDraft";
import CaptureForm, { type ComposerControls, type InputSubmission } from "./CaptureForm";
import SaveText from "./SaveText";
import MemoryChanges from "./MemoryChanges";
import IconButton from "./IconButton";
import { finishVoiceInputs } from "./useVoice";

function SourcePreview({
  source,
  messageId,
  onClose,
}: {
  source: Source;
  messageId?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation("workspace");
  const [value, setValue] = useState<SourceEvidence | null>(null),
    [error, setError] = useNotice();
  useEffect(() => {
    let alive = true;
    void call<SourceEvidence>("discussion_source", { source, messageId })
      .then((x) => {
        if (alive) setValue(x);
      })
      .catch((e) => {
        if (alive) setError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [source.id, source.kind, messageId]);
  return (
    <Modal title={t("discussion.sourceTitle")} onClose={onClose}>
      <ErrorNotice text={error} />
      {value ? (
        <>
          <h3>{value.title}</h3>
          <p className="field-help">{t("discussion.sourceMeta", { label: source.kind === "version" ? (value.current ? t("discussion.currentVersion") : t("discussion.historicalVersion")) : t("discussion.originalCapture"), date: fullDate(value.recorded_at) })}</p>
          <p className="readable-text">{value.text}</p>
          {value.additional_spans?.map(span=><div key={span.start}><p className="field-help">{t("discussion.additionalSpan")}</p><p className="readable-text">{span.text}</p></div>)}
          {value.truncated && (
            <p className="field-help">{t("discussion.excerptOnly")}</p>
          )}
        </>
      ) : (
        !error && <p>{t("discussion.loadingSource")}</p>
      )}
    </Modal>
  );
}
const failures: Record<string, UiMessage> = {
  network: message("errors", "model_network"),
  rate_limit: message("errors", "model_rate_limit"),
  invalid_answer: message("errors", "discussion_invalid_answer"),
  source_unavailable: message("errors", "discussion_source_unavailable"),
  interrupted: message("errors", "discussion_interrupted"),
  tools_unsupported: message("errors", "model_tools_unsupported"),
  model_required: message("errors", "agent_model_required"),
  model_configuration: message("errors", "agent_model_required"),
  agent_budget: message("errors", "agent_budget"),
  changes_undone: message("workspace", "input.changesUndone"),
};
const progressLabels = {
  preparing: "input.preparing", search_memories: "input.recalling", list_memories: "input.recalling",
  read_memory: "input.readingMemory", read_conversation: "input.readingDiscussion",
  write_memory: "input.updatingMemory", merge_memories: "input.updatingMemory", undo_changes: "input.undoingChanges",
  list_collections: "input.findingCollections", read_collection: "input.readingCollection",
  create_collection: "input.updatingCollections", update_collection: "input.updatingCollections", update_collection_members: "input.updatingCollections",
} as const;

export type DiscussionComposerControls = ComposerControls & {
  topicId: string;
  submit: (value: InputSubmission) => Promise<void>;
};
type Operation = "sending" | "stopping";

export default function Discussion({ topic, revision: requestedRevision = 0, configured, onSettings, onRefresh, onOpenRecord, compact = false, quick = false, sourceApp = "Memivy", composerVisible = true, onReady, onBusy, onComposerControls, focus = 1 }: {
  topic: Topic; revision?: number; configured: boolean; onSettings: () => void; onRefresh: () => void; onOpenRecord: (key: Key) => void;
  compact?: boolean; quick?: boolean; sourceApp?: string; composerVisible?: boolean; focus?: number; onReady?: () => void; onBusy?: (busy: boolean) => void;
  onComposerControls?: (value: DiscussionComposerControls | null) => void;
}) {
  const { t } = useTranslation("workspace");
  const resourceRevision = useResourceVersion([{ domain: "discussion", entity: topic.id }]) + requestedRevision;
  const [localRevision, setLocalRevision] = useState(0), [messages, setMessages] = useState<Message[]>([]), [error, setError] = useNotice();
  const [operation, setOperation] = useState<Operation | null>(null), [activeTurn, setActiveTurn] = useState<string | null>(null);
  const [ready, setReady] = useState(false), [readError, setReadError] = useNotice();
  const [more, setMore] = useState(false), [loadingMore, setLoadingMore] = useState(false);
  const [source, setSource] = useState<(Source & { messageId?: string }) | null>(null), [save, setSave] = useState<Message | null>(null);
  const draft = useDraft(`discussion:${topic.id}`, { title: "", body: "", expected_version: null, context: [], origin: { kind: "user", app: sourceApp, project: null, uri: null } });
  const bottom = useRef<HTMLDivElement>(null), messageList = useRef<HTMLDivElement>(null), historyAnchor = useRef<{ element: Element; top: number } | null>(null);
  const olderLock = useRef(false), following = useRef(true), [unreadReply, setUnreadReply] = useState(false);
  const mounted = useRef(false), activity = useRef({ ready: false, turn: null as string | null, operation: null as Operation | null, revision: 0 });
  const suggestionRequest = useRef<{ text: string; id: string } | null>(null);
  const phase = operation ?? (!ready ? "loading" : activeTurn ? "answering" : "idle");
  const controls = { topicId: topic.id, phase, beginSend, submit, stop: () => void cancel() } satisfies DiscussionComposerControls;
  const latest = messages.at(-1);
  function refresh() {
    void expireQueries(["discussion_messages"], [{ domain: "discussion", entity: topic.id }]).then(() => {
      if (mounted.current) setLocalRevision(value => value + 1);
    });
    onRefresh();
  }
  useLayoutEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; onComposerControls?.(null); };
  }, [onComposerControls]);
  useLayoutEffect(() => { onComposerControls?.(controls); }, [onComposerControls, topic.id, quick, phase]);
  useEffect(() => {
    if (!native) return;
    let active = true;
    const off = listen<{ topicId: string; inputId: string }>("discussion-updated", e => {
      if (e.payload.topicId !== topic.id) return;
      void expireQueries(["discussion_messages"], [{ domain: "discussion", entity: topic.id }]).then(() => { if (active) setLocalRevision(value => value + 1); });
    });
    return () => { active = false; void off.then(stop => stop()); };
  }, [topic.id]);
  useEffect(() => {
    let alive = true;
    const revision = activity.current.revision;
    void call<Message[]>("discussion_messages", { id: topic.id, before: null }).then(next => {
      if (!alive || revision !== activity.current.revision) return;
      const turn = next.find(m => m.role === "assistant" && m.status === "processing")?.turn_id ?? null;
      activity.current.ready = true;
      activity.current.turn = turn;
      setReady(true); setActiveTurn(turn); setReadError("");
      setMessages(old => {
        const merged = new Map(old.map(m => [m.id, m])); next.forEach(m => merged.set(m.id, m));
        return [...merged.values()].sort((a, b) => a.seq - b.seq);
      });
      setMore(old => old || next.length === 40);
    }).catch(e => { if (alive && revision === activity.current.revision) setReadError(errorText(e)); });
    return () => { alive = false; };
  }, [topic.id, resourceRevision, localRevision]);
  useLayoutEffect(() => {
    const anchor = historyAnchor.current;
    if (anchor && messageList.current) { messageList.current.scrollTop += anchor.element.getBoundingClientRect().top - anchor.top; historyAnchor.current = null; }
  }, [messages]);
  useEffect(() => {
    if (following.current) bottom.current?.scrollIntoView({ block: "nearest" }); else setUnreadReply(true);
  }, [latest?.id, latest?.status, latest?.text]);
  function acceptTurn(turn: string | null) {
    if (!mounted.current) return;
    // A read started before an acknowledgement must not restore the previous turn.
    activity.current.revision++;
    activity.current.turn = turn;
    setActiveTurn(turn);
    refresh();
  }
  function beginOperation(value: Operation) {
    if (!mounted.current || activity.current.operation) return null;
    activity.current.operation = value;
    setOperation(value); onBusy?.(true); setError("");
    return () => {
      activity.current.operation = null;
      if (mounted.current) setOperation(null);
      onBusy?.(false);
    };
  }
  function beginSend() {
    if (!activity.current.ready || activity.current.turn) return null;
    return beginOperation("sending");
  }
  async function submit(value: InputSubmission) {
    following.current = true; setUnreadReply(false);
    await call<Topic>("discussion_submit", { ...value, topicId: topic.id, quick });
    acceptTurn(value.id);
  }
  async function suggestion(text: string) {
    if (!draft.ready) return;
    const release = beginSend();
    if (!release) return;
    try {
      // A suggestion is a separate input: the user's unsent text is never consumed.
      await finishVoiceInputs();
      const value = await draft.flush();
      if (suggestionRequest.current?.text !== text) suggestionRequest.current = { text, id: uid() };
      await submit({ id: suggestionRequest.current.id, text, context: value.context || [], origin: value.origin });
      suggestionRequest.current = null;
    } catch (e) { setError(errorText(e)); }
    finally { release(); }
  }
  async function retry(inputId: string) {
    const release = beginSend();
    if (!release) return;
    try { await call("discussion_retry", { inputId }); acceptTurn(inputId); }
    catch (e) { setError(errorText(e)); }
    finally { release(); }
  }
  async function cancel() {
    const id = activity.current.turn;
    if (!id) return;
    const release = beginOperation("stopping");
    if (!release) return;
    try { await call("discussion_cancel", { id }); acceptTurn(null); }
    catch (e) { setError(errorText(e)); }
    finally { release(); }
  }
  async function older() {
    if (olderLock.current) return;
    olderLock.current = true; setLoadingMore(true);
    try {
      const next = await call<Message[]>("discussion_messages", { id: topic.id, before: messages[0]?.seq || null });
      const list = messageList.current, bounds = list?.getBoundingClientRect();
      const anchor = list && bounds && [...list.querySelectorAll(".discussion-message")].find(element => element.getBoundingClientRect().bottom > bounds.top);
      if (anchor) historyAnchor.current = { element: anchor, top: anchor.getBoundingClientRect().top };
      setMessages(old => [...next, ...old.filter(m => !next.some(n => n.id === m.id))]); setMore(next.length === 40);
    } catch (e) { setError(errorText(e)); }
    finally { olderLock.current = false; setLoadingMore(false); }
  }
  return <section className={`discussion-page ${compact ? "compact-discussion" : ""}`}>
    <div className="discussion-heading"><h1>{topic.title}</h1></div>
    <div className="discussion-timeline">
    <div className="discussion-messages" ref={messageList} onScroll={e => { const list = e.currentTarget; following.current = list.scrollHeight - list.scrollTop - list.clientHeight < 48; setUnreadReply(!following.current); }}>
      {more && <button className="outline-button" disabled={loadingMore} onClick={() => void older()}>{t("discussion.older")}</button>}
      {!ready && !readError && <div className="discussion-start" role="status">{t("discussion.loadingMessages")}</div>}
      {readError && <div className="discussion-start"><ErrorNotice text={readError} /><button className="quiet" onClick={refresh}>{t("discussion.retry")}</button></div>}
      {ready && !messages.length && <div className="discussion-start">{t("input.discussionStart")}</div>}
      {messages.map(m => <article key={m.id} className={`discussion-message ${m.role}`}>
        {m.text && (m.role === "assistant" ? <Markdown text={m.text} sources={m.citations} onSource={ref => setSource({ ...ref, messageId: m.id })} /> : <p className="readable-text">{m.text}</p>)}
        {m.status === "processing" && <p className="thinking-status" role="status">{t(progressLabels[m.progress as keyof typeof progressLabels] || "discussion.processing")}</p>}
        {["failed", "cancelled", "interrupted"].includes(m.status) && <p className="field-help" role="status">{m.status === "cancelled" && m.error_code !== "changes_undone" ? t("discussion.cancelled") : renderMessage(failures[m.error_code || ""] || (m.error_code ? errorText({ code: m.error_code }) : message("workspace", "discussion.incomplete")), translateCatalog)}</p>}
        {!!m.citations.length && <div className="discussion-citations">{m.citations.map((citation, index) => <button key={`${citation.source.kind}:${citation.source.id}`} disabled={!citation.available} onClick={() => setSource({ ...citation.source, messageId: m.id })}>{citation.available ? t("discussion.citation", { count: index + 1 }) : t("discussion.sourceDeleted")}</button>)}</div>}
        {m.role === "assistant" && <>
          <MemoryChanges inputId={m.turn_id} receipts={m.receipts} incomplete={["failed", "cancelled", "interrupted"].includes(m.status)} onOpenRecord={onOpenRecord} onRefresh={refresh} />
          {m.status === "complete" && !!m.followups.length && <div className="discussion-followups">{m.followups.map(text => <button key={text} disabled={phase !== "idle"} onClick={() => void suggestion(text)}>{text}<Icon name="arrow" size={12} /></button>)}</div>}
          <div className="discussion-message-actions">{["failed", "cancelled", "interrupted"].includes(m.status) && m.error_code !== "changes_undone" && <button className="quiet" disabled={phase !== "idle"} onClick={() => void retry(m.turn_id)}>{t("discussion.retry")}</button>}
            {m.text && m.status !== "processing" && <MoreMenu><button onClick={() => setSave(m)}>{t("input.saveText")}</button></MoreMenu>}
          </div>
        </>}
      </article>)}
      <div ref={bottom} />
    </div>
      {unreadReply && <div className="discussion-jump"><IconButton label={t("discussion.unreadReply")} icon="arrow" className="discussion-jump-button" onClick={() => { following.current = true; setUnreadReply(false); bottom.current?.scrollIntoView({ block: "nearest" }); }} /></div>}
    </div>
    <CaptureForm visible={composerVisible} draftKey={`discussion:${topic.id}`} presentation="discussion" quick={quick} sourceApp={sourceApp} focus={focus} configured={configured} onSettings={onSettings} onReady={onReady} controls={controls} onSubmit={submit} onSource={setSource} />
    <ErrorNotice text={error} />
    {source && <SourcePreview source={source} messageId={source.messageId} onClose={() => setSource(null)} />}
    {save && <SaveText message={save} topic={topic} onClose={() => setSave(null)} onSaved={() => { setSave(null); refresh(); }} />}
  </section>;
}
