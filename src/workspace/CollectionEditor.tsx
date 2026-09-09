import { useRef, useState } from "react";
import { call, errorText, uid, type Collection } from "./api";
import { ErrorNotice, Modal } from "./components";

export default function CollectionEditor({ value, onSaved, onClose }: {
  value?: Collection; onSaved: (id: string) => void; onClose: () => void;
}) {
  const [name, setName] = useState(value?.name || "");
  const [description, setDescription] = useState(value?.description || "");
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const id = useRef(value?.id || uid()), lock = useRef(false);
  async function save() {
    if (lock.current || !name.trim()) return;
    lock.current = true; setBusy(true); setError("");
    try {
      await call("navigation_save_collection", { id: id.current, name: name.trim(), description: description.trim(), expected: value?.revision ?? null });
      onSaved(id.current);
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  return <Modal title={value ? "编辑专题" : "新建专题"} onClose={() => { if (!lock.current) onClose(); }} className="collection-dialog">
    <p className="field-help">把围绕同一件事的记忆放在一起，记录时不用分类。</p>
    <label>专题名称<input autoFocus aria-label="专题名称" maxLength={80} value={name} disabled={busy} placeholder="例如：面试准备" onChange={e => setName(e.target.value)} /></label>
    <label>想关注什么<textarea aria-label="专题说明" maxLength={800} value={description} disabled={busy} placeholder="写一点背景，AI 推荐时会参考这里。可留空。" onChange={e => setDescription(e.target.value)} /></label>
    <ErrorNotice text={error} />
    <div className="action-row"><button className="outline-button" disabled={busy} onClick={onClose}>取消</button><button className="send-button" disabled={busy || !name.trim()} onClick={() => void save()}>{busy ? "保存中…" : value ? "保存" : "创建专题"}</button></div>
  </Modal>;
}
