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
import CaptureForm, { type InputSubmission } from "./CaptureForm";
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
  recalling: "input.recalling", search_memories: "input.recalling", list_memories: "input.recalling",
  read_memory: "input.readingMemory", read_conversation: "input.readingDiscussion",
  write_memory: "input.updatingMemory", undo_changes: "input.undoingMemory", set_turn_options: "input.understanding",
} as const;

export default function Discussion({ topic, revision: requestedRevision = 0, configured, onSettings, onRefresh, onOpenRecord, compact = false, quick = false, sourceApp = "Memivy", composerVisible = true, onReady, onBusy, focus = 1 }: {
  topic: Topic; revision?: number; configured: boolean; onSettings: () => void; onRefresh: () => void; onOpenRecord: (key: Key) => void;
  compact?: boolean; quick?: boolean; sourceApp?: string; composerVisible?: boolean; focus?: number; onReady?: () => void; onBusy?: (busy: boolean) => void;
}) {
  const { t } = useTranslation("workspace");
  const resourceRevision = useResourceVersion([{ domain: "discussion", entity: topic.id }]) + requestedRevision;
  const [localRevision, setLocalRevision] = useState(0), [messages, setMessages] = useState<Message[]>([]), [error, setError] = useNotice();
  const [sending, setSending] = useState(false), [more, setMore] = useState(false), [loadingMore, setLoadingMore] = useState(false);
  const [source, setSource] = useState<(Source & { messageId?: string }) | null>(null), [save, setSave] = useState<Message | null>(null);
  const draft = useDraft(`discussion:${topic.id}`, { title: "", body: "", expected_version: null, context: [], origin: { kind: "user", app: sourceApp, project: null, uri: null } });
  const bottom = useRef<HTMLDivElement>(null), messageList = useRef<HTMLDivElement>(null), historyAnchor = useRef<{ element: Element; top: number } | null>(null);
  const olderLock = useRef(false), lock = useRef(false), following = useRef(true), [unreadReply, setUnreadReply] = useState(false);
  const suggestionRequest = useRef<{ text: string; id: string } | null>(null);
  const pending = messages.find(m => m.role === "assistant" && m.status === "processing");
  const latest = messages.at(-1);
  function refresh() { setLocalRevision(value => value + 1); onRefresh(); }
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
    void call<Message[]>("discussion_messages", { id: topic.id, before: null }).then(next => {
      if (!alive) return;
      setMessages(old => {
        const merged = new Map(old.map(m => [m.id, m])); next.forEach(m => merged.set(m.id, m));
        return [...merged.values()].sort((a, b) => a.seq - b.seq);
      });
      setMore(old => old || next.length === 40);
    }).catch(e => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [topic.id, resourceRevision, localRevision]);
  useLayoutEffect(() => {
    const anchor = historyAnchor.current;
    if (anchor && messageList.current) { messageList.current.scrollTop += anchor.element.getBoundingClientRect().top - anchor.top; historyAnchor.current = null; }
  }, [messages]);
  useEffect(() => {
    if (following.current) bottom.current?.scrollIntoView({ block: "nearest" }); else setUnreadReply(true);
  }, [latest?.id, latest?.status, latest?.text]);
  async function submit(value: InputSubmission) {
    following.current = true; setUnreadReply(false);
    await call<Topic>("discussion_submit", { ...value, topicId: topic.id, quick });
    refresh();
  }
  async function suggestion(text: string) {
    if (lock.current || pending || !draft.ready) return;
    lock.current = true; setSending(true); onBusy?.(true); setError("");
    try {
      // A suggestion is a separate input: the user's unsent text is never consumed.
      await finishVoiceInputs();
      const value = await draft.flush();
      if (suggestionRequest.current?.text !== text) suggestionRequest.current = { text, id: uid() };
      await submit({ id: suggestionRequest.current.id, text, context: value.context || [], origin: value.origin });
      suggestionRequest.current = null;
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setSending(false); onBusy?.(false); }
  }
  async function retry(inputId: string) {
    if (lock.current || pending) return;
    lock.current = true; setSending(true); setError("");
    try { await call("discussion_retry", { inputId }); refresh(); }
    catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setSending(false); }
  }
  async function cancel() {
    if (!pending) return;
    try { await call("discussion_cancel", { id: pending.turn_id }); refresh(); }
    catch (e) { setError(errorText(e)); }
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
    <div className="discussion-heading"><span className="eyebrow">{t("discussion.eyebrow")}</span><h1>{topic.title}</h1><p>{t("input.discussionHelp")}</p></div>
    <div className="discussion-timeline">
    <div className="discussion-messages" ref={messageList} onScroll={e => { const list = e.currentTarget; following.current = list.scrollHeight - list.scrollTop - list.clientHeight < 48; setUnreadReply(!following.current); }}>
      {more && <button className="outline-button" disabled={loadingMore} onClick={() => void older()}>{t("discussion.older")}</button>}
      {!messages.length && <div className="discussion-start">{t("input.discussionStart")}</div>}
      {messages.map(m => <article key={m.id} className={`discussion-message ${m.role}`}>
        {m.text && (m.role === "assistant" ? <Markdown text={m.text} sources={m.citations} onSource={ref => setSource({ ...ref, messageId: m.id })} /> : <p className="readable-text">{m.text}</p>)}
        {m.status === "processing" && <p className="thinking-status" role="status">{t(progressLabels[m.progress as keyof typeof progressLabels] || "discussion.processing")}</p>}
        {["failed", "cancelled", "interrupted"].includes(m.status) && <p className="field-help" role="status">{m.status === "cancelled" && m.error_code !== "changes_undone" ? t("discussion.cancelled") : renderMessage(failures[m.error_code || ""] || (m.error_code ? errorText({ code: m.error_code }) : message("workspace", "discussion.incomplete")), translateCatalog)}</p>}
        {!!m.citations.length && <div className="discussion-citations">{m.citations.map((citation, index) => <button key={`${citation.source.kind}:${citation.source.id}`} disabled={!citation.available} onClick={() => setSource({ ...citation.source, messageId: m.id })}>{citation.available ? t("discussion.citation", { count: index + 1 }) : t("discussion.sourceDeleted")}</button>)}</div>}
        {m.role === "assistant" && <>
          <MemoryChanges inputId={m.turn_id} receipts={m.receipts} onOpenRecord={onOpenRecord} onRefresh={refresh} />
          {m.status === "complete" && !m.record_only && !!m.followups.length && <div className="discussion-followups">{m.followups.map(text => <button key={text} disabled={!!pending || sending} onClick={() => void suggestion(text)}>{text}<Icon name="arrow" size={12} /></button>)}</div>}
          <div className="discussion-message-actions">{["failed", "cancelled", "interrupted"].includes(m.status) && m.error_code !== "changes_undone" && <button className="quiet" disabled={!!pending || sending} onClick={() => void retry(m.turn_id)}>{t("discussion.retry")}</button>}
            {m.text && m.status !== "processing" && <MoreMenu><button onClick={() => setSave(m)}>{t("input.saveText")}</button></MoreMenu>}
          </div>
        </>}
      </article>)}
      <div ref={bottom} />
    </div>
      {unreadReply && <div className="discussion-jump"><IconButton label={t("discussion.unreadReply")} icon="arrow" className="discussion-jump-button" onClick={() => { following.current = true; setUnreadReply(false); bottom.current?.scrollIntoView({ block: "nearest" }); }} /></div>}
    </div>
    <CaptureForm visible={composerVisible} draftKey={`discussion:${topic.id}`} presentation="discussion" quick={quick} sourceApp={sourceApp} focus={focus} configured={configured} onSettings={onSettings} onReady={onReady} onBusy={onBusy} pending={!!pending || sending} onCancel={() => void cancel()} onSubmit={submit} onSource={setSource} />
    <ErrorNotice text={error} />
    {source && <SourcePreview source={source} messageId={source.messageId} onClose={() => setSource(null)} />}
    {save && <SaveText message={save} topic={topic} onClose={() => setSave(null)} onSaved={() => { setSave(null); refresh(); }} />}
  </section>;
}
