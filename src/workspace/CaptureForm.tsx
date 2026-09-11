import { useEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import { call, errorText, type Key, type CaptureResult } from "./api";
import { ErrorNotice } from "./components";
import { useDraft } from "./useDraft";
import { isRecallSubmitKey, isSubmitKey } from "./keyboard";
import DraftConflict from "./DraftConflict";
export default function CaptureForm({
  onSaved,
  onAsk,
  mode,
  onMode,
  onEdit,
  focus, quick = false, sourceApp = "Memivy", onBusy, onReady, presentation = "panel", onDraftChange,
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
  presentation?: "panel" | "capture" | "query";
  onDraftChange?: (body: string) => void;
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
    if (draft.ready) onDraftChange?.(draft.value.body);
  }, [draft.ready, draft.value.body, onDraftChange]);
  useEffect(() => {
    if (focus && draft.ready) { input.current?.focus(); onReady?.(); }
  }, [focus, draft.ready]);
  useEffect(() => {
    // Pinned capture stays ready for another note; a submitted query hands focus
    // to the discussion instead of taking it back when the request finishes.
    if (presentation !== "query" && focus && draft.ready && !busy) input.current?.focus();
  }, [busy, presentation, focus, draft.ready]);
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
      const raw = await call<CaptureResult>(quick ? "desktop_capture" : "library_capture", {
        ...(quick ? { submittedAt } : {}),
        request: {
          request_id: d.request_id,
          text: d.body,
          origin: d.origin || { kind: "user", app: "Memivy", project: null, uri: null },
        },
      });
      await draft.clear(d.request_id);
      onSaved({ kind: "memory", id: raw.memory_id });
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
      <div className={presentation === "query" ? "recall-input" : "workspace-composer"}>
        {presentation === "panel" && <div className="composer-top">
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
        </div>}
        <textarea
          ref={input}
          aria-label={mode === "capture" ? "记下想法" : presentation === "query" ? "搜索记忆或提问" : "问一问"}
          rows={presentation === "query" ? 3 : undefined}
          title={presentation === "query" ? "AI 从当前范围的记忆中查找并回答 · Enter 提问，Shift+Enter 换行" : undefined}
          placeholder={
            mode === "capture"
              ? "一句想法，一段刚刚说过的话……"
              : presentation === "query" ? "你想找回什么，或了解什么？" : "关于过去的记录，你想问些什么？"
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
            const submit = presentation === "query" ? isRecallSubmitKey : isSubmitKey;
            if (submit({ ...e, isComposing: e.nativeEvent.isComposing }, composing.current)) {
              e.preventDefault();
              void save();
            }
          }}
        />
        {presentation === "query" ? <div className="recall-panel-footer">
          <span>Enter 提问 · Shift+Enter 换行</span>
          <button className="recall-submit" aria-label="从记忆中查找并回答" title="查找并回答"
            disabled={!draft.ready || busy || !draft.value.body.trim()} onClick={() => void save()}>
            <Icon name={busy ? "refresh" : "arrow"} size={16} />
          </button>
        </div> : <div className="composer-bottom">
          <span>
            {mode === "capture" ? "原话先保存在本机" : "讨论不会自动存为记忆"}{" "}
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
        </div>}
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
