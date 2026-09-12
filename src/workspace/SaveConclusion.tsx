import Select from "./Select";
import { message as uiMessage } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import MarkdownEditor from "./MarkdownEditor";
import Markdown from "./Markdown";
import { useEffect, useRef, useState } from "react";
import { call, errorText, type Detail, type Message, type Page, type Receipt, type Source, type Topic, type ConclusionDestination } from "./api";
import { ErrorNotice, Modal } from "./components";

import { useDraft } from "./useDraft";
import DraftConflict from "./DraftConflict";
export default function SaveConclusion({ message, topic, context, onClose, onSaved }: {
  message: Message; topic: Topic; context: Source[]; onClose: () => void; onSaved: (receipt: Receipt) => void;
}) {
  const { t } = useTranslation("workspace");
  const draft = useDraft(`conclusion:${message.id}`, {
    title: topic.title, body: message.answer?.conclusion || message.text,
    expected_version: null, conclusion: { destination: { kind: "new" }, merged_body: null },
  });
  const { title, body: text } = draft.value;
  const destination: ConclusionDestination = draft.value.conclusion?.destination || { kind: "new" };
  const merged = draft.value.conclusion?.merged_body ?? null;
  const [unresolvedSelection, setUnresolvedSelection] = useState<string | null>(null);
  const selection = unresolvedSelection ?? (destination.kind === "existing" ? destination.memory_id : "");
  const setTitle = (value: string) => draft.update({ title: value });
  const setText = (value: string) => draft.update({ body: value });
  const setMerged = (value: string | null) => draft.update({ conclusion: { destination, merged_body: value } });
  const [suggestions, setSuggestions] = useState<[string, string][]>([]);
  const [query, setQuery] = useState(""), [rows, setRows] = useState<Page["items"]>([]);
  const [target, setTarget] = useState<Detail | null>(null);
  const [needsCheck, setNeedsCheck] = useState(false);
  const [busy, setBusy] = useState(false), [previewing, setPreviewing] = useState(false), [error, setError] = useNotice();
  const mergedField = useRef<HTMLDivElement | null>(null);
  useEffect(() => { if (merged !== null) mergedField.current?.scrollIntoView({ block: "center" }); }, [merged !== null]);
  const locked = useRef(false), generation = useRef(0), alive = useRef(true);
  useEffect(() => { generation.current++; setPreviewing(false); }, [draft.value.request_id]);
  useEffect(() => { alive.current = true; return () => { alive.current = false; generation.current++; }; }, []);
  useEffect(() => {
    let valid = true;
    const timer = setTimeout(() => { void call<Page>("library_query", { query: { query, trash: false, limit: 30 } }).then(page => { if (valid) setRows(page.items.filter(r => r.key.kind === "memory")); }).catch(e => { if (valid) setError(errorText(e)); }); }, 150);
    return () => { valid = false; clearTimeout(timer); };
  }, [query]);
  useEffect(() => {
    let valid = true;
    void call<[string, string][]>("discussion_targets", { sources: context }).then(value => { if (valid) setSuggestions(value); }).catch(e => { if (valid) setError(errorText(e)); });
    return () => { valid = false; };
  }, [JSON.stringify(context)]);
  function changed() { generation.current++; setMerged(null); setPreviewing(false); }
  // Loading a saved destination never silently rebases a reviewed rewrite.
  useEffect(() => {
    let valid = true;
    setTarget(null);
    if (!draft.ready || destination.kind !== "existing") { setNeedsCheck(false); return; }
    const expected = destination.expected_version;
    void call<Detail>("library_detail", { key: { kind: "memory", id: destination.memory_id } }).then(detail => {
      if (!valid) return;
      if (detail.state !== "active" || detail.current?.id !== expected) {
        setNeedsCheck(true); setError(uiMessage("workspace", "saveConclusion.destinationChanged"));
      } else { setTarget(detail); setNeedsCheck(false); }
    }).catch(e => { if (valid) { setNeedsCheck(true); setError(errorText(e)); } });
    return () => { valid = false; };
  }, [draft.ready, destination.kind === "existing" ? destination.memory_id : "", destination.kind === "existing" ? destination.expected_version : ""]);
  async function select(id: string, recheck = false) {
    generation.current++; setPreviewing(false); setError(""); setTarget(null); setUnresolvedSelection(id);
    if (!id) {
      draft.update({ conclusion: { destination: { kind: "new" }, merged_body: null } });
      setTarget(null); setNeedsCheck(false); setUnresolvedSelection(null); return;
    }
    const run = generation.current;
    setBusy(true);
    try {
      const detail = await call<Detail>("library_detail", { key: { kind: "memory", id } });
      if (detail.state !== "active" || !detail.current) {
        setError(uiMessage("workspace", "saveConclusion.destinationUnavailable"));
        return;
      }
      if (alive.current && run === generation.current) {
        setTarget(detail); setNeedsCheck(false); setUnresolvedSelection(null);
        draft.update({ title: recheck ? title : detail.title, conclusion: {
          destination: { kind: "existing", memory_id: id, expected_version: detail.current.id },
          merged_body: recheck ? merged : null,
        } });
      }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { if (alive.current) setBusy(false); }
  }
  async function preview() {
    if (!target || locked.current) return;
    const run = ++generation.current;
    setPreviewing(true); setError("");
    try {
      const body = await call<string>("discussion_merge", { destination, text });
      if (alive.current && run === generation.current) { setMerged(body); }
    } catch (e) { if (alive.current && run === generation.current) setError(errorText(e)); }
    finally { if (alive.current && run === generation.current) setPreviewing(false); }
  }
  async function save() {
    if (locked.current || !draft.ready || busy || previewing || needsCheck || (selection && !target)) return;
    locked.current = true; setBusy(true); setError("");
    let persisted = false;
    try {
      // Persist the exact reviewed payload before any durable memory mutation.
      const saved = await draft.flush(true);
      persisted = true;
      const receipt = await call<Receipt>("discussion_save", { request: { request_id: saved.request_id, message_id: message.id, destination: saved.conclusion!.destination, title: saved.title, text: saved.body }, mergedBody: saved.conclusion!.merged_body });
      if (receipt.status === "needs_review") {
        setNeedsCheck(true);
        setError(uiMessage("workspace", "saveConclusion.destinationChanged"));
        return;
      }
      // Core already consumed the exact submitted draft atomically. A local
      // cache refresh failure must not turn a successful save into a retry.
      await draft.clear(saved.request_id).catch(() => {});
      onSaved(receipt);
    } catch (e) {
      setError(uiMessage("errors", persisted ? "save_unconfirmed" : "review_unsaved", { error: errorText(e) }));
    }
    finally { locked.current = false; setBusy(false); }
  }
  return <Modal title={t("saveConclusion.title")} onClose={() => { if (!busy) onClose(); }}>
    <p className="field-help">{t("saveConclusion.help")}</p>
    <MarkdownEditor label={t("saveConclusion.editorLabel")} value={text} disabled={busy || !draft.ready} onChange={value => { setText(value); changed(); }} />
    <label className="discussion-field">{t("saveConclusion.destinationLabel")}<Select aria-label={t("saveConclusion.destinationAria")} value={selection} disabled={busy || !draft.ready} onChange={e => void select(e.target.value)}>
      <option value="">{t("saveConclusion.newMemory")}</option>
      {selection && !target && !rows.some(r => r.key.id === selection) && <option value={selection}>{t("saveConclusion.unresolvedDestination")}</option>}
      {target && !rows.some(r => r.key.id === target.key.id) && <option value={target.key.id}>{target.title}</option>}
      {suggestions.filter(([id]) => !rows.some(r => r.key.id === id) && id !== target?.key.id).map(([id, name]) => <option key={id} value={id}>{t("saveConclusion.discussionSuggestionOption", { name })}</option>)}
      {rows.map(row => <option key={row.key.id} value={row.key.id}>{suggestions.some(([id]) => id === row.key.id) ? t("saveConclusion.discussionSuggestionOption", { name: row.title }) : row.title}</option>)}
    </Select></label>
    <label className="discussion-field">{t("saveConclusion.searchLabel")}<input value={query} disabled={busy || !draft.ready} placeholder={t("saveConclusion.searchPlaceholder")} onChange={e => setQuery(e.target.value)} /></label>
    <label className="discussion-field">{target ? t("saveConclusion.savedTitle") : t("saveConclusion.titleLabel")}<input value={title} disabled={busy || !draft.ready} onChange={e => { setTitle(e.target.value); }} /></label>
    {needsCheck && selection && <button className="outline-button" disabled={busy} onClick={() => void select(selection, true)}>{t("saveConclusion.recheck")}</button>}
    {target && <>
      <p className="field-help">{t("saveConclusion.defaultAdd", { title: target.title })}</p>
      <details><summary>{t("saveConclusion.currentBody")}</summary><Markdown text={target.body} /></details>
      <div className="action-row"><button className="outline-button" disabled={busy || previewing || !text.trim()} onClick={() => void preview()}>{previewing ? t("saveConclusion.previewing") : t("saveConclusion.preview")}</button>
        {(merged !== null || previewing) && <button className="quiet" disabled={busy || !draft.ready} onClick={changed}>{t("saveConclusion.useDefault")}</button>}
      </div>
    </>}
    {merged !== null && <div ref={mergedField}><MarkdownEditor label={t("saveConclusion.mergedLabel")} value={merged} disabled={busy || !draft.ready} onChange={value => { generation.current++; setPreviewing(false); setMerged(value); }} /><p className="field-help">{t("saveConclusion.mergedHelp")}</p></div>}
    <ErrorNotice text={error || draft.error} />
    <DraftConflict draft={draft} />
    <div className="action-row"><button className="send-button" disabled={!draft.ready || needsCheck || busy || previewing || !!(selection && !target) || !title.trim() || !text.trim() || merged?.trim() === ""} onClick={() => void save()}>{busy ? t("saveConclusion.saving") : merged !== null ? t("saveConclusion.confirmMerged") : target ? t("saveConclusion.confirmExisting") : t("saveConclusion.confirmNew")}</button><button className="outline-button" disabled={busy || !draft.ready} onClick={onClose}>{t("saveConclusion.cancel")}</button></div>
  </Modal>;
}
