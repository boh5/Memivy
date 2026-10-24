import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNotice } from "../i18n/react";
import { call, errorText, fullDate, native, uid, type Collection, type ContextExportDraft, type ContextExportSelection, type ContextExportSnapshot, type Detail, type Page, type Row } from "./api";
import { ErrorNotice, Modal } from "./components";
import { expireQueries, useResourceVersion } from "./resources";
import { useDraft } from "./useDraft";
import DraftConflict from "./DraftConflict";
import Markdown from "./Markdown";
import MarkdownEditor from "./MarkdownEditor";
import Select from "./Select";
import { CONTEXT_EXPORT_LIMIT, contextCopyText, contextSourceFooter, fullContextText } from "./contextExportText";
import "./context-export.css";

const isStale = (error: unknown) => !!error && typeof error === "object" && "code" in error && ["context_export_stale", "unavailable", "conflict"].includes(String(error.code));

export default function ContextExport({ collections, collectionId, initialSelection, configured, onClose }: {
  collections: Collection[]; collectionId?: string; initialSelection?: ContextExportSelection;
  configured: boolean; onClose: () => void;
}) {
  const { t, i18n } = useTranslation("workspace");
  const empty: ContextExportDraft = { snapshot: null, mode: "full", selections: initialSelection ? [initialSelection] : [], collection_id: collectionId };
  const draft = useDraft("context_export", { title: "", body: "", expected_version: null, context_export: empty });
  const context = draft.value.context_export || empty;
  const current = useRef({ context, body: draft.value.body }); current.current = { context, body: draft.value.body };
  const [picking, setPicking] = useState(true), [initialized, setInitialized] = useState(false);
  const [query, setQuery] = useState("");
  const [page, setPage] = useState<Page>({ items: [], next_offset: null });
  const [loading, setLoading] = useState(false), [selecting, setSelecting] = useState(false);
  const [busy, setBusy] = useState<"" | "prepare" | "generate" | "copy">("");
  const [error, setError] = useNotice(), [listError, setListError] = useNotice();
  const [stale, setStale] = useState(false), [copied, setCopied] = useState(false), [closing, setClosing] = useState(false);
  const alive = useRef(false), operation = useRef(0), job = useRef<string | null>(null), locked = useRef(false);
  const validationSequence = useRef(0);
  const querySequence = useRef(0), selectionLock = useRef(false), closeLock = useRef(false);
  const revision = useResourceVersion([{ domain: "memory" }, { domain: "collection" }]);
  const snapshot = context.snapshot;
  const snapshotKey = JSON.stringify(snapshot);
  const footer = snapshot ? contextSourceFooter(snapshot, {
    sources: t("contextExport.sources"), version: t("contextExport.version"), captured: t("contextExport.captured"), localSource: t("contextExport.localSource"),
    user: t("contextExport.sourceUser"), agent: t("contextExport.sourceAgent"), conversation: t("contextExport.sourceConversation"), discussion: t("contextExport.sourceDiscussion"), imported: t("contextExport.sourceImported"),
    userMessage: t("contextExport.userMessage"), assistantMessage: t("contextExport.assistantMessage"), confirmedByUser: t("contextExport.confirmedByUser"),
    application: t("contextExport.application"), file: t("contextExport.file"), importedAt: t("contextExport.importedAt"), source: t("contextExport.source"), attributionHelp: t("contextExport.attributionHelp"),
  }, fullDate) : "";
  const scope = snapshot ? t("contextExport.scope", { count: snapshot.items.length }) : "";

  function update(next: ContextExportDraft, body = current.current.body) {
    current.current = { context: next, body };
    draft.update({ context_export: next, body });
    setCopied(false);
  }
  function cancel() {
    ++operation.current;
    const id = job.current; job.current = null;
    if (id) void call("context_export_cancel", { id }).catch(e => { if (alive.current) setError(errorText(e)); });
    locked.current = false;
    if (alive.current) setBusy("");
  }
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; cancel(); ++querySequence.current; };
  }, []);
  useEffect(() => {
    if (!draft.ready || initialized) return;
    setInitialized(true);
    setPicking(!context.snapshot);
  }, [draft.ready, initialized, context.snapshot]);
  useEffect(() => {
    if (!initialized || !picking) return;
    const sequence = ++querySequence.current;
    setLoading(true); setListError(""); setPage({ items: [], next_offset: null });
    void call<Page>("library_query", { query: { query, trash: false, collection_id: context.collection_id || undefined, limit: 40 } })
      .then(result => { if (sequence === querySequence.current) setPage(result); })
      .catch(e => { if (sequence === querySequence.current) setListError(errorText(e)); })
      .finally(() => { if (sequence === querySequence.current) setLoading(false); });
    return () => { ++querySequence.current; };
  }, [initialized, picking, query, context.collection_id, revision]);
  useEffect(() => {
    if (!snapshot) return;
    let active = true;
    const sequence = ++validationSequence.current;
    setCopied(false);
    void call("context_export_validate", { snapshot })
      .then(() => { if (active && sequence === validationSequence.current) setStale(false); })
      .catch(e => { if (active && sequence === validationSequence.current) { setStale(true); if (!isStale(e)) setError(errorText(e)); } });
    return () => { active = false; };
  }, [snapshotKey, revision]);

  async function more() {
    if (loading || page.next_offset === null) return;
    const sequence = querySequence.current; setLoading(true); setListError("");
    try {
      const result = await call<Page>("library_query", { query: { query, trash: false, collection_id: context.collection_id || undefined, limit: 40, offset: page.next_offset } });
      if (sequence === querySequence.current) setPage(previous => ({ ...result, items: [...new Map([...previous.items, ...result.items].map(row => [row.key.id, row])).values()] }));
    } catch (e) { if (sequence === querySequence.current) setListError(errorText(e)); }
    finally { if (sequence === querySequence.current) setLoading(false); }
  }
  async function select(row: Row) {
    if (selectionLock.current || locked.current) return;
    const previous = current.current.context;
    if (previous.selections.some(item => item.memory_id === row.key.id)) {
      update({ ...previous, selections: previous.selections.filter(item => item.memory_id !== row.key.id) }); return;
    }
    if (previous.selections.length >= CONTEXT_EXPORT_LIMIT) return;
    selectionLock.current = true; setSelecting(true); setError("");
    try {
      const detail = await call<Detail>("library_detail", { key: row.key, archives: false });
      if (alive.current) {
        if (detail.state !== "active") throw { code: "unavailable" };
        const state = current.current.context;
        update({ ...state, selections: [...state.selections, { memory_id: detail.key.id, version_id: detail.current.id, title: detail.title }] });
      }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { selectionLock.current = false; if (alive.current) setSelecting(false); }
  }
  async function prepare(refresh = false) {
    if (locked.current || selectionLock.current || !draft.ready || draft.conflicted || !context.selections.length) return;
    locked.current = true; setBusy("prepare"); setError(""); setCopied(false);
    const sequence = ++operation.current;
    try {
      let selections = current.current.context.selections;
      if (refresh) {
        await expireQueries(["library_detail"]);
        selections = await Promise.all(selections.map(async item => {
          const detail = await call<Detail>("library_detail", { key: { kind: "memory", id: item.memory_id }, archives: false });
          if (detail.state !== "active") throw { code: "unavailable" };
          return { memory_id: item.memory_id, version_id: detail.current.id, title: detail.title };
        }));
      }
      if (!alive.current || operation.current !== sequence) return;
      const result = await call<ContextExportSnapshot>("context_export_prepare", { selections: selections.map(({ memory_id, version_id }) => ({ memory_id, version_id })) });
      if (!alive.current || operation.current !== sequence) return;
      update({ ...current.current.context, selections, snapshot: result, mode: "full" }, fullContextText(result));
      setStale(false); setPicking(false); await draft.flush();
    } catch (e) { if (alive.current && operation.current === sequence) { if (isStale(e)) setStale(true); setError(errorText(e)); } }
    finally { if (operation.current === sequence) { locked.current = false; if (alive.current) setBusy(""); } }
  }
  async function generate() {
    if (!snapshot || stale || locked.current || draft.conflicted) return;
    locked.current = true; setBusy("generate"); setError(""); setCopied(false);
    const sequence = ++operation.current, id = uid(); job.current = id;
    ++validationSequence.current;
    try {
      await draft.flush();
      if (!alive.current || sequence !== operation.current) return;
      await call("context_export_start", { id });
      if (!alive.current || sequence !== operation.current) { void call("context_export_cancel", { id }).catch(() => {}); return; }
      const body = await call<string>("context_export_generate", { id, snapshot, language: i18n.resolvedLanguage || i18n.language });
      if (!alive.current || sequence !== operation.current) return;
      update({ ...current.current.context, mode: "summary" }, body);
      await draft.flush();
    } catch (e) { if (alive.current && sequence === operation.current) { if (isStale(e)) setStale(true); setError(errorText(e)); } }
    finally { if (sequence === operation.current) { job.current = null; locked.current = false; if (alive.current) setBusy(""); } }
  }
  async function copy() {
    if (!snapshot || stale || locked.current || !current.current.body.trim() || draft.conflicted) return;
    locked.current = true; setBusy("copy"); setError(""); setCopied(false);
    const sequence = ++operation.current;
    ++validationSequence.current;
    try {
      await draft.flush();
      await call("context_export_validate", { snapshot });
      if (!alive.current || sequence !== operation.current) return;
      const body = contextCopyText(current.current.body, footer, scope);
      if (native) await call("context_export_copy", { snapshot, body });
      else await navigator.clipboard.writeText(body);
      if (alive.current && sequence === operation.current) setCopied(true);
    } catch (e) { if (alive.current && sequence === operation.current) { if (isStale(e)) setStale(true); setError(errorText(e)); } }
    finally { if (sequence === operation.current) { locked.current = false; if (alive.current) setBusy(""); } }
  }
  async function close() {
    if (closeLock.current) return;
    closeLock.current = true; setClosing(true); cancel();
    try { await draft.flush(); if (alive.current) onClose(); }
    catch (e) { if (alive.current) setError(errorText(e)); }
    finally { closeLock.current = false; if (alive.current) setClosing(false); }
  }
  const disabled = !draft.ready || !!busy || closing || draft.conflicted;
  return <Modal title={t("contextExport.title")} className="context-export-dialog" onClose={() => void close()}>
    <div className="context-export-content">
      <ErrorNotice text={draft.error || error} />
      <DraftConflict draft={draft} />
      {!draft.ready ? <p role="status">{t("contextExport.loadingDraft")}</p> : <>
        {snapshot && <p className="field-help">{t("contextExport.draftHelp")}</p>}
        {initialSelection && !context.selections.some(item => item.memory_id === initialSelection.memory_id) && <div className="workspace-warning"><p>{t("contextExport.incoming")}</p><button disabled={disabled || selecting} onClick={() => { update({ ...context, selections: [initialSelection] }); setPicking(true); }}>{t("contextExport.useIncoming", { title: initialSelection.title })}</button></div>}
        {!initialSelection && (context.collection_id || "") !== (collectionId || "") && <div className="workspace-warning"><p>{t("contextExport.incomingScope")}</p><button disabled={disabled || selecting} onClick={() => { update({ ...context, collection_id: collectionId, selections: [] }); setQuery(""); setPicking(true); }}>{t("contextExport.useScope", { name: collections.find(item => item.id === collectionId)?.name || t("contextExport.allMemories") })}</button></div>}
        {stale && <div className="workspace-warning"><p>{t("contextExport.stale")}</p><button disabled={disabled || selecting} onClick={() => { setPicking(true); setCopied(false); }}>{t("contextExport.reviewSelection")}</button></div>}
        {picking ? <>
          <p className="field-help">{t("contextExport.pickerHelp", { limit: CONTEXT_EXPORT_LIMIT })}</p>
          <div className="context-export-filters">
            <Select aria-label={t("contextExport.filter")} value={context.collection_id || ""} disabled={disabled || selecting} onChange={e => update({ ...context, collection_id: e.target.value || undefined })}>
              <option value="">{t("contextExport.allMemories")}</option>
              {collections.map(collection => <option key={collection.id} value={collection.id}>{collection.name}</option>)}
            </Select>
            <input type="search" data-modal-autofocus aria-label={t("contextExport.search")} placeholder={t("contextExport.search")} value={query} onChange={e => setQuery(e.target.value)} disabled={disabled} />
          </div>
          <p className="context-export-count" role="status">{t("contextExport.selected", { count: context.selections.length, limit: CONTEXT_EXPORT_LIMIT })}</p>
          {!!context.selections.length && <div className="context-export-selected" aria-label={t("contextExport.selectedList")}>{context.selections.map(item => <button key={item.memory_id} disabled={disabled || selecting} aria-label={t("contextExport.remove", { title: item.title })} onClick={() => update({ ...context, selections: context.selections.filter(other => other.memory_id !== item.memory_id) })}>{item.title}<span aria-hidden="true"> ×</span></button>)}</div>}
          <ErrorNotice text={listError} />
          <div className="context-export-choices" aria-busy={loading}>
            {page.items.map(row => { const checked = context.selections.some(item => item.memory_id === row.key.id); return <label key={row.key.id} className="context-export-choice"><input type="checkbox" checked={checked} disabled={disabled || selecting || (!checked && context.selections.length >= CONTEXT_EXPORT_LIMIT)} onChange={() => void select(row)} /><span><strong>{row.title}</strong><span>{row.snippet}</span></span></label>; })}
            {loading && <p role="status">{t("contextExport.loading")}</p>}
            {!loading && !page.items.length && !listError && <p>{t("contextExport.noMatches")}</p>}
            {page.next_offset !== null && <button className="outline-button" disabled={loading} onClick={() => void more()}>{t("contextExport.loadMore")}</button>}
          </div>
          {snapshot && <p className="field-help">{t("contextExport.replaceHelp")}</p>}
          <div className="action-row">
            <button className="send-button" disabled={disabled || selecting || !context.selections.length} onClick={() => void prepare(!!snapshot || stale)}>{busy === "prepare" ? t("contextExport.preparing") : snapshot ? t("contextExport.replacePreview") : t("contextExport.prepare")}</button>
            {snapshot && <button className="outline-button" disabled={disabled} onClick={() => setPicking(false)}>{t("contextExport.backToPreview")}</button>}
          </div>
        </> : snapshot && <>
          <div className="context-export-preview-heading"><div><p className="context-export-count">{scope}</p><strong>{context.mode === "full" ? t("contextExport.full") : t("contextExport.summary")}</strong></div><button className="outline-button" disabled={disabled} onClick={() => { setPicking(true); setCopied(false); }}>{t("contextExport.choose")}</button></div>
          <div className="action-row context-export-generate">
            <button className="outline-button" disabled={disabled || stale || !configured} onClick={() => void generate()}>{busy === "generate" ? t("contextExport.generating") : t("contextExport.generate")}</button>
            {busy === "generate" && <button className="quiet" onClick={cancel}>{t("contextExport.cancelGeneration")}</button>}
            {context.mode === "summary" && <button className="quiet" disabled={disabled} onClick={() => { update({ ...context, mode: "full" }, fullContextText(snapshot)); setError(""); }}>{t("contextExport.restoreFull")}</button>}
          </div>
          <p className="field-help">{configured ? t("contextExport.generationHelp") : t("contextExport.noModel")}</p>
          <MarkdownEditor label={t("contextExport.preview")} value={draft.value.body} disabled={disabled} onChange={body => update(current.current.context, body)} />
          <details className="context-export-sources"><summary>{t("contextExport.sourcesHelp")}</summary><Markdown text={footer} /></details>
          <div className="action-row"><button className="send-button" disabled={disabled || stale || !draft.value.body.trim()} onClick={() => void copy()}>{busy === "copy" ? t("contextExport.copying") : t("contextExport.copy")}</button><span role="status">{copied ? t("contextExport.copied") : ""}</span></div>
        </>}
      </>}
    </div>
    <div className="action-row context-export-close"><button className="quiet" disabled={closing} onClick={() => void close()}>{closing ? t("contextExport.savingDraft") : t("contextExport.close")}</button></div>
  </Modal>;
}
