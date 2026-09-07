import { useEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import { call, errorText, type Key, type Raw } from "./api";
import { ErrorNotice } from "./components";
import { useDraft } from "./useDraft";
import { isSubmitKey } from "./keyboard";
import DraftConflict from "./DraftConflict";
export default function CaptureForm({
  onSaved,
  onAsk,
  mode,
  onMode,
  onEdit,
  focus, quick = false, sourceApp = "Memivy", onBusy, onReady,
}: {
  onSaved: (key: Key) => void;
  onAsk: (question: string, id: string) => Promise<void>;
  mode: "capture" | "ask";
  onMode: (mode: "capture" | "ask") => void;
  onEdit: () => void;
  focus: number;
  quick?: boolean;
  sourceApp?: string;
  onBusy?: (busy: boolean) => void;
  onReady?: () => void;
}) {
  const draft = useDraft(quick ? (mode === "capture" ? "quick_capture" : "quick_question") : (mode === "capture" ? "capture" : "question"), {
    title: "",
    body: "",
    expected_version: null,
    ...(quick && mode === "capture" ? { origin: { kind: "user" as const, app: sourceApp, project: null, uri: null } } : {}),
  });
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const input = useRef<HTMLTextAreaElement>(null),
    lock = useRef(false),
    composing = useRef(false);
  useEffect(() => {
    if (focus && draft.ready && !busy) { input.current?.focus(); onReady?.(); }
  }, [focus, draft.ready, busy]);
  async function save() {
    if (lock.current || !draft.ready || !draft.value.body.trim()) return;
    const submittedAt = Date.now();
    lock.current = true;
    setBusy(true);
    onBusy?.(true);
    setError("");
    try {
      const d = await draft.flush();
      if (mode === "ask") {
        await onAsk(d.body, d.request_id);
        await draft.clear(d.request_id);
        return;
      }
      const raw = await call<Raw>(quick ? "desktop_capture" : "library_capture", {
        ...(quick ? { submittedAt } : {}),
        request: {
          request_id: d.request_id,
          text: d.body,
          origin: d.origin || { kind: "user", app: "Memivy", project: null, uri: null },
        },
      });
      await draft.clear(d.request_id);
      onSaved({ kind: "capture", id: raw.id });
      input.current?.focus();
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
      setBusy(false);
      onBusy?.(false);
    }
  }
  return (
    <>
      <div className="workspace-composer">
        <div className="composer-top">
          <div className="segmented">
            <button
              aria-pressed={mode === "capture"}
              onClick={() => onMode("capture")}
              disabled={busy}
            >
              <Icon name="plus" size={14} />
              记一下
            </button>
            <button
              aria-pressed={mode === "ask"}
              onClick={() => onMode("ask")}
              disabled={busy}
            >
              <Icon name="spark" size={14} />
              问一问
            </button>
          </div>
          <span>
            {mode === "capture" ? "想法不用整理好再来" : "结合记忆，一起接着想"}
          </span>
        </div>
        <textarea
          ref={input}
          aria-label={mode === "capture" ? "记下想法" : "问一问"}
          placeholder={
            mode === "capture"
              ? "一句想法，一段刚刚说过的话……"
              : "关于过去的记录，你想问些什么？"
          }
          value={draft.value.body}
          disabled={!draft.ready || busy}
          onChange={(e) => {
            draft.update({ body: e.target.value });
            onEdit();
          }}
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
          }}
          onKeyDown={(e) => {
            if (isSubmitKey({ ...e, isComposing: e.nativeEvent.isComposing }, composing.current)) {
              e.preventDefault();
              void save();
            }
          }}
        />
        <div className="composer-bottom">
          <span>
            {draft.saved
              ? mode === "capture"
                ? "原话先保存在本机"
                : "讨论不会自动存为记忆"
              : "保存草稿中…"}{" "}
            · ⌘ Enter 提交
          </span>
          <button
            className="send-button"
            disabled={!draft.ready || busy || !draft.value.body.trim()}
            onClick={() => void save()}
          >
            {busy
              ? mode === "capture"
                ? "保存中…"
                : "发送中…"
              : mode === "capture"
                ? "记下"
                : "问一问"}
            <Icon name="arrow" size={16} />
          </button>
        </div>
      </div>
      {quick && mode === "capture" && <div className="quick-source">
        <span>来源</span>
        {draft.value.origin?.app && draft.value.origin.app !== "Memivy" ? <span className="source-chip">{draft.value.origin.app}<button aria-label="移除应用来源" disabled={busy || !draft.ready} onClick={() => draft.update({ origin: { ...draft.value.origin!, app: "Memivy" } })}>×</button></span> : <span>仅这段文字</span>}
        <details><summary>附带链接或文件路径</summary>
          <input aria-label="来源链接或文件路径" placeholder="粘贴链接或绝对文件路径" disabled={busy || !draft.ready} value={draft.value.origin?.uri || ""} onChange={e => draft.update({ origin: { kind: "user", app: draft.value.origin?.app || "Memivy", project: null, uri: e.target.value || null } })} />
        </details>
      </div>}
      <ErrorNotice text={error || draft.error} />
      <DraftConflict draft={draft} />
    </>
  );
}
