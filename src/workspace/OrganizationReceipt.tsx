import { useResourceVersion } from "./resources";
import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type Detail, type Key, type Receipt } from "./api";
import { ErrorNotice, Modal } from "./components";



type Job = { can_retry: boolean; memory_id: string; capture_id: string; attempt_id: string; status: string; reason: string; receipt: Receipt | null };
export default function OrganizationReceipt({ record, revision: requestedRevision = 0, onOpen, onRefresh, presentation = "history" }: {
  presentation?: "history" | "status";
  record: Key; revision?: number; onOpen: (key: Key) => void; onRefresh: () => void;
}) {
  const revision = useResourceVersion([{domain:"organization",entity:`${record.kind}:${record.id}`},{domain:"memory",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [dismissed, setDismissed] = useState("");
  const [expanded, setExpanded] = useState(false);
  const recordKey = `${record.kind}:${record.id}`;
  const identity = useRef({ key: recordKey, owner: uid() });
  if (identity.current.key !== recordKey) identity.current = { key: recordKey, owner: uid() };
  const owner = identity.current.owner;
  const currentOwner = useRef<string>(owner); currentOwner.current = owner;
  const [result, setResult] = useState<{ owner: string; jobs: Job[] } | null>(null), [error, setError] = useState("");
  const jobs = result?.owner === owner ? result.jobs : [];
  const [busy, setBusy] = useState(false), [change, setChange] = useState<{ before: string; after: string } | null>(null);
  const locked = useRef(false), request = useRef({ action: "", id: uid() });
  useEffect(() => {
    currentOwner.current = owner;
    setResult(null); setError(""); setChange(null); setExpanded(false); setBusy(false); locked.current = false;
    return () => { if (currentOwner.current === owner) currentOwner.current = ""; };
  }, [record.id, record.kind]);
  useEffect(() => {
    let active = true;
    // Revisions refresh server data without dismissing immutable comparisons or
    // invalidating a write that is still completing for this same record.
    void call<Job[]>("organization_jobs", { key: record }).then(rows => {
      if (active) { setResult({ owner, jobs: rows }); setError(""); }
    }).catch(e => {
      if (active) { setError(errorText(e)); }
    });
    return () => { active = false; };
  }, [record.id, record.kind, revision]);
  async function run(job: Job, action: "undo" | "retry" | "changes") {
    if (locked.current || currentOwner.current !== owner || result?.owner !== owner || !jobs.includes(job)) return;
    locked.current = true; setBusy(true); setError("");
    const actionKey = `${job.attempt_id}:${action}`;
    if (request.current.action !== actionKey) request.current = { action: actionKey, id: uid() };
    try {
      const r = job.receipt;
      if (action === "changes" && r?.memory_id) {
        const detail = await call<Detail>("library_detail", { key: { kind: "memory", id: r.memory_id } });
        if (currentOwner.current !== owner) return;
        const after = detail.history.find(v => v.id === r.after_version);
        const before = detail.history.find(v => v.id === r.before_version);
        if (!after || (r.before_version && !before)) throw "这个版本已不可用。";
        setChange({ before: before?.body || "（新建记忆）", after: after.body });
      } else if (action === "undo" && r) {
        await call("library_action", { action: { action: "undo", request_id: request.current.id, original_request: r.request_id } });
        if (currentOwner.current !== owner) return;
        onRefresh(); onOpen({ kind: "memory", id: job.memory_id });
      } else if (action === "retry") {
        await call("organization_retry", { memoryId: job.memory_id }); if (currentOwner.current === owner) onRefresh();

      }
    } catch (e) { if (currentOwner.current === owner) setError(errorText(e)); }
    finally { if (currentOwner.current === owner) { locked.current = false; setBusy(false); } }
  }
  return <>
    <ErrorNotice text={error} />
    {(expanded ? jobs : jobs.slice(0, 1)).map(job => {
      const receipt = job.receipt, undone = receipt?.status === "undone";
      const applied = receipt?.status === "applied";
      const label = undone ? "已撤销整理，记忆已恢复。" : job.status === "pending" ? "已保存，等待后台整理；现在即可编辑和检索。" : job.status === "processing" ? "已保存，正在整理…" : job.status === "deferred" ? `已保存，待核对。${job.reason}` : job.status === "failed" ? `记忆已保留。${job.reason}` : job.status === "paused" ? `自动整理已停止。${job.reason}` : `${receipt?.action === "merge" ? "已归入已有记忆" : "已整理当前记忆"}。${job.reason}`;
      if (presentation === "status") {
        if (job !== jobs[0] || applied || undone || dismissed === job.attempt_id) return null;
        const processing = job.status === "processing" || job.status === "pending";
        return <div className="organization-inline-state" role="status" key={job.attempt_id}>
          <span>{job.status === "pending" ? "等待整理" : processing ? "整理中…" : "已保存，整理未完成"}</span>
          {!processing && <>{job.can_retry && <button className="quiet" disabled={busy} onClick={() => void run(job,"retry")}>重试</button>}<button className="quiet" onClick={() => setDismissed(job.attempt_id)} aria-label="收起整理状态">收起</button></>}
        </div>;
      }
      return <div className="organization-history-entry" role="status" key={job.capture_id}>
        <p>{label}</p><div className="receipt-actions">
          {receipt?.memory_id && !undone && <button onClick={() => onOpen({ kind: "memory", id: receipt.memory_id! })}>查看记忆</button>}
          {applied && <><button disabled={busy} onClick={() => void run(job, "changes")}>查看变化</button><button disabled={busy} onClick={() => void run(job, "undo")}>撤销整理</button></>}
          {!applied && job.can_retry && <button disabled={busy} onClick={() => void run(job, "retry")}>重新整理</button>}
        </div>
      </div>;
    })}
    {presentation === "history" && jobs.length > 1 && <button className="quiet" onClick={() => setExpanded(v => !v)}>{expanded ? "收起更早回执" : `更早的整理回执（${jobs.length - 1}）`}</button>}
    {change && <Modal title="这次整理的变化" onClose={() => setChange(null)}><div className="change-comparison"><section><h3>修改前</h3><p className="readable-text">{change.before}</p></section><section><h3>修改后</h3><p className="readable-text">{change.after}</p></section></div></Modal>}
  </>;
}
