import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import { useResourceVersion } from "./resources";
import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type AgentUndoResult, type Key, type MemoryReceipt, type Receipt, type Version } from "./api";
import { Icon } from "../ui";
import { ErrorNotice, Modal } from "./components";
import CollectionChanges from "./CollectionChanges";
import { UndoConflicts } from "./MemoryChanges";

type Change = { receipt: Receipt; before: Version | null; after: Version | null };
export default function MemoryReceipts({ record, revision: requestedRevision = 0, onOpen, onRefresh, presentation = "history", currentVersion, excludedReceipt }: {
  presentation?: "history" | "summary"; currentVersion?: string; excludedReceipt?: string;
  record: Key; revision?: number; onOpen: (key: Key) => void; onRefresh: () => void;
}) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{ domain: "memory", entity: `${record.kind}:${record.id}` }, { domain: "collection" }]) + requestedRevision;
  const [expanded, setExpanded] = useState(false), [error, setError] = useNotice();
  const recordKey = `${record.kind}:${record.id}`;
  const identity = useRef({ key: recordKey, owner: uid() });
  if (identity.current.key !== recordKey) identity.current = { key: recordKey, owner: uid() };
  const owner = identity.current.owner;
  const currentOwner = useRef<string>(owner); currentOwner.current = owner;
  const [result, setResult] = useState<{ owner: string; rows: MemoryReceipt[] } | null>(null);
  const rows = result?.owner === owner ? result.rows : [];
  const [busy, setBusy] = useState(false), [changes, setChanges] = useState<Change[] | null>(null);
  const [conflicts, setConflicts] = useState<AgentUndoResult | null>(null);
  const [comparisonReceipt, setComparisonReceipt] = useState<MemoryReceipt | null>(null);
  const locked = useRef(false), request = useRef({ action: "", id: uid() });
  useEffect(() => {
    currentOwner.current = owner;
    setResult(null); setError(""); setChanges(null); setConflicts(null); setExpanded(false); setBusy(false); locked.current = false;
    return () => { if (currentOwner.current === owner) currentOwner.current = ""; };
  }, [record.id, record.kind]);
  useEffect(() => {
    let active = true;
    void call<MemoryReceipt[]>("memory_receipts", { key: record }).then(value => {
      if (active) { setResult({ owner, rows: value }); setError(""); }
    }).catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [record.id, record.kind, revision]);
  async function run(receipt: MemoryReceipt, action: "undo" | "changes") {
    if (locked.current || currentOwner.current !== owner || !rows.includes(receipt)) return;
    locked.current = true; setBusy(true); setError(""); setConflicts(null);
    const actionKey = `${action === "undo" ? receipt.logical_input_id ?? receipt.request_id : receipt.request_id}:${action}`;
    if (request.current.action !== actionKey) request.current = { action: actionKey, id: uid() };
    try {
      if (action === "changes") {
        const value = await call<Change[]>("memory_receipt_changes", { request: receipt.request_id });
        if (currentOwner.current === owner) { setChanges(value); setComparisonReceipt(receipt); }
      } else {
        if (receipt.logical_input_id) {
          const value = await call<AgentUndoResult>("discussion_undo", { inputId: receipt.logical_input_id, requestId: request.current.id });
          if (value.conflicts.length || value.collection_conflicts?.length) {
            if (currentOwner.current === owner) setConflicts(value);
            return;
          }
        } else {
          await call("library_action", { action: { action: "undo", request_id: request.current.id, original_request: receipt.request_id } });
        }
        if (currentOwner.current === owner) { onRefresh(); onOpen(record); }
      }
    } catch (e) { if (currentOwner.current === owner) setError(errorText(e)); }
    finally { if (currentOwner.current === owner) { locked.current = false; setBusy(false); } }
  }
  return <>
    <ErrorNotice text={error} />
    <UndoConflicts result={conflicts} receipts={rows} onOpenRecord={onOpen} />
    {(expanded ? rows : rows.slice(0, 1)).map(receipt => {
      const applied = receipt.status === "applied";
      const collectionOnly = !receipt.memory_id && !!receipt.collection_changes?.length;
      const collectionCount = new Set(receipt.collection_changes?.map(change => change.collection_id)).size;
      const summary = collectionOnly ? t("collectionChanges.updatedCollections", { count: collectionCount }) : t(receipt.action === "merge" ? "receipt.summaryMerge" : "receipt.summaryUpdated");
      if (presentation === "summary") {
        if (!applied || (!collectionOnly && receipt.after_version !== currentVersion) || receipt.request_id === excludedReceipt) return null;
        return <div className="memory-change-receipt" role="status" key={receipt.request_id}>
          <Icon name="check" size={15} /><span>{summary}</span>
          <button disabled={busy} onClick={() => void run(receipt, "changes")}>{t("receipt.viewChanges")}</button>
          <button disabled={busy} onClick={() => void run(receipt, "undo")}>{t(receipt.logical_input_id ? "input.undoTurn" : "receipt.undo")}</button>
        </div>;
      }
      return <div className="memory-history-entry" role="status" key={receipt.request_id}>
        <p>{applied ? summary : t(receipt.status === "undone" ? (receipt.collection_changes?.length ? "input.changesUndone" : "receipt.statusUndone") : "collectionChanges.notApplied")}</p>
        {receipt.reason && <p>{receipt.reason}</p>}
        <div className="receipt-actions">
          {receipt.memory_id && <button onClick={() => onOpen({ kind: "memory", id: receipt.memory_id! })}>{t("receipt.viewMemory")}</button>}
          <button disabled={busy} onClick={() => void run(receipt, "changes")}>{t("receipt.viewChanges")}</button>
          {applied && <button disabled={busy} onClick={() => void run(receipt, "undo")}>{t(receipt.logical_input_id ? "input.undoTurn" : "receipt.undo")}</button>}
        </div>
      </div>;
    })}
    {presentation === "history" && rows.length > 1 && <button className="quiet" onClick={() => setExpanded(v => !v)}>{expanded ? t("receipt.collapseHistory") : t("receipt.expandHistory", { count: rows.length - 1 })}</button>}
    {changes && <Modal title={t(comparisonReceipt?.collection_changes?.length ? "collectionChanges.changeTitle" : "receipt.changeTitle")} onClose={() => setChanges(null)}>{(rows.find(receipt => receipt.request_id === comparisonReceipt?.request_id)?.status ?? comparisonReceipt?.status) === "undone" && <p className="field-help" role="status">{t("collectionChanges.undoneHelp")}</p>}{!!comparisonReceipt?.collection_changes?.length && <CollectionChanges changes={comparisonReceipt.collection_changes} onOpenRecord={onOpen} />}{changes.filter(change => change.before || change.after || change.receipt.memory_id).map((change, index) => <div className="change-comparison" key={change.after?.id ?? index}><section><h3>{t("receipt.before")}</h3><p className="readable-text">{change.before?.body ?? t(change.receipt.before_version ? "receipt.versionUnavailable" : "receipt.newMemory")}</p></section><section><h3>{t("receipt.after")}</h3><p className="readable-text">{change.after?.body ?? t("receipt.versionUnavailable")}</p></section></div>)}</Modal>}
  </>;
}
