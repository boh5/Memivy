import { useResourceVersion } from "./resources";
import IconButton from "./IconButton";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import { call, errorText, type Collection, type Key, type RecordNavigation as Navigation } from "./api";
import { ErrorNotice, Modal } from "./components";
import CollectionEditor from "./CollectionEditor";

export default function RecordNavigation({ record, revision: requestedRevision = 0, onChanged }: { record: Key; revision?: number; onChanged: () => void }) {
  const revision = useResourceVersion([{domain:"navigation",entity:`${record.kind}:${record.id}`},{domain:"collection"},{domain:"memory",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [value, setValue] = useState<Navigation | null>(null), [error, setError] = useState("");
  const [open, setOpen] = useState(false), [creating, setCreating] = useState(false);
  const [collections, setCollections] = useState<Collection[]>([]), [busy, setBusy] = useState(false);
  const lock = useRef(false);
  useEffect(() => {
    let alive = true;
    void call<Navigation>("navigation_record", { key: record }).then(v => { if (alive) { setValue(v); setError(""); } }).catch(e => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [record.id, record.kind, revision]);
  useEffect(() => {
    if (!open) return;
    let alive = true;
    void call<Collection[]>("navigation_collections").then(v => { if (alive) setCollections(v); }).catch(e => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [open, revision]);
  async function update(collection?: string, included?: boolean) {
    if (!value || lock.current) return;
    lock.current = true; setBusy(true); setError("");
    try {
      if (collection) {
        await call("navigation_collect", { collection, key: record, included });
        setValue(v => v && { ...v, collections: included ? [...new Set([...v.collections, collection])] : v.collections.filter(id => id !== collection) });
      } else {
        await call("navigation_pin", { key: record, pinned: !value.pinned });
        setValue(v => v && { ...v, pinned: !value.pinned });
      }
      onChanged();
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  return <>
    <IconButton label="管理记忆所属专题" icon="folder" disabled={busy || !value} onClick={() => { setOpen(true); setError(""); }} />
    <IconButton icon="pin" label={value?.pinned ? "取消置顶记忆" : "置顶记忆"} className={`record-pin ${value?.pinned ? "is-pinned" : ""}`} aria-pressed={value?.pinned || false} disabled={busy || !value} onClick={() => void update()} />
    {error && !open && <span className="navigation-inline-error" role="alert">{error}<button onClick={onChanged}>重试</button></span>}
    {open && <Modal title="加入专题" onClose={() => { if (!lock.current) setOpen(false); }} className="collection-dialog">
      <p className="field-help">一条记忆可以放进多个专题。移出专题会保留记忆。</p>
      <div className="collection-choices">{collections.map(c => <label key={c.id}><input type="checkbox" checked={value?.collections.includes(c.id) || false} disabled={busy} onChange={e => void update(c.id, e.target.checked)} /><span>{c.name}</span><small>{c.count} 条</small></label>)}</div>
      {!collections.length && <p className="field-help">还没有专题，先为正在关注的事建一个。</p>}
      <ErrorNotice text={error} />
      <div className="action-row"><button className="outline-button" disabled={busy} onClick={() => setCreating(true)}><Icon name="plus" size={14} />新建专题</button><button className="send-button" disabled={busy} onClick={() => setOpen(false)}>完成</button></div>
      {creating && <CollectionEditor onClose={() => setCreating(false)} onSaved={id => { setCreating(false); void update(id, true); }} />}
    </Modal>}
  </>;
}
