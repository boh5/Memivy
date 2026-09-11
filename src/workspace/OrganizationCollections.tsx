import { useResourceVersion } from "./resources";
import { useEffect, useRef, useState } from "react";
import { notify } from "./Toast";
import { call, errorText, native, type Collection, type Key, type RecordNavigation } from "./api";

type Suggestion = { collection: Collection; reason: string };
// A receipt can be visible in both the capture feedback and reading pane. Share
// in-flight/results for this session; UI refreshes never start another model call.
const recommendations = new Map<string, Promise<Suggestion[]>>();
const dismissed = new Set<string>();
function load(receipt: string, retry: boolean) {
  if (retry) recommendations.delete(receipt);
  let request = recommendations.get(receipt);
  if (!request) {
    request = call<Suggestion[]>("organization_collections", { receipt });
    recommendations.set(receipt, request);
    if (recommendations.size > 64) {
      const oldest = recommendations.keys().next().value!;
      recommendations.delete(oldest); dismissed.delete(oldest);
    }
  }
  return request;
}

export default function OrganizationCollections({ receipt, record, revision: requestedRevision = 0, disabled, onRefresh }: {
  receipt: string; record: Key; revision?: number; disabled: boolean; onRefresh: () => void;
}) {
  const revision = useResourceVersion([{domain:"navigation",entity:`${record.kind}:${record.id}`},{domain:"collection"}]) + requestedRevision;
  const [rows, setRows] = useState<Suggestion[] | null>(null), [error, setError] = useState("");
  const [hidden, setHidden] = useState(() => dismissed.has(receipt)), [attempt, setAttempt] = useState(0);
  const [members, setMembers] = useState<string[]>([]);
  const [reasonFor, setReasonFor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false), [ready, setReady] = useState(false);
  const alive = useRef(true), lock = useRef(false), sequence = useRef(0);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    if (!native || hidden) return;
    let active = true; setRows(null); setError("");
    void load(receipt, attempt > 0).then(value => { if (active) setRows(value); })
      .catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [receipt, attempt, hidden]);
  useEffect(() => {
    if (!native || hidden) return;
    const request = ++sequence.current;
    void call<RecordNavigation>("navigation_record", { key: record }).then(value => {
      if (alive.current && sequence.current === request) { setMembers(value.collections); setReady(true); }
    }).catch(e => { if (alive.current && sequence.current === request) setError(errorText(e)); });
    return () => { if (sequence.current === request) sequence.current++; };
  }, [record.id, record.kind, revision, hidden, attempt]);
  async function update(suggestion: Suggestion, undo: boolean) {
    if (lock.current || !alive.current || disabled || !ready) return;
    lock.current = true; setBusy(true); setError(""); ++sequence.current;
    const id = suggestion.collection.id;
    try {
      if (undo) await call("navigation_collect", { collection: id, key: record, included: false });
      else await call("organization_collect", { receipt, collection: id, revision: suggestion.collection.revision });
      if (!alive.current) return;
      setMembers(v => undo ? v.filter(c => c !== id) : [...new Set([...v, id])]);
      onRefresh();
      if (!undo) notify(`已加入「${suggestion.collection.name}」`, "撤销", () => {
        void call("navigation_collect", {collection:id,key:record,included:false}).then(() => { if(alive.current)onRefresh(); }).catch(e => { if(alive.current)setError(errorText(e)); });
      }, 8000);
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  if (!native || hidden || rows?.length === 0 && !error) return null;
  async function dismiss() {
    if(lock.current)return; lock.current=true;setBusy(true);
    try { await call("organization_dismiss",{receipt}); if(alive.current){dismissed.add(receipt);setHidden(true);onRefresh();} }
    catch(e){if(alive.current)setError(errorText(e));}
    finally {lock.current=false;if(alive.current)setBusy(false);}
  }
  const visible = rows?.filter(s => !members.includes(s.collection.id));
  if (rows && !visible?.length && !error) return null;
  return <section className="organization-collections" aria-label="专题推荐">
    <div className="recommendation-tags">
      <span className="recommendation-label">{rows?.length ? "建议加入" : error ? "专题推荐" : "正在寻找合适的专题…"}</span>
      {visible?.map(s => <div className="recommendation-chip" key={s.collection.id}>
        <button disabled={disabled || busy || !ready} onClick={() => void update(s,false)}>＋ {s.collection.name}</button>
        <button className="recommendation-why" aria-label={`为什么推荐${s.collection.name}`} aria-expanded={reasonFor===s.collection.id} onClick={() => setReasonFor(v=>v===s.collection.id?null:s.collection.id)}>?</button>
      </div>)}
      <button className="quiet dismiss-recommendations" disabled={busy} onClick={() => void dismiss()}>忽略本次建议</button>
    </div>
    {reasonFor && <p className="recommendation-reason">{visible?.find(s=>s.collection.id===reasonFor)?.reason}</p>}
    {error && <div className="organization-collections-error"><span role="status">{error}</span><button className="quiet" disabled={busy || disabled} onClick={() => setAttempt(v => v + 1)}>重试推荐</button></div>}
  </section>;
}
