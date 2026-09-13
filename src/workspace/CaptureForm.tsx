import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { call, errorText, type Detail, type Origin, type Page, type Source } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useDraft } from "./useDraft";
import { isSubmitKey } from "./keyboard";
import { useVoice } from "./useVoice";
import { VoiceButton, VoiceFeedback } from "./VoiceInput";
import DraftConflict from "./DraftConflict";
import { useNotice } from "../i18n/react";

export type InputSubmission = { id: string; text: string; context: Source[]; origin?: Origin };

function MemoryPicker({ selected, onSelect, onClose }: { selected: Source[]; onSelect: (source: Source) => void; onClose: () => void }) {
  const { t } = useTranslation("workspace");
  const [query, setQuery] = useState(""), [page, setPage] = useState<Page | null>(null), [busy, setBusy] = useState(false);
  const [error, setError] = useNotice();
  const [loading, setLoading] = useState(true);
  const generation = useRef(0), locked = useRef(false);
  useEffect(() => {
    const run = ++generation.current;
    setLoading(true);
    const timer = setTimeout(() => {
      void call<Page>("library_query", { query: { query, trash: false, limit: 30 } })
        .then(value => { if (generation.current === run) setPage(value); })
        .catch(e => { if (generation.current === run) setError(errorText(e)); })
        .finally(() => { if (generation.current === run) setLoading(false); });
    }, query ? 120 : 0);
    return () => { generation.current++; clearTimeout(timer); };
  }, [query]);
  async function select(row: Page["items"][number]) {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError("");
    try {
      const detail = await call<Detail>("library_detail", { key: row.key });
      const source: Source = detail.current ? { kind: "version", id: detail.current.id } : { kind: "capture", id: row.key.id };
      if (!selected.some(s => s.kind === source.kind && s.id === source.id)) onSelect(source);
      onClose();
    } catch (e) { setError(errorText(e)); }
    finally { locked.current = false; setBusy(false); }
  }
  async function more() {
    if (locked.current || page?.next_offset == null) return;
    const run = generation.current;
    locked.current = true; setBusy(true);
    try {
      const value = await call<Page>("library_query", { query: { query, trash: false, limit: 30, offset: page.next_offset } });
      if (run === generation.current) setPage({ ...value, items: [...page.items, ...value.items] });
    } catch (e) { setError(errorText(e)); }
    finally { locked.current = false; setBusy(false); }
  }
  return <Modal title={t("input.addMemory")} onClose={onClose}>
    <p className="field-help">{t("input.memoryFocusHelp")}</p>
    <input className="memory-picker-query" data-modal-autofocus aria-label={t("input.searchMemory")} value={query} onChange={e => setQuery(e.target.value)} />
    <div className="memory-picker-list" aria-busy={loading}>
      {loading && !page ? <p className="field-help" role="status">{t("list.loading")}</p> : page?.items.map(row => <button key={`${row.key.kind}:${row.key.id}`} disabled={busy || loading} onClick={() => void select(row)}><strong>{row.title}</strong><span>{row.snippet}</span></button>)}
      {!loading && page?.items.length === 0 && <p className="field-help" role="status">{t("list.emptyFilteredTitle")}</p>}
      {page?.next_offset != null && <button className="quiet" disabled={busy || loading} onClick={() => void more()}>{t("input.moreMemories")}</button>}
    </div>
    <ErrorNotice text={error} />
  </Modal>;
}

export default function CaptureForm({
  onSubmit, onEdit, focus = 0, quick = false, sourceApp = "Memivy", onBusy, onReady,
  presentation = "panel", onDraftChange, visible = true, draftKey, pending = false, onCancel,
  configured = true, onSettings, onSource,
}: {
  onSubmit: (input: InputSubmission) => Promise<void>;
  onEdit?: () => void;
  focus?: number;
  quick?: boolean;
  sourceApp?: string;
  onBusy?: (busy: boolean) => void;
  onReady?: () => void;
  presentation?: "panel" | "query" | "discussion";
  onDraftChange?: (body: string) => void;
  visible?: boolean;
  draftKey?: string;
  pending?: boolean;
  onCancel?: () => void;
  configured?: boolean;
  onSettings?: () => void;
  onSource?: (source: Source) => void;
}) {
  const { t } = useTranslation("workspace");
  const draft = useDraft(draftKey || (quick ? "quick_input" : "input"), {
    title: "", body: "", expected_version: null, context: [],
    origin: { kind: "user", app: sourceApp, project: null, uri: null },
  });
  const [busy, setBusy] = useState(false), [picker, setPicker] = useState(false), [error, setError] = useNotice();
  const [titles, setTitles] = useState<Record<string, string>>({});
  const input = useRef<HTMLTextAreaElement>(null), lock = useRef(false), composing = useRef(false);
  const voice = useVoice(draft.value.key, draft.value.body, body => { draft.update({ body }); onEdit?.(); }, () => draft.flush(), draft.ready,
    () => [input.current?.selectionStart ?? draft.value.body.length, input.current?.selectionEnd ?? draft.value.body.length], quick && visible);
  const context = draft.value.context || [];
  useEffect(() => {
    let active = true;
    void Promise.all(context.map(async s => {
      try { const value = await call<{ title: string }>("discussion_source", { source: s }); return [`${s.kind}:${s.id}`, value.title] as const; }
      catch { return [`${s.kind}:${s.id}`, t("discussion.sourceDeleted")] as const; }
    })).then(rows => { if (active) setTitles(previous => JSON.stringify(previous) === JSON.stringify(Object.fromEntries(rows)) ? previous : Object.fromEntries(rows)); });
    return () => { active = false; };
  }, [JSON.stringify(context), t]);
  useEffect(() => { if (!visible && voice.owned) void voice.finish().catch(e => setError(errorText(e))); }, [visible, voice.owned]);
  useEffect(() => { if (draft.ready) onDraftChange?.(draft.value.body); }, [draft.ready, draft.value.body, onDraftChange]);
  useEffect(() => { if (visible && focus && draft.ready) { input.current?.focus(); onReady?.(); } }, [focus, draft.ready, visible]);
  async function send() {
    if (lock.current || pending || !draft.ready) return;
    lock.current = true; setBusy(true); onBusy?.(true); setError("");
    try {
      const voiceSessionId = await voice.finish();
      const value = await draft.flush();
      if (!value.body.trim()) return;
      // The host acknowledges durable input before checking the configured model.
      await onSubmit({ id: value.request_id, text: value.body, context: value.context || [], origin: value.origin });
      await draft.clear(value.request_id, value.key.startsWith("discussion:"));
      await voice.consumed(voiceSessionId);
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); onBusy?.(false); }
  }
  return <>
    <div hidden={!visible} className={presentation === "query" ? "recall-input" : presentation === "discussion" ? "discussion-composer" : "workspace-composer"}>
      {!!context.length && <div className="discussion-context">{context.map((source, index) => <span key={`${source.kind}:${source.id}`}>
        <button onClick={() => onSource?.(source)} disabled={!onSource}><Icon name="book" size={13} />{titles[`${source.kind}:${source.id}`] || t("discussion.selectedMemory", { suffix: ` ${index + 1}` })}</button>
        <button aria-label={t("discussion.removeEvidence")} disabled={!draft.ready || busy} onClick={() => draft.update({ context: context.filter(s => s !== source) })}>×</button>
      </span>)}</div>}
      <textarea ref={input} aria-label={t("input.aria")} placeholder={t("input.placeholder")} value={draft.value.body}
        disabled={!draft.ready || busy} readOnly={voice.active}
        onChange={e => { draft.update({ body: e.target.value }); onEdit?.(); }}
        onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }}
        onKeyDown={e => { if (isSubmitKey({ ...e, isComposing: e.nativeEvent.isComposing }, composing.current)) { e.preventDefault(); void send(); } }} />
      <VoiceFeedback voice={voice} />
      <div className={presentation === "query" ? "recall-panel-footer" : "composer-bottom"}>
        <div className="composer-tools"><button className="quiet add-memory" disabled={!draft.ready || busy} onClick={() => setPicker(true)} aria-label={t("input.addMemory")}><Icon name="plus" size={14} />{t("input.memory")}</button><span>{t("input.keyboardHint")}</span></div>
        <div className="voice-submit-actions"><VoiceButton voice={voice} disabled={!draft.ready || busy} />
          {pending ? <button className="outline-button" onClick={onCancel}><Icon name="stop" size={13} />{t("discussion.stop")}</button> :
            <button className={presentation === "query" ? "recall-submit" : "send-button"} aria-label={t("input.send")} disabled={!draft.ready || busy || (!draft.value.body.trim() && !voice.active)} onClick={() => void send()}>
              {presentation !== "query" && (busy ? t("discussion.sending") : t("input.send"))}<Icon name={busy ? "refresh" : "arrow"} size={16} />
            </button>}
        </div>
      </div>
      {quick && <div className="quick-source">
        {draft.value.origin?.app && draft.value.origin.app !== "Memivy" ? <span className="source-chip">{draft.value.origin.app}<button aria-label={t("capture.removeAppSource")} disabled={busy || !draft.ready} onClick={() => draft.update({ origin: { ...draft.value.origin!, app: "Memivy" } })}>×</button></span> : null}
        <details><summary>{t("capture.attachments")}</summary><input aria-label={t("capture.sourceUriAria")} placeholder={t("capture.sourceUriPlaceholder")} disabled={busy || !draft.ready} value={draft.value.origin?.uri || ""} onChange={e => draft.update({ origin: { ...draft.value.origin!, uri: e.target.value || null } })} /></details>
      </div>}
      {!configured && onSettings && <button className="connect-model-link" onClick={onSettings}>{t("input.connectModel")}</button>}
      <ErrorNotice text={error || draft.error} /><DraftConflict draft={draft} />
    </div>
    {picker && <MemoryPicker selected={context} onClose={() => setPicker(false)} onSelect={source => draft.update({ context: [...context, source] })} />}
  </>;
}
