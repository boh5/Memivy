import { expireQueries, useResourceVersion } from "./resources";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import { call, date, errorText, keyOf, sourceName, native, type Key, type Page, type Query } from "./api";
import { Empty, ErrorNotice } from "./components";

export default function MemoryList({ trash, active, selected, revision: requestedRevision = 0, onSelect, onCapture, onRefresh, review = false, collectionId }: {
  review?: boolean; collectionId?: string;
  trash: boolean; active: boolean; selected: Key | null; revision?: number;
  onSelect: (key: Key) => void; onCapture: () => void; onRefresh: () => void;
}) {
  const [reload, setReload] = useState(0);
  const revision = useResourceVersion([{domain:"memory"},{domain:"navigation"},{domain:"collection"}]) + requestedRevision + reload;
  const statusRevision = useResourceVersion([{domain:"organization"}]);
  const [states, setStates] = useState<Record<string,{status:string;recommendations:number}>>({});
  const [reviewOffset, setReviewOffset] = useState(0);
  const [origin, setOrigin] = useState(""), [project, setProject] = useState("");
  const [since, setSince] = useState(""), [until, setUntil] = useState("");
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [projects, setProjects] = useState<string[]>([]);
  const [result, setResult] = useState<Page>({ items: [], next_offset: null });
  const [loading, setLoading] = useState(true), [listError, setListError] = useState("");
  const sequence = useRef(0), loadingMore = useRef(false);
  const visibleCount = useRef(40);
  const viewport = useRef<HTMLDivElement>(null);
  const anchor = useRef<{key:string; top:number} | null>(null);
  useLayoutEffect(() => {
    const saved = anchor.current, list = viewport.current;
    if (saved && list) {
      const row = [...list.querySelectorAll<HTMLElement>("[data-record]")].find(row => row.dataset.record === saved.key);
      if (row) list.scrollTop += row.getBoundingClientRect().top - saved.top;
    }
    anchor.current = null;
  }, [result]);
  const loadedSignature = useRef<string | null>(null);
  const options: Query = {
    query: "", trash, collection_id: collectionId, oldest: review, offset: review ? reviewOffset : undefined, origin: origin || undefined, project: project || undefined,
    since: since ? new Date(`${since}T00:00:00`).getTime() : undefined,
    until: until ? new Date(new Date(`${until}T00:00:00`).setDate(new Date(`${until}T00:00:00`).getDate() + 1)).getTime() : undefined,
  };
  const signature = JSON.stringify(options);
  useEffect(() => { setReviewOffset(0); }, [origin, project, since, until]);
  useEffect(() => {
    let alive = true;
    void call<string[]>("library_projects").then(p => { if (alive) setProjects(p); })
      .catch(e => { if (alive) setListError(errorText(e)); });
    return () => { alive = false; };
  }, [revision]);
  useEffect(() => {
    const seq = ++sequence.current;
    setLoading(true); setListError(""); loadingMore.current = false;
    const readWindow = async (): Promise<Page> => {
      const count = review ? 3 : loadedSignature.current === signature ? visibleCount.current : 40;
      let page = await call<Page>("library_query", { query: { ...options, limit: review ? 3 : 40 } });
      const items = [...page.items];
      while (!review && items.length < count && page.next_offset !== null && sequence.current === seq) {
        page = await call<Page>("library_query", { query: { ...options, offset: page.next_offset, limit: 40 } });
        items.push(...page.items);
      }
      return { ...page, items: [...new Map(items.map(row => [keyOf(row.key), row])).values()] };
    };
    void readWindow()
      .then(r => { if (sequence.current === seq) { const list = viewport.current;
        if (loadedSignature.current === signature && list && list.scrollTop > 0) {
          const top = list.getBoundingClientRect().top;
          const row = [...list.querySelectorAll<HTMLElement>("[data-record]")].find(row => row.getBoundingClientRect().bottom > top);
          if (row) anchor.current = {key:row.dataset.record!,top:row.getBoundingClientRect().top};
        }
        loadedSignature.current = signature; visibleCount.current = Math.max(40, r.items.length); setResult(r); if (review && reviewOffset > 0 && !r.items.length) setReviewOffset(0); } })
      .catch(e => { if (sequence.current === seq) { setListError(errorText(e)); if (loadedSignature.current !== signature) setResult({ items: [], next_offset: null }); } })
      .finally(() => { if (sequence.current === seq) setLoading(false); });
    return () => { ++sequence.current; };
  }, [signature, revision]);
  useEffect(() => {
    if (!native || trash) { setStates({}); return; }
    let active = true;
    const keys = result.items.map(r=>r.key);
    const batches = [];
    for(let i=0;i<keys.length;i+=100)batches.push(call<Array<{key:Key;status:string;recommendations:number}>>("organization_states",{keys:keys.slice(i,i+100)}));
    void Promise.all(batches).then(groups => {if(active)setStates(Object.fromEntries(groups.flat().map(s=>[keyOf(s.key),s])));}).catch(()=>{ /* Retain known statuses on a transient read failure. */ });
    return () => {active=false;};
  },[result,statusRevision,trash]);
  async function more() {
    if (loading || loadingMore.current || result.next_offset === null) return;
    loadingMore.current = true; setLoading(true);
    const seq = sequence.current;
    try {
      const r = await call<Page>("library_query", { query: { ...options, offset: result.next_offset, limit: 40 } });
      if (seq === sequence.current) setResult(old => {
        const seen = new Set(old.items.map(r => keyOf(r.key)));
        const items = [...old.items, ...r.items.filter(r => !seen.has(keyOf(r.key)))];
        visibleCount.current = items.length;
        return { items, next_offset: r.next_offset };
      });
    } catch (e) { if (seq === sequence.current) setListError(errorText(e)); }
    finally { if (seq === sequence.current) { loadingMore.current = false; setLoading(false); } }
  }
  function resetFilters() { setOrigin(""); setProject(""); setSince(""); setUntil(""); }
  const filtering = !!(origin || project || since || until);
  return (
    <section
      className="library-list-pane"
      aria-label={trash ? "回收站列表" : "记忆列表"}
    >
      <div className="library-list-heading">
        <div>
          <span className="eyebrow">
            {trash
              ? "可以恢复，留一份余地"
              : "留在这里，下次用得上"}
          </span>
          <h1>{trash ? "回收站" : review ? "回顾" : collectionId ? "专题记忆" : "全部记忆"}</h1>
        </div>
        <button
          className="icon-button"
          aria-label="刷新记忆列表"
          onClick={() => { void expireQueries(["library_query","library_projects","organization_states"]).then(() => { setReload(v => v + 1); onRefresh(); }).catch(e => setListError(errorText(e))); }}
        >
          <Icon name="refresh" />
        </button>
      </div>
      <div className="filter-bar">
        <span>{trash ? "已删除的记忆" : review ? "从较早的记忆开始" : "最近更新"}</span>
        <button
          aria-expanded={filtersOpen}
          aria-label="筛选记忆列表"
          onClick={() => setFiltersOpen((v) => !v)}
        >
          筛选{filtering ? " · 已启用" : ""}
          <Icon name="chevron" size={12} />
        </button>
      </div>
      {filtersOpen && (
        <div className="library-filters">
          <label>
            来源
            <select
              aria-label="来源筛选"
              value={origin}
              onChange={(e) => setOrigin(e.target.value)}
            >
              <option value="">全部来源</option>
              <option value="user">我的记录</option>
              <option value="agent">来自 Agent</option>
              <option value="conversation">确认的结论</option>
            </select>
          </label>
          <label>
            项目
            <select
              aria-label="项目筛选"
              value={project}
              onChange={(e) => setProject(e.target.value)}
            >
              <option value="">全部项目</option>
              {projects.map((p) => (
                <option key={p}>{p}</option>
              ))}
            </select>
          </label>
          <label>
            更新于
            <input
              aria-label="开始日期"
              type="date"
              value={since}
              onChange={(e) => setSince(e.target.value)}
            />
          </label>
          <label>
            至
            <input
              aria-label="结束日期"
              type="date"
              value={until}
              onChange={(e) => setUntil(e.target.value)}
            />
          </label>
          {filtering && (
            <button onClick={resetFilters}>清除全部条件</button>
          )}
        </div>
      )}
      <ErrorNotice text={listError} />
      <div
        className={`library-rows ${loading && loadedSignature.current !== signature ? "is-loading" : ""}`}
        aria-busy={loading}
        ref={viewport}
      >
        {result.items.map((r) => (
          <button
            aria-current={active && selected && keyOf(r.key) === keyOf(selected) ? "true" : undefined}
            className={`library-row ${active && selected && keyOf(r.key) === keyOf(selected) ? "selected" : ""}`}
            key={keyOf(r.key)}
            data-record={keyOf(r.key)}
            onClick={() => onSelect(r.key)}
          >
            <div className="row-title">
              <strong>
                {r.title}
              </strong>
            </div>
            <p>
              {r.snippet}
            </p>
            {!trash && states[keyOf(r.key)] && <span className="row-organization-state">{states[keyOf(r.key)].recommendations ? `${states[keyOf(r.key)].recommendations} 个专题推荐` : states[keyOf(r.key)].status === "pending" ? "等待整理" : states[keyOf(r.key)].status === "processing" ? "整理中…" : ["failed","deferred","paused"].includes(states[keyOf(r.key)].status) ? "整理未完成" : ""}</span>}
            <div className="row-meta">
              <span>
                {sourceName(r.origin)}
              </span>
              <time>{date(r.updated_at)}</time>
            </div>
          </button>
        ))}
        {!result.items.length && !loading && !listError && (
          <Empty
            title={
              filtering
                ? "没有找到匹配的记忆"
                : trash
                  ? "回收站是空的"
                  : collectionId ? "专题里还没有记忆" : "从一句话开始"
            }
            text={
              filtering
                ? "试试调整来源、项目或日期范围。"
                : trash
                  ? "删除的记忆会留在这里，直到你明确永久删除。"
                  : collectionId ? "在记忆正文点击“专题”手动加入，或试试上方的 AI 推荐。" : "记下的内容立即可用，AI 可以稍后整理。"
            }
          >
            {filtering ? (
              <button className="outline-button" onClick={resetFilters}>
                清除筛选
              </button>
            ) : (
              !trash && !collectionId && (
                <button className="outline-button" onClick={onCapture}>
                  记下一个想法
                </button>
              )
            )}
          </Empty>
        )}
        {loading && loadedSignature.current !== signature && (
          <p className="list-progress" role="status">
            正在读取本地记忆…
          </p>
        )}
        {review && !loading && result.items.length > 0 && <button className="load-more review-next" onClick={() => setReviewOffset(result.next_offset ?? 0)}>{result.next_offset !== null ? "再回顾三条" : "从头回顾"}<Icon name="arrow" size={13} /></button>}
        {!review && result.next_offset !== null && (
          <button
            className="load-more"
            disabled={loading}
            onClick={() => void more()}
          >
            加载更多
          </button>
        )}
      </div>
    </section>
  );
}
