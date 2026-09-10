import { useState } from "react";
import { call, errorText, type Draft } from "./api";
import { DRAFT_CONFLICT } from "./draftQueue";
import { ErrorNotice, Modal } from "./components";
import type { useDraft } from "./useDraft";

export default function DraftConflict({ draft }: { draft: ReturnType<typeof useDraft> }) {
  const [other, setOther] = useState<{ value: Draft | null } | null>(null);
  const [error, setError] = useState("");
  async function review() {
    try { setOther({ value: await call<Draft | null>("draft_read", { key: draft.value.key }) }); setError(""); }
    catch (e) { setError(errorText(e)); }
  }
  async function resolve(local: boolean) {
    try { await draft.resolve(local, other?.value?.request_id || null); setOther(null); }
    catch (e) { await review(); setError(errorText(e)); }
  }
  return <>
    {draft.error === DRAFT_CONFLICT && <button className="outline-button" onClick={() => void review()}>核对两份草稿</button>}
    {other && <Modal title="选择继续编辑的草稿" onClose={() => setOther(null)}>
      <p>请先复制需要合并的文字。确认后会以选中的内容继续编辑。</p>
      {[{ label: "这个窗口", value: draft.value }, { label: "另一窗口", value: other.value }].map(({ label, value }) => <div key={label}>
        {value?.conclusion && <p className="field-help">{label} · {value.conclusion.destination.kind === "new" ? "新建记忆" : "已有记忆"} · {value.title}</p>}
        <label className="discussion-field">{label}<textarea rows={4} readOnly value={value?.body || ""} /></label>
        {value?.conclusion?.merged_body != null && <label className="discussion-field">{label}的完整融合稿<textarea rows={6} readOnly value={value.conclusion.merged_body} /></label>}
      </div>)}
      <ErrorNotice text={error} />
      <div className="action-row">
        <button className="outline-button" onClick={() => void resolve(false)}>使用另一窗口的草稿</button>
        <button className="send-button" onClick={() => void resolve(true)}>使用这个窗口的草稿</button>
      </div>
    </Modal>}
  </>;
}
