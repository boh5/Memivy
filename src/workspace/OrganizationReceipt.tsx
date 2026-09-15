import { translateCatalog } from "../i18n";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import { useResourceVersion } from "./resources";
import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type Detail, type Key, type Receipt } from "./api";
import { Icon } from "../ui";
import { ErrorNotice, Modal } from "./components";



type Job = { can_retry: boolean; memory_id: string; capture_id: string; attempt_id: string; status: string; reason: string; reason_code?: string | null; receipt: Receipt | null };
export default function OrganizationReceipt({ record, revision: requestedRevision = 0, onOpen, onRefresh, presentation = "history", currentVersion, excludedReceipt }: {
  presentation?: "history" | "status" | "summary";
  currentVersion?: string; excludedReceipt?: string;
  record: Key; revision?: number; onOpen: (key: Key) => void; onRefresh: () => void;
}) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{domain:"organization",entity:`${record.kind}:${record.id}`},{domain:"memory",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [dismissed, setDismissed] = useState("");
  const [expanded, setExpanded] = useState(false);
  const recordKey = `${record.kind}:${record.id}`;
  const identity = useRef({ key: recordKey, owner: uid() });
  if (identity.current.key !== recordKey) identity.current = { key: recordKey, owner: uid() };
  const owner = identity.current.owner;
  const currentOwner = useRef<string>(owner); currentOwner.current = owner;
  const [result, setResult] = useState<{ owner: string; jobs: Job[] } | null>(null), [error, setError] = useNotice();
  const jobs = result?.owner === owner ? result.jobs : [];
  const [busy, setBusy] = useState(false), [change, setChange] = useState<{ before: string | null; after: string } | null>(null);
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
        if (!after || (r.before_version && !before)) {
          setError({ ns: "workspace", key: "receipt.versionUnavailable" });
          return;
        }
        setChange({ before: before?.body ?? null, after: after.body });
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
      const reason = job.reason_code ? translateCatalog(job.reason_code, { ns: "errors" }) : job.reason;
      const reasonSuffix = reason ? ` ${reason}` : "";
      const label = undone ? t("receipt.statusUndone") : job.status === "pending" ? t("receipt.statusPending") : job.status === "processing" ? t("receipt.statusProcessing") : job.status === "deferred" ? t("receipt.statusDeferred", { reason: reasonSuffix }) : job.status === "failed" ? t("receipt.statusFailed", { reason: reasonSuffix }) : job.status === "paused" ? t("receipt.statusPaused", { reason: reasonSuffix }) : t(receipt?.action === "merge" ? "receipt.statusMerge" : "receipt.statusSeparate", { reason: reasonSuffix });
      if (presentation === "summary" && applied) {
        if (receipt?.after_version !== currentVersion || receipt.request_id === excludedReceipt) return null;
        return <div className="memory-change-receipt" role="status" key={job.attempt_id}>
          <Icon name="check" size={15} /><span>{t(receipt.action === "merge" ? "receipt.summaryMerge" : "receipt.summaryOrganized")}</span>
          <button disabled={busy} onClick={() => void run(job, "changes")}>{t("receipt.viewChanges")}</button>
          <button disabled={busy} onClick={() => void run(job, "undo")}>{t("receipt.undo")}</button>
        </div>;
      }
      if (presentation === "status" || presentation === "summary") {
        if (job !== jobs[0] || applied || undone || dismissed === job.attempt_id) return null;
        const processing = job.status === "processing" || job.status === "pending";
        return <div className="organization-inline-state" role="status" key={job.attempt_id}>
          <span>{job.status === "pending" ? t("receipt.waiting") : processing ? t("receipt.organizing") : t("receipt.incomplete")}</span>
          {!processing && <>{job.can_retry && <button className="quiet" disabled={busy} onClick={() => void run(job,"retry")}>{t("receipt.retry")}</button>}<button className="quiet" onClick={() => setDismissed(job.attempt_id)} aria-label={t("receipt.collapseAria")}>{t("receipt.collapse")}</button></>}
        </div>;
      }
      return <div className="organization-history-entry" role="status" key={job.capture_id}>
        <p>{label}</p><div className="receipt-actions">
          {receipt?.memory_id && !undone && <button onClick={() => onOpen({ kind: "memory", id: receipt.memory_id! })}>{t("receipt.viewMemory")}</button>}
          {applied && <><button disabled={busy} onClick={() => void run(job, "changes")}>{t("receipt.viewChanges")}</button><button disabled={busy} onClick={() => void run(job, "undo")}>{t("receipt.undo")}</button></>}
          {!applied && job.can_retry && <button disabled={busy} onClick={() => void run(job, "retry")}>{t("receipt.reorganize")}</button>}
        </div>
      </div>;
    })}
    {presentation === "history" && jobs.length > 1 && <button className="quiet" onClick={() => setExpanded(v => !v)}>{expanded ? t("receipt.collapseHistory") : t("receipt.expandHistory", { count: jobs.length - 1 })}</button>}
    {change && <Modal title={t("receipt.changeTitle")} onClose={() => setChange(null)}><div className="change-comparison"><section><h3>{t("receipt.before")}</h3><p className="readable-text">{change.before ?? t("receipt.newMemory")}</p></section><section><h3>{t("receipt.after")}</h3><p className="readable-text">{change.after}</p></section></div></Modal>}
  </>;
}
