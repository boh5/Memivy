import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { translateCatalog } from "../i18n";
import { useNotice } from "../i18n/react";
import { call, errorText, uid, type ImportResult, type Key, type PreparedImport, type Receipt } from "./api";
import { ErrorNotice, Modal } from "./components";
import { importedSources, importOrganizationInstruction, type ImportOrganization } from "./importMemoryBatch";
import "./import-memories.css";

export default function ImportMemories({ open, configured, onClose, onChanged, onOpenRecord, onOrganize, onOpenDiscussion }: {
  open: boolean; configured: boolean; onClose: () => void; onChanged: () => void;
  onOpenRecord: (key: Key) => void; onOrganize: (request: ImportOrganization) => Promise<void>;
  onOpenDiscussion: (id: string) => Promise<void>;
}) {
  const { t, i18n } = useTranslation("workspace");
  const [prepared, setPrepared] = useState<PreparedImport | null>(null), [selection, setSelection] = useState<string[]>([]);
  const [result, setResult] = useState<ImportResult | null>(null), [undone, setUndone] = useState(false), [started, setStarted] = useState(false);
  const [busy, setBusy] = useState<"" | "choose" | "apply" | "undo" | "organize" | "close">("");
  const [error, setError] = useNotice(), [retryApply, setRetryApply] = useState(false);
  const alive = useRef(false), lock = useRef(false), staged = useRef<string | null>(null), applying = useRef(false);
  const attempt = useRef<{ id: string; fileIds: string[] } | null>(null), undoId = useRef(uid());
  const organization = useRef<ImportOrganization | null>(null);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (staged.current && !applying.current) void call("import_discard", { id: staged.current }).catch(() => {});
    };
  }, []);
  async function discard() {
    const id = staged.current;
    if (!id) return;
    await call("import_discard", { id });
    if (staged.current === id) staged.current = null;
  }
  async function choose() {
    if (lock.current) return;
    lock.current = true; setBusy("choose"); setError("");
    try {
      await discard();
      if (!alive.current) return;
      setPrepared(null); setSelection([]); attempt.current = null; setRetryApply(false);
      const value = await call<PreparedImport | null>("import_prepare");
      if (!alive.current) { if (value) void call("import_discard", { id: value.id }).catch(() => {}); return; }
      staged.current = value?.id || null;
      if (value) {
        setPrepared(value); setSelection(value.files.filter(file => !file.error).map(file => file.id));
        setResult(null); setUndone(false); setStarted(false); organization.current = null; undoId.current = uid();
      }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(""); }
  }
  async function apply() {
    if (lock.current || !prepared || (!attempt.current && !selection.length)) return;
    lock.current = true; applying.current = true; setBusy("apply"); setError("");
    const request = attempt.current || { id: prepared.id, fileIds: [...selection] };
    attempt.current = request;
    try {
      const value = await call<ImportResult>("import_apply", request);
      if (alive.current) { setResult(value); setUndone(value.receipt?.status === "undone"); setPrepared(null); setRetryApply(false); onChanged(); }
    } catch (e) { if (alive.current) { setError(errorText(e)); setRetryApply(true); } }
    finally {
      applying.current = false; lock.current = false;
      if (alive.current) setBusy("");
      else if (staged.current) void discard().catch(() => {});
    }
  }
  async function close() {
    if (lock.current) return;
    lock.current = true; setBusy("close"); setError("");
    try {
      await discard();
      if (alive.current) { setPrepared(null); setSelection([]); attempt.current = null; setRetryApply(false); onClose(); }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(""); }
  }
  async function undo() {
    if (lock.current || !result?.receipt || undone) return;
    lock.current = true; setBusy("undo"); setError("");
    try {
      await call<Receipt>("library_action", { action: { action: "undo", request_id: undoId.current, original_request: result.receipt.request_id } });
      if (alive.current) { setUndone(true); onChanged(); }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(""); }
  }
  async function organize() {
    if (lock.current || !result || undone || (!configured && !started) || !importedSources(result).length) return;
    lock.current = true; setBusy("organize"); setError("");
    organization.current ||= {
      id: uid(), topicId: uid(), title: t("import.discussionTitle"),
      text: importOrganizationInstruction(i18n.resolvedLanguage || i18n.language, result.items.filter(item => item.status === "imported").map(item => item.memory_id)),
      context: importedSources(result),
      memories: result.items.filter(item => item.status === "imported").map(({ memory_id, version_id }) => ({ memory_id, version_id })),
    };
    try {
      await discard();
      if (!alive.current) return;
      if (started) await onOpenDiscussion(organization.current.topicId);
      else { await onOrganize(organization.current); if (alive.current) setStarted(true); }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(""); }
  }
  async function openRecord(key: Key) {
    if (lock.current) return;
    lock.current = true; setBusy("close"); setError("");
    try { await discard(); if (alive.current) { onClose(); onOpenRecord(key); } }
    catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(""); }
  }
  if (!open) return null;
  const imported = result?.items.filter(item => item.status === "imported").length || 0;
  const duplicates = (result?.items.length || 0) - imported;
  const importedIds = new Set(result?.items.filter(item => item.status === "imported").map(item => item.memory_id) || []);
  return <Modal title={t("import.title")} className="import-dialog" onClose={() => void close()}>
    <div className="import-content">
      <p className="field-help">{t("import.help")}</p>
      <ErrorNotice text={error} />
      {!prepared && !result && <button data-modal-autofocus className="send-button" disabled={!!busy} onClick={() => void choose()}>{busy === "choose" ? t("import.choosing") : t("import.choose")}</button>}
      {prepared && <>
        <p className="import-count" role="status">{t("import.selected", { selected: selection.length, total: prepared.files.length })}</p>
        <div className="import-file-list">{prepared.files.map(file => <label className={`import-file${file.error ? " is-invalid" : ""}`} key={file.id}>
          <input type="checkbox" checked={selection.includes(file.id)} disabled={!!busy || !!file.error || retryApply} onChange={e => setSelection(items => e.target.checked ? [...items, file.id] : items.filter(id => id !== file.id))} />
          <span><strong>{file.file_name}</strong><span>{t("import.bytes", { count: file.bytes })}</span>{file.error && <span className="import-file-error">{translateCatalog(file.error, { ns: "errors" })}</span>}</span>
        </label>)}</div>
        <p className="field-help">{t("import.previewHelp")}</p>
        {retryApply && <p className="workspace-warning">{t("import.unconfirmed")}</p>}
        <div className="action-row"><button className="send-button" disabled={!!busy || !selection.length} onClick={() => void apply()}>{busy === "apply" ? t("import.importing") : retryApply ? t("import.retry") : t("import.confirm")}</button><button className="outline-button" disabled={!!busy || retryApply} onClick={() => void choose()}>{t("import.chooseAgain")}</button></div>
      </>}
      {result && <>
        <div className="import-summary" role="status"><strong>{undone ? t("import.undone") : t("import.complete", { imported, duplicates })}</strong><p>{undone ? t("import.undoneHelp") : imported ? t("import.savedHelp") : t("import.duplicatesHelp")}</p></div>
        <div className="import-file-list">{result.items.map((item, index) => <div className="import-result" key={`${item.memory_id}:${index}`}><div><strong>{item.file_name}</strong><span>{item.status === "duplicate" ? t("import.duplicate") : undone ? t("import.undoneItem") : t("import.imported")}</span></div><button className="quiet" disabled={!!busy || (undone && importedIds.has(item.memory_id))} onClick={() => void openRecord({ kind: "memory", id: item.memory_id })}>{t("import.view")}</button></div>)}</div>
        {!!imported && !undone && <div className="import-organize"><h3>{t("import.organizeTitle")}</h3><p className="field-help">{started ? t("import.startedHelp") : t("import.organizeHelp")}</p>{!configured && <p className="field-help">{t("import.noModel")}</p>}<button className="send-button" disabled={!!busy || (!configured && !started)} onClick={() => void organize()}>{busy === "organize" ? t("import.starting") : started ? t("import.openDiscussion") : organization.current ? t("import.retryOrganize") : t("import.organize")}</button></div>}
        <div className="action-row"><button className="outline-button" disabled={!!busy} onClick={() => void choose()}>{t("import.chooseMore")}</button>{result.receipt && !undone && !organization.current && <button className="quiet" disabled={!!busy} onClick={() => void undo()}>{busy === "undo" ? t("import.undoing") : t("import.undo")}</button>}</div>
      </>}
    </div>
    <div className="action-row import-close"><button className="quiet" disabled={!!busy} onClick={() => void close()}>{busy === "close" ? t("import.closing") : t("import.close")}</button></div>
  </Modal>;
}
