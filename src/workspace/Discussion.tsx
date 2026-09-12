import { translateCatalog } from "../i18n";
import { message, renderMessage, type UiMessage } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import { useResourceVersion } from "./resources";
import Markdown from "./Markdown";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import {
  call,
  fullDate,
  errorText,
  uid,
  type Message,
  type Receipt,
  type Source,
  type SourceEvidence,
  type Topic,
  type Key,
} from "./api";
import { ErrorNotice, Modal } from "./components";
import { useDraft } from "./useDraft";
import { isSubmitKey } from "./keyboard";
import DraftConflict from "./DraftConflict";
import SaveConclusion from "./SaveConclusion";

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
};
export default function Discussion({
  topic,
  revision: requestedRevision = 0,
  configured,
  onSettings,
  onRefresh,
  onOpenRecord, compact = false, onReady, onBusy, focus = 0,
}: {
  topic: Topic;
  revision?: number;
  configured: boolean;
  onSettings: () => void;
  onRefresh: () => void;
  onOpenRecord: (key: Key) => void;
  compact?: boolean;
  focus?: number;
  onReady?: () => void;
  onBusy?: (busy: boolean) => void;
}) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{domain:"discussion",entity:topic.id}]) + requestedRevision;
  const draft = useDraft(`discussion:${topic.id}`, {
    title: "",
    body: "",
    expected_version: null,
    context: [],
  });
  const [messages, setMessages] = useState<Message[]>([]),
    [error, setError] = useNotice(),
    [sending, setSending] = useState(false),
    [more, setMore] = useState(false),
    [loadingMore, setLoadingMore] = useState(false),
    [source, setSource] = useState<(Source & { messageId?: string }) | null>(null),
    [review, setReview] = useState<Message | null>(null),
    [receipt, setReceipt] = useState<Receipt | null>(null),
    [savedNotice, setSavedNotice] = useNotice();
  const input = useRef<HTMLTextAreaElement>(null),
    bottom = useRef<HTMLDivElement>(null),
    messageList = useRef<HTMLDivElement>(null),
    historyAnchor = useRef<{ element: Element; top: number } | null>(null),
    olderLock = useRef(false),
    lock = useRef(false),
    composing = useRef(false),
    undoRequest = useRef(uid());
  const pending = messages.find(
    (m) => m.role === "assistant" && m.status === "processing",
  );
  useEffect(() => {
    let alive = true;
    void call<Message[]>("discussion_messages", { id: topic.id, before: null })
      .then((next) => {
        if (!alive) return;
        setMessages((old) => {
          const merged = new Map(old.map((m) => [m.id, m]));
          next.forEach((m) => merged.set(m.id, m));
          return [...merged.values()].sort((a, b) => a.seq - b.seq);
        });
        setMore((old) => old || next.length === 40);
      })
      .catch((e) => {
        if (alive) setError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [topic.id, revision]);
  useEffect(() => {
    if (draft.ready && !sending) { input.current?.focus(); onReady?.(); }
  }, [draft.ready, focus, sending]);
  const following = useRef(true);
  const [unreadReply, setUnreadReply] = useState(false);
  const latest = messages.at(-1);
  useLayoutEffect(() => {
    const anchor = historyAnchor.current;
    if (anchor && messageList.current) {
      messageList.current.scrollTop +=
        anchor.element.getBoundingClientRect().top - anchor.top;
      historyAnchor.current = null;
    }
  }, [messages]);
  useEffect(() => {
    if (following.current) bottom.current?.scrollIntoView({ block: "nearest" });
    else setUnreadReply(true);
  }, [latest?.id, latest?.status]);
  async function send() {
    if (lock.current || pending || !draft.ready || !draft.value.body.trim())
      return;
    if (!configured) {
      setError(message("workspace", "discussion.modelRequired"));
      onSettings();
      return;
    }
    lock.current = true;
    following.current = true; setUnreadReply(false);
    setSending(true);
    onBusy?.(true);
    setError("");
    try {
      const value = await draft.flush();
      await call("discussion_ask", {
        id: value.request_id,
        topicId: topic.id,
        question: value.body,
        context: value.context || [],
      });
      await draft.clear(value.request_id, true);
      onRefresh();
      input.current?.focus();
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
      setSending(false);
      onBusy?.(false);
    }
  }
  async function cancel() {
    if (!pending) return;
    try {
      await call("discussion_cancel", { id: pending.turn_id });
      onRefresh();
    } catch (e) {
      setError(errorText(e));
    }
  }
  async function older() {
    if (olderLock.current) return;
    olderLock.current = true;
    setLoadingMore(true);
    try {
      const next = await call<Message[]>("discussion_messages", {
        id: topic.id,
        before: messages[0]?.seq || null,
      });
      const list = messageList.current;
      const bounds = list?.getBoundingClientRect();
      const anchor = list && bounds &&
        [...list.querySelectorAll(".discussion-message")].find(
          (element) => element.getBoundingClientRect().bottom > bounds.top,
        );
      if (anchor)
        historyAnchor.current = {
          element: anchor,
          top: anchor.getBoundingClientRect().top,
        };
      setMessages((old) => [
        ...next,
        ...old.filter((m) => !next.some((n) => n.id === m.id)),
      ]);
      setMore(next.length === 40);
    } catch (e) {
      setError(errorText(e));
    } finally {
      olderLock.current = false;
      setLoadingMore(false);
    }
  }
  async function undo() {
    if (!receipt || lock.current) return;
    lock.current = true;
    try {
      await call("library_action", {
        action: {
          action: "undo",
          request_id: undoRequest.current,
          original_request: receipt.request_id,
        },
      });
      setReceipt(null);
      setSavedNotice(message("workspace", "discussion.undoNotice"));
      onRefresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
    }
  }
  return (
    <section className={`discussion-page ${compact ? "compact-discussion" : ""}`}>
      <div className="discussion-heading">
        <span className="eyebrow">{t("discussion.eyebrow")}</span>
        <h1>{topic.title}</h1>
        <p>{t("discussion.description")}</p>
      </div>
      {unreadReply && <button className="quiet" onClick={() => { following.current=true; setUnreadReply(false); bottom.current?.scrollIntoView({block:"nearest"}); }}>{t("discussion.unreadReply")}</button>}
      <div className="discussion-messages" ref={messageList} onScroll={event => {
        const list = event.currentTarget;
        following.current = list.scrollHeight - list.scrollTop - list.clientHeight < 48;
        if (following.current) setUnreadReply(false);
      }}>
        {more && (
          <button
            className="outline-button"
            disabled={loadingMore}
            onClick={() => void older()}
          >
            {t("discussion.older")}
          </button>
        )}
        {!messages.length && (
          <div className="discussion-start">
            {draft.value.context?.length
              ? t("discussion.startWithContext")
              : t("discussion.startWithoutContext")}
          </div>
        )}
        {messages.map((m) => (
          <article
            key={m.id}
            className={`discussion-message ${m.role}`}
          >
            <strong>{m.role === "user" ? t("discussion.userLabel") : "Memivy"}</strong>
            {m.status === "processing" ? (
              <p className="thinking-status" role="status">
                {t("discussion.processing")}
              </p>
            ) : m.answer ? (
              <div className="answer-content">
                {!m.answer.recollections.length && <p className="field-help">{t("discussion.noEvidence")}</p>}
                {m.answer.recollections.map((claim, index) => <div key={index}><Markdown text={claim.text} /><div className="discussion-citations">{claim.sources.map(ref => {
                  const citation = m.citations.find(c => c.source.kind === ref.kind && c.source.id === ref.id);
                  return <button key={`${ref.kind}:${ref.id}`} disabled={!citation?.available} onClick={() => setSource({ ...ref, messageId: m.id })}>{citation?.available ? t("discussion.viewCitation") : t("discussion.sourceDeleted")}</button>;
                })}</div></div>)}
                {m.answer.ideas && <div><strong className="answer-label">{t("discussion.answerIdeas")}</strong><Markdown text={m.answer.ideas} /></div>}
                {m.answer.conclusion && <div><strong className="answer-label">{t("discussion.answerConclusion")}</strong><Markdown text={m.answer.conclusion} /></div>}
              </div>
            ) : (
              <p className="readable-text">
                {m.text ||
                  (m.status === "cancelled"
                    ? t("discussion.cancelled")
                    : renderMessage(failures[m.error_code || ""] || message("workspace", "discussion.incomplete"), translateCatalog))}
              </p>
            )}
            {!m.answer && !!m.citations.length && (
              <div className="discussion-citations">
                {m.citations.map((c, i) => (
                  <button
                    key={`${c.source.kind}:${c.source.id}`}
                    disabled={!c.available}
                    onClick={() => setSource({ ...c.source, messageId: m.id })}
                  >
                    {c.available ? t("discussion.citation", { count: i + 1 }) : t("discussion.sourceDeleted")}
                  </button>
                ))}
              </div>
            )}
            {m.role === "assistant" && m.status === "complete" && (
              <button
                className="save-discussion-link"
                onClick={() => setReview(m)}
              >
                {t("discussion.leaveConclusion")} <Icon name="plus" size={12} />
              </button>
            )}
            {m.role === "assistant" &&
              ["failed", "cancelled", "interrupted"].includes(m.status) && (
                <button
                  className="save-discussion-link"
                  disabled={!!pending || sending}
                  onClick={() => {
                    const question = messages.find(
                      (x) => x.turn_id === m.turn_id && x.role === "user",
                    );
                    if (question) {
                      draft.update({ body: question.text });
                      input.current?.focus();
                    }
                  }}
                >
                  {t("discussion.retry")}
                </button>
              )}
          </article>
        ))}
        {savedNotice && (
          <div className="mutation-receipt" role="status">
            <span>{savedNotice}</span>
            {receipt && (
              <>
                <button
                  onClick={() =>
                    onOpenRecord({
                      kind: receipt.memory_id ? "memory" : "capture",
                      id: receipt.memory_id || receipt.capture_id!,
                    })
                  }
                >
                  {receipt.status === "needs_review" ? t("discussion.retainedDraft") : t("discussion.viewMemory")}
                </button>
                {receipt.status === "applied" && <button onClick={() => void undo()}>{t("discussion.undo")}</button>}
              </>
            )}
          </div>
        )}
        <div ref={bottom} />
      </div>
      <div className="discussion-composer">
        {!!draft.value.context?.length && (
          <div className="discussion-context">
            {draft.value.context.map((s, i) => (
              <span key={`${s.kind}:${s.id}`}>
                <button onClick={() => setSource(s)}>
                  <Icon name="book" size={13} />
                  {t("discussion.selectedMemory", { suffix: draft.value.context!.length > 1 ? ` ${i + 1}` : "" })}
                </button>
                <button
                  aria-label={t("discussion.removeEvidence")}
                  disabled={!!pending || sending}
                  onClick={() =>
                    draft.update({
                      context: draft.value.context!.filter((x) => x !== s),
                    })
                  }
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        )}
        <textarea
          ref={input}
          aria-label={t("discussion.composerAria")}
          placeholder={t("discussion.composerPlaceholder")}
          value={draft.value.body}
          disabled={!draft.ready || sending}
          onChange={(e) => draft.update({ body: e.target.value })}
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onKeyDown={(e) => {
            if (isSubmitKey({ ...e, isComposing: e.nativeEvent.isComposing }, composing.current)) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div className="composer-bottom">
          <span>
            {t("discussion.composerHelp")}
          </span>
          {pending ? (
            <button className="outline-button" onClick={() => void cancel()}>
              <Icon name="stop" size={13} />
              {t("discussion.stop")}
            </button>
          ) : (
            <button
              className="send-button"
              disabled={!draft.ready || sending || !draft.value.body.trim()}
              onClick={() => void send()}
            >
              {sending ? t("discussion.sending") : t("discussion.send")}
              <Icon name="arrow" size={16} />
            </button>
          )}
        </div>
        {!configured && (
          <button className="connect-model-link" onClick={onSettings}>
            {t("discussion.connectModel")}
          </button>
        )}
        <ErrorNotice text={error || draft.error} />
        <DraftConflict draft={draft} />
      </div>
      {source && (
        <SourcePreview source={source} messageId={source.messageId} onClose={() => setSource(null)} />
      )}{" "}
      {review && (
        <SaveConclusion
          message={review}
          context={draft.value.context || []}
          topic={topic}
          onClose={() => setReview(null)}
          onSaved={(r) => {
            setReceipt(r);
            setSavedNotice(r.status === "needs_review" ? message("workspace", "discussion.savedNeedsReview") : r.before_version ? message("workspace", "discussion.savedExisting") : message("workspace", "discussion.savedNew"));
            undoRequest.current = uid();
            setReview(null);
            onRefresh();
          }}
        />
      )}
    </section>
  );
}
