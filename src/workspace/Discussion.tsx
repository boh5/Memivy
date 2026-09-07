import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import {
  call,
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

function SourcePreview({
  source,
  onClose,
}: {
  source: Source;
  onClose: () => void;
}) {
  const [value, setValue] = useState<SourceEvidence | null>(null),
    [error, setError] = useState("");
  useEffect(() => {
    let alive = true;
    void call<SourceEvidence>("discussion_source", { source })
      .then((x) => {
        if (alive) setValue(x);
      })
      .catch((e) => {
        if (alive) setError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [source.id, source.kind]);
  return (
    <Modal title="讨论依据" onClose={onClose}>
      <ErrorNotice text={error} />
      {value ? (
        <>
          <h3>{value.title}</h3>
          <p className="field-help">
            {source.kind === "version"
              ? "本次讨论使用的记忆版本"
              : "本次讨论使用的原话"}
          </p>
          <p className="readable-text">{value.text}</p>
          {value.truncated && (
            <p className="field-help">这里只展示来源节选。</p>
          )}
        </>
      ) : (
        !error && <p>正在读取来源…</p>
      )}
    </Modal>
  );
}
function SaveConclusion({
  message,
  topic,
  onClose,
  onSaved,
}: {
  message: Message;
  topic: Topic;
  onClose: () => void;
  onSaved: (r: Receipt) => void;
}) {
  const [title, setTitle] = useState(topic.title),
    [text, setText] = useState(
      message.text.split("可以留下的结论：\n").at(-1) || message.text,
    ),
    [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const request = useRef(uid()),
    lock = useRef(false);
  async function save() {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const r = await call<Receipt>("discussion_save", {
        request: {
          request_id: request.current,
          message_id: message.id,
          destination: { kind: "new" },
          title,
          text,
        },
      });
      onSaved(r);
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <Modal
      title="留下这段结论"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <p className="field-help">
        核对文字后，保存为一条新记忆。讨论本身不会自动进入记忆库。
      </p>
      <label className="discussion-field">
        标题
        <input
          value={title}
          disabled={busy}
          onChange={(e) => {
            setTitle(e.target.value);
            request.current = uid();
          }}
        />
      </label>
      <label className="discussion-field">
        要保存的文字
        <textarea
          value={text}
          disabled={busy}
          rows={7}
          onChange={(e) => {
            setText(e.target.value);
            request.current = uid();
          }}
        />
      </label>
      <ErrorNotice text={error} />
      <div className="action-row">
        <button
          className="send-button"
          disabled={busy || !title.trim() || !text.trim()}
          onClick={() => void save()}
        >
          {busy ? "保存中…" : "确认保存新记忆"}
        </button>
        <button className="outline-button" disabled={busy} onClick={onClose}>
          取消
        </button>
      </div>
    </Modal>
  );
}
const failures: Record<string, string> = {
  network: "模型请求未完成，请检查连接后重试。",
  invalid_answer: "这次回答缺少有效依据或格式不完整，请重试。",
  source_unavailable: "本次使用的来源已不可用，请重新选择记忆后提问。",
  interrupted: "上次退出时回答尚未完成，可以继续提问。",
};
export default function Discussion({
  topic,
  revision,
  configured,
  onSettings,
  onRefresh,
  onOpenRecord,
}: {
  topic: Topic;
  revision: number;
  configured: boolean;
  onSettings: () => void;
  onRefresh: () => void;
  onOpenRecord: (key: Key) => void;
}) {
  const draft = useDraft(`discussion:${topic.id}`, {
    title: "",
    body: "",
    expected_version: null,
    context: [],
  });
  const [messages, setMessages] = useState<Message[]>([]),
    [error, setError] = useState(""),
    [sending, setSending] = useState(false),
    [more, setMore] = useState(false),
    [loadingMore, setLoadingMore] = useState(false),
    [source, setSource] = useState<Source | null>(null),
    [review, setReview] = useState<Message | null>(null),
    [receipt, setReceipt] = useState<Receipt | null>(null),
    [savedNotice, setSavedNotice] = useState("");
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
    if (draft.ready) input.current?.focus();
  }, [draft.ready]);
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
    bottom.current?.scrollIntoView({ block: "nearest" });
  }, [latest?.id, latest?.status]);
  async function send() {
    if (lock.current || pending || !draft.ready || !draft.value.body.trim())
      return;
    if (!configured) {
      setError("先连接一个模型，问题草稿会保留在这里。");
      onSettings();
      return;
    }
    lock.current = true;
    setSending(true);
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
      setSavedNotice("已撤销本次整理，确认的原话仍保留。");
      onRefresh();
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
    }
  }
  return (
    <section className="discussion-page">
      <div className="discussion-heading">
        <span className="eyebrow">接着想</span>
        <h1>{topic.title}</h1>
        <p>结合自己的记忆，继续想一想。</p>
      </div>
      <div className="discussion-messages" ref={messageList}>
        {more && (
          <button
            className="outline-button"
            disabled={loadingMore}
            onClick={() => void older()}
          >
            查看更早的讨论
          </button>
        )}
        {!messages.length && (
          <div className="discussion-start">
            {draft.value.context?.length
              ? "这条记忆已放在这里。你想从哪里接着想？"
              : "从一个问题开始，也可以聊聊新的想法。"}
          </div>
        )}
        {messages.map((m) => (
          <article
            key={m.id}
            className={`discussion-message ${m.role}`}
          >
            <strong>{m.role === "user" ? "我" : "Memivy"}</strong>
            {m.status === "processing" ? (
              <p className="thinking-status" role="status">
                正在查找记忆、组织回答…
              </p>
            ) : (
              <p className="readable-text">
                {m.text ||
                  (m.status === "cancelled"
                    ? "已停止，问题仍保留。"
                    : failures[m.error_code || ""] || "这次回答未完成。")}
              </p>
            )}
            {!!m.citations.length && (
              <div className="discussion-citations">
                {m.citations.map((c, i) => (
                  <button
                    key={`${c.source.kind}:${c.source.id}`}
                    disabled={!c.available}
                    onClick={() => setSource(c.source)}
                  >
                    {c.available ? `依据 ${i + 1}` : "来源已删除"}
                  </button>
                ))}
              </div>
            )}
            {m.role === "assistant" && m.status === "complete" && (
              <button
                className="save-discussion-link"
                onClick={() => setReview(m)}
              >
                留下这段结论 <Icon name="plus" size={12} />
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
                  重新提问
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
                  查看记忆
                </button>
                <button onClick={() => void undo()}>撤销</button>
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
                  已选记忆 {draft.value.context!.length > 1 ? i + 1 : ""}
                </button>
                <button
                  aria-label="移除这条讨论依据"
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
          aria-label="继续讨论"
          placeholder="从这条思路，继续问下去…"
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
            if (
              e.key === "Enter" &&
              !e.shiftKey &&
              !e.nativeEvent.isComposing &&
              !composing.current &&
              e.keyCode !== 229
            ) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <div className="composer-bottom">
          <span>
            {draft.saved ? "讨论留在本机 · 确认后才存为记忆" : "保存草稿中…"}
          </span>
          {pending ? (
            <button className="outline-button" onClick={() => void cancel()}>
              <Icon name="stop" size={13} />
              停止
            </button>
          ) : (
            <button
              className="send-button"
              disabled={!draft.ready || sending || !draft.value.body.trim()}
              onClick={() => void send()}
            >
              {sending ? "发送中…" : "发送"}
              <Icon name="arrow" size={16} />
            </button>
          )}
        </div>
        {!configured && (
          <button className="connect-model-link" onClick={onSettings}>
            连接模型后即可讨论
          </button>
        )}
        <ErrorNotice text={error || draft.error} />
      </div>
      {source && (
        <SourcePreview source={source} onClose={() => setSource(null)} />
      )}{" "}
      {review && (
        <SaveConclusion
          message={review}
          topic={topic}
          onClose={() => setReview(null)}
          onSaved={(r) => {
            setReceipt(r);
            setSavedNotice("已保存为一条新记忆，原话与讨论出处均保留。");
            undoRequest.current = uid();
            setReview(null);
            onRefresh();
          }}
        />
      )}
    </section>
  );
}
