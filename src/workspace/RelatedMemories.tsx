import { useEffect, useRef, useState } from "react";
import { call, errorText, type Key, type Source } from "./api";
import { ErrorNotice } from "./components";

type Related = { memory_id: string; version_id: string; title: string; snippet: string; source: Source };
export default function RelatedMemories({ memoryId, versionId, revision, onOpen, onDiscuss }: {
  memoryId: string; versionId: string; revision: number;
  onOpen: (key: Key) => void; onDiscuss: (sources: Source[]) => Promise<void>;
}) {
  const [rows, setRows] = useState<Related[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  useEffect(() => {
    let active = true;
    setRows([]); setSelected([]); setError("");
    // Collapse rapid refresh events into one bounded local query.
    const timer = setTimeout(() => {
      void call<Related[]>("memory_related", { memoryId, expectedVersion: versionId })
        .then(result => { if (active) setRows(result); })
        .catch(e => { if (active) setError(errorText(e)); });
    }, 180);
    return () => { active = false; clearTimeout(timer); };
  }, [memoryId, versionId, revision]);
  async function discuss() {
    if (lock.current || !selected.length) return;
    lock.current = true; setBusy(true); setError("");
    try {
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
        checked={selected.includes(row.version_id)} onChange={e => setSelected(ids => e.target.checked ? [...ids, row.version_id] : ids.filter(id => id !== row.version_id))} />
      <button className="related-memory-link" disabled={busy} onClick={() => onOpen({ kind: "memory", id: row.memory_id })}>
        <strong>{row.title}</strong><span>{row.snippet}</span>
      </button>
    </article>)}
    {selected.length > 0 && <button className="outline-button" disabled={busy} onClick={() => void discuss()}>
      {busy ? "打开讨论…" : `结合选中的 ${selected.length} 条继续想`}
    </button>}
  </section>;
}
