import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNotice } from "../i18n/react";
import { call, errorText, uid, type Receipt, type Key, type Version } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useResourceVersion } from "./resources";
import Markdown from "./Markdown";
type Change = { receipt: Receipt; before: Version | null; after: Version | null };
export default function MemoryChanges({ inputId, receipts, onOpenRecord, onRefresh }: { inputId: string; receipts: Receipt[]; onOpenRecord: (key: Key) => void; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const [changes, setChanges] = useState<Change[] | null>(null), [conflicts, setConflicts] = useState<string[]>([]), [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const request = useRef(uid()), lock = useRef(false);
  const applied = receipts.filter(r => r.status === "applied");
  const count = new Set(applied.map(r => r.memory_id).filter(Boolean)).size;
  async function run(action: "changes" | "undo") {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError("");
    try {
      if (action === "changes") setChanges(await call<Change[]>("discussion_changes", { inputId }));
      else {
        const result = await call<{ receipt: Receipt | null; conflicts: string[] }>("discussion_undo", { inputId, requestId: request.current });
        setConflicts(result.conflicts); onRefresh();
      }
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  if (!receipts.length) return null;
  return <div className="discussion-receipt">
    <div className="mutation-receipt" role="status"><span>{applied.length ? t("input.updatedMemories", { count }) : t("input.changesUndone")}</span><button disabled={busy} onClick={() => void run("changes")}>{t("receipt.viewChanges")}</button>{!!applied.length && <button disabled={busy} onClick={() => void run("undo")}>{t("input.undoTurn")}</button>}</div>
    {!!conflicts.length && <p className="workspace-error" role="alert">{t("input.undoConflict")} {conflicts.map(id => <button key={id} className="quiet" onClick={() => onOpenRecord({ kind: "memory", id })}>{id}</button>)}</p>}
    <ErrorNotice text={error} />
    {changes && <Modal title={t("receipt.changeTitle")} onClose={() => setChanges(null)}>{changes.map((change, index) => <section key={`${change.receipt.request_id}:${index}`} className="discussion-change">
      <button className="quiet" disabled={!change.after && !change.before} onClick={() => { const id = change.after?.memory_id || change.before?.memory_id; if (id) onOpenRecord({ kind: "memory", id }); }}>{change.after?.title || change.before?.title || t("discussion.sourceDeleted")}</button>
      <div className="change-comparison"><section><h3>{t("receipt.before")}</h3>{change.before ? <Markdown text={change.before.body} /> : <p>{change.receipt.before_version ? t("discussion.sourceDeleted") : t("receipt.newMemory")}</p>}</section><section><h3>{t("receipt.after")}</h3>{change.after ? <Markdown text={change.after.body} /> : <p>{t("discussion.sourceDeleted")}</p>}</section></div>
    </section>)}</Modal>}
  </div>;
}

export function MemoryChangeHistory({ memoryId, onOpenRecord, onRefresh }: { memoryId: string; onOpenRecord: (key: Key) => void; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{ domain: "memory", entity: `memory:${memoryId}` }]);
  const [groups, setGroups] = useState<{ input_id: string; receipts: Receipt[] }[]>([]), [refresh, setRefresh] = useState(0), [error, setError] = useNotice();
  useEffect(() => {
    let active = true;
    void call<typeof groups>("library_agent_changes", { memoryId }).then(value => { if (active) setGroups(value); }).catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [memoryId, revision, refresh]);
  if (!groups.length && !error) return null;
  return <section className="memory-change-history"><h3>{t("input.memoryChanges")}</h3>
    {groups.map(group => <MemoryChanges key={group.input_id} inputId={group.input_id} receipts={group.receipts} onOpenRecord={onOpenRecord} onRefresh={() => { setRefresh(value => value + 1); onRefresh(); }} />)}
    <ErrorNotice text={error} />
  </section>;
}
