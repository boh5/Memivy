import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { call, errorText, type Key, type CaptureResult } from "./api";
import { ErrorNotice } from "./components";
import { useDraft } from "./useDraft";
import { isRecallSubmitKey, isSubmitKey } from "./keyboard";
import { useVoice } from "./useVoice";
import { VoiceButton, VoiceFeedback } from "./VoiceInput";
import DraftConflict from "./DraftConflict";
import { useNotice } from "../i18n/react";
export default function CaptureForm({
  onSaved,
  onAsk,
  mode,
  onMode,
  onEdit,
  focus, quick = false, sourceApp = "Memivy", onBusy, onReady, presentation = "panel", onDraftChange, visible = true,
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
  visible?: boolean;
}) {
  const { t } = useTranslation("workspace");
  const draft = useDraft(quick ? (mode === "capture" ? "quick_capture" : "quick_question") : (mode === "capture" ? "capture" : "question"), {
    title: "",
    body: "",
    expected_version: null,
    ...(quick && mode === "capture" ? { origin: { kind: "user" as const, app: sourceApp, project: null, uri: null } } : {}),
  });
  const [busy, setBusy] = useState(false),
    [error, setError] = useNotice();
  const input = useRef<HTMLTextAreaElement>(null),
    lock = useRef(false),
    composing = useRef(false);
  const voice = useVoice(draft.value.key, draft.value.body, body => { draft.update({body}); onEdit(); }, () => draft.flush(), draft.ready,
    () => [input.current?.selectionStart ?? draft.value.body.length, input.current?.selectionEnd ?? draft.value.body.length]);
  useEffect(() => { if (!visible && voice.active) void voice.finish().catch(e => setError(errorText(e))); }, [visible, voice.active]);
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
    if (lock.current || !draft.ready || voice.busy) return;
    const submittedAt = Date.now();
    lock.current = true;
    setBusy(true);
    onBusy?.(true);
    setError("");
    try {
      await voice.finish();
      const d = await draft.flush();
      if (!d.body.trim()) return;
      if (mode === "ask") {
        await onAsk(d.body, d.request_id);
        await draft.clear(d.request_id);
        await voice.consumed();
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
      await voice.consumed();
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
              disabled={busy || voice.active}
            >
              <Icon name="plus" size={14} />
              {t("capture.captureMode")}
            </button>
            <button
              aria-pressed={mode === "ask"}
              onClick={() => onMode("ask")}
              disabled={busy || voice.active}
            >
              <Icon name="spark" size={14} />
              {t("capture.askMode")}
            </button>
          </div>
          <span>
            {mode === "capture" ? t("capture.captureHint") : t("capture.askHint")}
          </span>
        </div>}
        <textarea
          ref={input}
          aria-label={mode === "capture" ? t("capture.captureAria") : presentation === "query" ? t("capture.queryAria") : t("capture.askAria")}
          rows={presentation === "query" ? 3 : undefined}
          title={presentation === "query" ? t("capture.queryTitle") : undefined}
          placeholder={
            mode === "capture"
              ? t("capture.capturePlaceholder")
              : presentation === "query" ? t("capture.queryPlaceholder") : t("capture.askPlaceholder")
          }
          value={draft.value.body}
          disabled={!draft.ready || busy}
          readOnly={voice.active}
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
        <VoiceFeedback voice={voice} />
        {presentation === "query" ? <div className="recall-panel-footer">
          <span>{t("capture.queryKeyboardHint")}</span>
          <div className="voice-submit-actions"><VoiceButton voice={voice} disabled={!draft.ready || busy} />
          <button className="recall-submit" aria-label={t("capture.findAnswerAria")} title={t("capture.findAnswerTitle")}
            disabled={!draft.ready || busy || voice.busy || (!draft.value.body.trim() && !voice.active)} onClick={() => void save()}>
            <Icon name={busy ? "refresh" : "arrow"} size={16} />
          </button></div>
        </div> : <div className="composer-bottom">
          <span>
            {mode === "capture" ? t("capture.localRaw") : t("capture.unsavedDiscussion")}{" "}
            · {t("capture.submitShortcut")}
          </span>
          <div className="voice-submit-actions"><VoiceButton voice={voice} disabled={!draft.ready || busy} />
          <button
            className="send-button"
            disabled={!draft.ready || busy || voice.busy || (!draft.value.body.trim() && !voice.active)}
            onClick={() => void save()}
          >
            {busy
              ? mode === "capture"
                ? t("capture.saving")
                : t("capture.sending")
              : mode === "capture"
                ? t("capture.captureButton")
                : t("capture.askButton")}
            <Icon name="arrow" size={16} />
          </button></div>
        </div>}
      </div>
      {quick && mode === "capture" && <div className="quick-source">
        <span>{t("capture.source")}</span>
        {draft.value.origin?.app && draft.value.origin.app !== "Memivy" ? <span className="source-chip">{draft.value.origin.app}<button aria-label={t("capture.removeAppSource")} disabled={busy || !draft.ready} onClick={() => draft.update({ origin: { ...draft.value.origin!, app: "Memivy" } })}>×</button></span> : <span>{t("capture.onlyThisText")}</span>}
        <details><summary>{t("capture.attachments")}</summary>
          <input aria-label={t("capture.sourceUriAria")} placeholder={t("capture.sourceUriPlaceholder")} disabled={busy || !draft.ready} value={draft.value.origin?.uri || ""} onChange={e => draft.update({ origin: { kind: "user", app: draft.value.origin?.app || "Memivy", project: null, uri: e.target.value || null } })} />
        </details>
      </div>}
      <ErrorNotice text={error || draft.error} />
      <DraftConflict draft={draft} />
    </>
  );
}
