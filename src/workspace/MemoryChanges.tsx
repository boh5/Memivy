import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNotice } from "../i18n/react";
import { call, errorText, uid, type Receipt, type Key, type Version, type AgentChangeGroup, type AgentUndoResult, type CollectionChange } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useResourceVersion } from "./resources";
import Markdown from "./Markdown";
import CollectionChanges from "./CollectionChanges";

type Change = { receipt: Receipt; before: Version | null; after: Version | null };
type Comparison = { memories: Change[]; collections: CollectionChange[] };

export function UndoConflicts({ result, receipts, onOpenRecord }: { result: AgentUndoResult | null; receipts: Receipt[]; onOpenRecord: (key: Key) => void }) {
  const { t } = useTranslation("workspace");
  if (!result || (!result.conflicts.length && !result.collection_conflicts?.length)) return null;
  const names = new Map(receipts.flatMap(receipt => (receipt.collection_changes || []).map(change => [change.collection_id, change.after.name] as const)));
  return <div className="workspace-error undo-conflicts" role="alert">
    <p>{t(result.collection_conflicts?.length ? "collectionChanges.undoConflict" : "input.undoConflict")}</p>
    <ul>{result.conflicts.map(id => <li key={id}><button className="quiet" onClick={() => onOpenRecord({ kind: "memory", id })}>{id}</button></li>)}
      {result.collection_conflicts?.map(id => <li key={id}>{names.get(id) || t("collectionChanges.changedCollection")}</li>)}
    </ul>
  </div>;
}

export default function MemoryChanges({ inputId, receipts, incomplete = false, onOpenRecord, onRefresh }: { inputId: string; receipts: Receipt[]; incomplete?: boolean; onOpenRecord: (key: Key) => void; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const [changes, setChanges] = useState<Comparison | null>(null), [undo, setUndo] = useState<AgentUndoResult | null>(null), [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const request = useRef(uid()), lock = useRef(false), owner = useRef(inputId);
  const applied = receipts.filter(receipt => receipt.status === "applied");
  const memoryCount = new Set(applied.map(receipt => receipt.memory_id).filter(Boolean)).size;
  const collectionCount = new Set(applied.flatMap(receipt => (receipt.collection_changes || []).map(change => change.collection_id))).size;
  const hasCollections = receipts.some(receipt => receipt.collection_changes?.length);
  useEffect(() => {
    owner.current = inputId;
    return () => { owner.current = ""; };
  }, [inputId]);
  async function run(action: "changes" | "undo") {
    if (lock.current || owner.current !== inputId) return;
    lock.current = true; setBusy(true); setError(""); setUndo(null);
    try {
      if (action === "changes") {
        const memories = await call<Change[]>("discussion_changes", { inputId });
        if (owner.current === inputId) setChanges({ memories: memories.filter(change => change.before || change.after || change.receipt.memory_id), collections: receipts.flatMap(receipt => receipt.collection_changes || []) });
      } else {
        const result = await call<AgentUndoResult>("discussion_undo", { inputId, requestId: request.current });
        if (owner.current === inputId) { setUndo(result); onRefresh(); }
      }
    } catch (e) { if (owner.current === inputId) setError(errorText(e)); }
    finally { if (owner.current === inputId) { lock.current = false; setBusy(false); } }
  }
  if (!receipts.length) return null;
  return <div className="discussion-receipt">
    <div className="mutation-receipt" role="status">
      {!!memoryCount && <span>{t("input.updatedMemories", { count: memoryCount })}</span>}
      {!!collectionCount && <span>{t("collectionChanges.updatedCollections", { count: collectionCount })}</span>}
      {!applied.length && <span>{t(receipts.every(receipt => receipt.status === "undone") ? "input.changesUndone" : "collectionChanges.notApplied")}</span>}
      <button disabled={busy} onClick={() => void run("changes")}>{t("receipt.viewChanges")}</button>
      {!!applied.length && <button disabled={busy} onClick={() => void run("undo")}>{t("input.undoTurn")}</button>}
    </div>
    {incomplete && !!applied.length && <p className="field-help receipt-partial">{t("collectionChanges.partial")}</p>}
    <UndoConflicts result={undo} receipts={receipts} onOpenRecord={onOpenRecord} />
    <ErrorNotice text={error} />
    {changes && <Modal title={t(hasCollections ? "collectionChanges.changeTitle" : "receipt.changeTitle")} onClose={() => setChanges(null)}>
      {receipts.every(receipt => receipt.status === "undone") && <p className="field-help" role="status">{t("collectionChanges.undoneHelp")}</p>}
      {!!changes.collections.length && <CollectionChanges changes={changes.collections} onOpenRecord={onOpenRecord} />}
      {changes.memories.map((change, index) => <section key={`${change.receipt.request_id}:${index}`} className="discussion-change">
        <button className="quiet" disabled={!change.after && !change.before} onClick={() => { const id = change.after?.memory_id || change.before?.memory_id; if (id) onOpenRecord({ kind: "memory", id }); }}>{change.after?.title || change.before?.title || t("discussion.sourceDeleted")}</button>
        <div className="change-comparison"><section><h3>{t("receipt.before")}</h3>{change.before ? <Markdown text={change.before.body} /> : <p>{change.receipt.before_version ? t("discussion.sourceDeleted") : t("receipt.newMemory")}</p>}</section><section><h3>{t("receipt.after")}</h3>{change.after ? <Markdown text={change.after.body} /> : <p>{t("discussion.sourceDeleted")}</p>}</section></div>
      </section>)}
    </Modal>}
  </div>;
}

export function MemoryChangeHistory({ memoryId, onOpenRecord, onRefresh }: { memoryId: string; onOpenRecord: (key: Key) => void; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{ domain: "memory", entity: `memory:${memoryId}` }, { domain: "collection" }]);
  const [groups, setGroups] = useState<AgentChangeGroup[]>([]), [refresh, setRefresh] = useState(0), [error, setError] = useNotice();
  useEffect(() => {
    let active = true;
    void call<AgentChangeGroup[]>("library_agent_changes", { memoryId }).then(value => { if (active) setGroups(value); }).catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [memoryId, revision, refresh]);
  if (!groups.length && !error) return null;
  return <section className="memory-change-history"><h3>{t("input.memoryChanges")}</h3>
    {groups.map(group => <MemoryChanges key={group.input_id} inputId={group.input_id} receipts={group.receipts} onOpenRecord={onOpenRecord} onRefresh={() => { setRefresh(value => value + 1); onRefresh(); }} />)}
    <ErrorNotice text={error} />
  </section>;
}

export function CollectionChangeHistory({ collectionId, onOpenRecord, onRefresh }: { collectionId: string; onOpenRecord: (key: Key) => void; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{ domain: "collection", entity: collectionId }, { domain: "memory" }]);
  const [result, setResult] = useState<{ id: string; groups: AgentChangeGroup[] } | null>(null), [refresh, setRefresh] = useState(0), [error, setError] = useNotice();
  const groups = result?.id === collectionId ? result.groups : null;
  useEffect(() => {
    let active = true;
    setError("");
    void call<AgentChangeGroup[]>("collection_agent_changes", { collectionId }).then(value => { if (active) setResult({ id: collectionId, groups: value }); }).catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [collectionId, revision, refresh]);
  return <section className="collection-change-history">
    <p className="field-help">{t("collectionChanges.historyHelp")}</p>
    {groups === null && !error && <p className="field-help" role="status">{t("collectionChanges.loading")}</p>}
    {groups?.length === 0 && <p className="collection-history-empty">{t("collectionChanges.empty")}</p>}
    {groups?.map(group => <article key={group.input_id} className="collection-history-entry">
      {group.receipts.every(receipt => receipt.status === "undone") && <p className="field-help" role="status">{t("collectionChanges.undoneHelp")}</p>}
      <CollectionChanges changes={group.receipts.flatMap(receipt => receipt.collection_changes || [])} onOpenRecord={onOpenRecord} />
      <MemoryChanges inputId={group.input_id} receipts={group.receipts} onOpenRecord={onOpenRecord} onRefresh={() => { setRefresh(value => value + 1); onRefresh(); }} />
    </article>)}
    <ErrorNotice text={error} />
    {error && <button className="quiet" onClick={() => setRefresh(value => value + 1)}>{t("detail.retryEvidence")}</button>}
  </section>;
}
