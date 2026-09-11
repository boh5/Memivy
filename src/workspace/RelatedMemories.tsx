import { useResourceVersion } from "./resources";
import { useEffect, useRef, useState } from "react";
import { call, errorText, unavailable, type Key, type Source } from "./api";
import { ErrorNotice } from "./components";

type Related = { memory_id: string; version_id: string; title: string; snippet: string; source: Source };
export default function RelatedMemories({ memoryId, versionId, revision: requestedRevision = 0, onOpen, onDiscuss, collectionId, paused = false }: {
  collectionId?: string; paused?: boolean;
  memoryId: string; versionId: string; revision?: number;
  onOpen: (key: Key) => void; onDiscuss: (sources: Source[]) => Promise<void>;
}) {
  const revision = useResourceVersion([{domain:"memory"},{domain:"collection"},{domain:"navigation"}]) + requestedRevision;
  const [rows, setRows] = useState<Related[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const chosen = useRef<Related[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const owner = JSON.stringify([memoryId, versionId, collectionId]);
  const previousOwner = useRef(owner);
  useEffect(() => {
    let active = true;
    // Background refreshes retain visible rows and the user's selection.
    // A different memory/version/scope must never inherit those results.
    if (previousOwner.current !== owner) {
      setRows([]); setSelected([]); chosen.current=[]; previousOwner.current = owner;
    }
    setError("");
    if (paused) return;
    // Collapse rapid refresh events into one bounded local query.
    const timer = setTimeout(() => {
      void call<Related[]>("memory_related", { memoryId, expectedVersion: versionId, collectionId })
        .then(async result => {
          const checked = [...chosen.current];
          const retained = await Promise.all(checked.map(async row => {
            if (result.some(next => next.version_id === row.version_id)) return row;
            try { await call("discussion_source", {source:row.source}); return row; }
            catch (error) { if (unavailable(error)) return null; throw error; }
          }));
          if (active) {
            const erased = checked.filter(row => retained.some(value => value?.version_id === row.version_id) === false);
            chosen.current = chosen.current.filter(row => !erased.includes(row));
            setSelected(ids => ids.filter(id => !erased.some(row => row.version_id === id)));
            setRows([...result,...chosen.current.filter(row => !result.some(next => next.version_id === row.version_id))]);
          }
        })
        .catch(e => { if (active) setError(errorText(e)); });
    }, 180);
    return () => { active = false; clearTimeout(timer); };
  }, [memoryId, versionId, revision, collectionId, paused]);
  async function discuss() {
    if (lock.current || !selected.length) return;
    lock.current = true; setBusy(true); setError("");
    try {
      await Promise.all(chosen.current.map(row => call("discussion_source", {source:row.source})));
      await onDiscuss(rows.filter(r => selected.includes(r.version_id)).map(r => r.source));
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  if (!rows.length && !error) return null;
  return <section className="related-memories" aria-label="相关记忆">
    <div className="section-heading"><h3>相关记忆</h3><span>来自本地内容的词语关联</span></div>
    <ErrorNotice text={error} />
    {rows.map(row => <article className="related-memory" key={row.version_id}>
      <input type="checkbox" aria-label={`选择相关记忆：${row.title}`} disabled={busy}
        checked={selected.includes(row.version_id)} onChange={e => { chosen.current = e.target.checked ? [...chosen.current,row] : chosen.current.filter(value => value.version_id !== row.version_id); setSelected(ids => e.target.checked ? [...ids, row.version_id] : ids.filter(id => id !== row.version_id)); }} />
      <button className="related-memory-link" disabled={busy} onClick={() => onOpen({ kind: "memory", id: row.memory_id })}>
        <strong>{row.title}</strong><span>{row.snippet}</span>
      </button>
    </article>)}
    {selected.length > 0 && <button className="outline-button" disabled={busy} onClick={() => void discuss()}>
      {busy ? "打开讨论…" : `结合选中的 ${selected.length} 条继续想`}
    </button>}
  </section>;
}
