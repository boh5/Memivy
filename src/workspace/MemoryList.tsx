import Select from "./Select";
import { expireQueries, useResourceVersion } from "./resources";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { call, date, errorText, keyOf, sourceName, native, type Key, type Page, type Query } from "./api";
import { Empty, ErrorNotice } from "./components";
import { useNotice } from "../i18n/react";

export default function MemoryList({ trash, active, selected, revision: requestedRevision = 0, onSelect, onCapture, onRefresh, review = false, collectionId }: {
  review?: boolean; collectionId?: string;
  trash: boolean; active: boolean; selected: Key | null; revision?: number;
  onSelect: (key: Key) => void; onCapture: () => void; onRefresh: () => void;
}) {
  const { t } = useTranslation("workspace");
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
  const [loading, setLoading] = useState(true), [listError, setListError] = useNotice();
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
      aria-label={trash ? t("list.trashAria") : t("list.libraryAria")}
    >
      <div className="library-list-heading">
        <div>
          <span className="eyebrow">
            {trash
              ? t("list.trashEyebrow")
              : t("list.libraryEyebrow")}
          </span>
          <h1>{trash ? t("list.trashTitle") : review ? t("list.reviewTitle") : collectionId ? t("list.collectionTitle") : t("list.libraryTitle")}</h1>
        </div>
        <button
          className="icon-button"
          aria-label={t("list.refresh")}
          onClick={() => { void expireQueries(["library_query","library_projects","organization_states"]).then(() => { setReload(v => v + 1); onRefresh(); }).catch(e => setListError(errorText(e))); }}
        >
          <Icon name="refresh" />
        </button>
      </div>
      <div className="filter-bar">
        <span>{trash ? t("list.trashSubtitle") : review ? t("list.reviewSubtitle") : t("list.recentSubtitle")}</span>
        <button
          aria-expanded={filtersOpen}
          aria-label={t("list.filter")}
          onClick={() => setFiltersOpen((v) => !v)}
        >
          {filtering ? t("list.filterActive") : t("list.filter")}
          <Icon name="chevron" size={12} />
        </button>
      </div>
      {filtersOpen && (
        <div className="library-filters">
          <label>
            {t("list.sourceLabel")}
            <Select
              aria-label={t("list.sourceFilterAria")}
              value={origin}
              onChange={(e) => setOrigin(e.target.value)}
            >
              <option value="">{t("list.allSources")}</option>
              <option value="user">{t("list.myNotes")}</option>
              <option value="agent">{t("list.agentSource")}</option>
              <option value="conversation">{t("list.confirmedConclusion")}</option>
            </Select>
          </label>
          <label>
            {t("list.projectLabel")}
            <Select
              aria-label={t("list.projectFilterAria")}
              value={project}
              onChange={(e) => setProject(e.target.value)}
            >
              <option value="">{t("list.allProjects")}</option>
              {projects.map((p) => (
                <option key={p}>{p}</option>
              ))}
            </Select>
          </label>
          <label>
            {t("list.updatedLabel")}
            <input
              aria-label={t("list.startDateAria")}
              type="date"
              value={since}
              onChange={(e) => setSince(e.target.value)}
            />
          </label>
          <label>
            {t("list.endDateLabel")}
            <input
              aria-label={t("list.endDateAria")}
              type="date"
              value={until}
              onChange={(e) => setUntil(e.target.value)}
            />
          </label>
          {filtering && (
            <button onClick={resetFilters}>{t("list.clearAll")}</button>
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
            {!trash && states[keyOf(r.key)] && <span className="row-organization-state">{states[keyOf(r.key)].recommendations ? t("list.recommendations", { count: states[keyOf(r.key)].recommendations }) : states[keyOf(r.key)].status === "pending" ? t("list.pending") : states[keyOf(r.key)].status === "processing" ? t("list.processing") : ["failed","deferred","paused"].includes(states[keyOf(r.key)].status) ? t("list.incomplete") : ""}</span>}
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
                ? t("list.emptyFilteredTitle")
                : trash
                  ? t("list.emptyTrashTitle")
                  : collectionId ? t("list.emptyCollectionTitle") : t("list.emptyLibraryTitle")
            }
            text={
              filtering
                ? t("list.emptyFilteredText")
                : trash
                  ? t("list.emptyTrashText")
                  : collectionId ? t("list.emptyCollectionText") : t("list.emptyLibraryText")
            }
          >
            {filtering ? (
              <button className="outline-button" onClick={resetFilters}>
                {t("list.clearFilters")}
              </button>
            ) : (
              !trash && !collectionId && (
                <button className="outline-button" onClick={onCapture}>
                  {t("list.captureIdea")}
                </button>
              )
            )}
          </Empty>
        )}
        {loading && loadedSignature.current !== signature && (
          <p className="list-progress" role="status">
            {t("list.loading")}
          </p>
        )}
        {review && !loading && result.items.length > 0 && <button className="load-more review-next" onClick={() => setReviewOffset(result.next_offset ?? 0)}>{result.next_offset !== null ? t("list.reviewMore") : t("list.reviewFromStart")}<Icon name="arrow" size={13} /></button>}
        {!review && result.next_offset !== null && (
          <button
            className="load-more"
            disabled={loading}
            onClick={() => void more()}
          >
            {t("list.loadMore")}
          </button>
        )}
      </div>
    </section>
  );
}
