import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { call, errorText, uid, type Detail, type Receipt } from "./api";
import { flushDraft, refreshDrafts } from "./useDraft";
import Markdown from "./Markdown";
import MarkdownEditor from "./MarkdownEditor";
import { ErrorNotice } from "./components";
import { cleanupDiff } from "./cleanupDiff";

type Snapshot = { memory_id: string; expected_version: string; draft_request: string | null; title: string; body: string };
type SaveRequest = { request_id: string; snapshot: Snapshot; body: string };
export default function MemoryCleanup({ detail, toolbar, onClose, onSaved }: {
  detail: Detail; toolbar: HTMLElement; onClose: () => void; onSaved: (receipt: Receipt) => void;
}) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null), [body, setBody] = useState<string | null>(null);
  const [busy, setBusy] = useState(true), [saving, setSaving] = useState(false), [error, setError] = useState("");
  const [view, setView] = useState<"result" | "diff">("result"), [refine, setRefine] = useState(false), [instruction, setInstruction] = useState("");
  const job = useRef(""), alive = useRef(false), lock = useRef(false), saveId = useRef(uid());
  const snapshotRef = useRef<Snapshot | null>(null);
  const bodyRef = useRef<string | null>(null);
  const attemptedSave = useRef<SaveRequest | null>(null);
  const [retryingSave, setRetryingSave] = useState(false);
  const conflict = snapshot && (detail.current?.id !== snapshot.expected_version || detail.state !== "active");
  const unchanged = body === detail.body && snapshot?.body === detail.body && snapshot?.title === detail.title;
  const parts = useMemo(() => cleanupDiff(snapshot?.body ?? "", body ?? ""), [snapshot?.body, body]);
  useEffect(() => {
    const id = uid(); job.current = id; alive.current = true;
    let active = true;
    async function start() {
      try {
        await flushDraft(`memory:${detail.key.id}`);
        if (!active) return;
        const original = await call<Snapshot>("cleanup_prepare", { id, memory: detail.key.id, expected: detail.current!.id });
        if (!active) { void call("cleanup_cancel", { id }).catch(() => {}); return; }
        snapshotRef.current = original; setSnapshot(original);
        const result = await call<string>("cleanup_generate", { id, snapshot: original, previous: null, instruction: "" });
        if (active) { bodyRef.current = result; setBody(result); }
      } catch (e) { if (active) setError(errorText(e)); }
      finally { if (active) setBusy(false); }
    }
    void start();
    return () => { active = false; alive.current = false; void call("cleanup_cancel", { id }).catch(() => {}); };
    // The session intentionally keeps the original snapshot when the library refreshes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [detail.key.id]);
  async function generate() {
    const original = snapshotRef.current;
    if (!original || lock.current || conflict) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const result = await call<string>("cleanup_generate", { id: job.current, snapshot: original, previous: bodyRef.current, instruction });
      if (alive.current) { bodyRef.current = result; setBody(result); saveId.current = uid(); attemptedSave.current = null; setRetryingSave(false); setView("result"); setRefine(false); }
    } catch (e) { if (alive.current) setError(errorText(e) + (bodyRef.current !== null ? " 下方保留调整前的审核稿。" : "")); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  async function save() {
    const reviewedBody = bodyRef.current;
    if (!snapshot || !reviewedBody?.trim() || busy || lock.current) return;
    // A reply can be lost after commit and after library-refresh advances the head.
    // Only an identical, already-confirmed request may bypass the UI conflict gate;
    // the core replays its receipt before CAS, and still rejects any new stale write.
    const previousAttempt = attemptedSave.current;
    if (!previousAttempt && (conflict || (reviewedBody === detail.body && snapshot.body === detail.body && snapshot.title === detail.title))) return;
    const request = previousAttempt ?? { request_id: saveId.current, snapshot, body: reviewedBody };
    attemptedSave.current = request;
    lock.current = true; setSaving(true); setError("");
    try {
      const receipt = await call<Receipt>("cleanup_save", { request });
      // Core consumed the exact source draft atomically with the reviewed version.
      void refreshDrafts().catch(() => {});
      if (alive.current) onSaved(receipt);
    } catch (e) { if (alive.current) { setError(errorText(e)); setRetryingSave(true); } }
    finally { lock.current = false; if (alive.current) setSaving(false); }
  }
  return <div className="memory-cleanup">
    {createPortal(<>
      {body !== null && <button className="outline-button" disabled={busy || saving || !!conflict} onClick={() => setRefine(v => !v)}>再调整</button>}
      {body !== null && <button className="send-button" disabled={busy || saving || !body.trim() || (!retryingSave && (!!conflict || unchanged))} onClick={() => void save()}>{saving ? "保存中…" : retryingSave ? "重试保存" : "接受并保存"}</button>}
      <button className="quiet" disabled={saving} onClick={onClose}>{busy ? "取消整理" : "取消"}</button>
    </>, toolbar)}
    <div className="section-heading"><h2>整理正文</h2><span role="status">{busy ? "正在整理，原文仍保留…" : "审核后保存为新版本"}</span></div>
    <ErrorNotice text={error} />
    {snapshot && snapshot.title !== detail.title && <p className="field-help">标题沿用编辑草稿：{snapshot.title}</p>}
    {retryingSave && <p className="field-help">保存结果尚未确认，可重试同一请求；不会重复创建版本或覆盖之后的修改。</p>}
    {conflict && !retryingSave && <p className="workspace-warning">记忆已有新版本或已被删除。整理稿仍在下方，请复制需要的内容，再取消并重新核对。</p>}
    {refine && <form className="cleanup-refine" onSubmit={e => { e.preventDefault(); void generate(); }}>
      <input autoFocus aria-label="调整整理方式" placeholder="例如：保留口语风格，把步骤列清楚" maxLength={1000} value={instruction} disabled={busy || saving} onChange={e => setInstruction(e.target.value)} />
      <button className="outline-button" disabled={busy || saving || !instruction.trim() || !!conflict}>调整</button>
    </form>}
    {body === null ? <>
      {!busy && snapshot && <button className="outline-button" onClick={() => void generate()} disabled={!!conflict}>重试整理</button>}
      <Markdown text={snapshot?.body ?? detail.body} />
    </> : <>
      <div className="cleanup-view" role="group" aria-label="整理预览方式">
        <button aria-pressed={view === "result"} onClick={() => setView("result")}>整理结果</button>
        <button aria-pressed={view === "diff"} onClick={() => setView("diff")}>查看改动</button>
        {body === snapshot?.body && <span>正文没有变化</span>}
      </div>
      {view === "result" ? <MarkdownEditor label="审核整理正文" value={body} disabled={busy || saving} onChange={value => { if (value !== bodyRef.current) { attemptedSave.current = null; setRetryingSave(false); saveId.current = uid(); } bodyRef.current = value; setBody(value); }} onSave={() => void save()} /> :
        <div className="cleanup-diff" aria-label="正文改动，红色删除，蓝色新增">{parts.map((p, i) => p.kind === "remove" ? <del key={i}>{p.text}</del> : p.kind === "add" ? <ins key={i}>{p.text}</ins> : <span key={i}>{p.text}</span>)}</div>}
      <p className="field-help">可直接修改整理结果。接受后保存完整审核稿；原话、来源和历史保留。取消会放弃这次预览，原编辑草稿仍保留。</p>
    </>}
  </div>;
}
