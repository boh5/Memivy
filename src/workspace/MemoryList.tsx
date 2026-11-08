import Select from "./Select";
import { expireQueries, useResourceVersion } from "./resources";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { call, date, errorText, keyOf, sourceName, type Key, type Page, type Query } from "./api";
import { Empty, ErrorNotice, Highlight, MoreMenu } from "./components";
import { useNotice } from "../i18n/react";

export default function MemoryList({ trash, active, selected, revision: requestedRevision = 0, onSelect, onCapture, onRefresh, collectionId, onImport, onCopyContext }: {
  collectionId?: string; onImport?: () => void; onCopyContext?: () => void;
  trash: boolean; active: boolean; selected: Key | null; revision?: number;
  onSelect: (key: Key, query: string) => void; onCapture: () => void; onRefresh: () => void;
}) {
  const { t } = useTranslation("workspace");
  const [search, setSearch] = useState(""), [query, setQuery] = useState("");
  const [composing, setComposing] = useState(false);
  const searchField = useRef<HTMLInputElement>(null);
  const filterId = useId();
  const [reload, setReload] = useState(0);
  const revision = useResourceVersion([{domain:"memory"},{domain:"navigation"},{domain:"collection"},{domain:"search"}]) + requestedRevision + reload;
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
  const requestedQuery = search.trim();
  useEffect(() => {
    if (composing || query === requestedQuery) return;
    const timer = setTimeout(() => setQuery(requestedQuery), 160);
    return () => clearTimeout(timer);
  }, [composing, query, requestedQuery]);
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
    query, trash, keyword_only: true, collection_id: collectionId, origin: origin || undefined, project: project || undefined,
    since: since ? new Date(`${since}T00:00:00`).getTime() : undefined,
    until: until ? new Date(new Date(`${until}T00:00:00`).setDate(new Date(`${until}T00:00:00`).getDate() + 1)).getTime() : undefined,
  };
  const signature = JSON.stringify(options);
  const invalidDates = !!since && !!until && since > until;
  const visibleQuery = composing ? query : requestedQuery;
  const pendingSearch = visibleQuery !== query;
  const matchingResults = !invalidDates && !pendingSearch && loadedSignature.current === signature;
  const waiting = !invalidDates && (loading || pendingSearch);
  useEffect(() => {
    let alive = true;
    void call<string[]>("library_projects").then(p => { if (alive) setProjects(p); })
      .catch(e => { if (alive) setListError(errorText(e)); });
    return () => { alive = false; };
  }, [revision]);
  useEffect(() => {
    const seq = ++sequence.current;
    setListError(""); loadingMore.current = false;
    if (invalidDates) { setLoading(false); return; }
    setLoading(true);
    const readWindow = async (): Promise<Page> => {
      const count = loadedSignature.current === signature ? visibleCount.current : 40;
      let page = await call<Page>("library_query", { query: { ...options, limit: 40 } });
      const items = [...page.items];
      while (items.length < count && page.next_offset !== null && sequence.current === seq) {
        page = await call<Page>("library_query", { query: { ...options, offset: page.next_offset, limit: 40 } });
        items.push(...page.items);
      }
      return { ...page, items: [...new Map(items.map(row => [keyOf(row.key), row])).values()] };
    };
    void readWindow()
      .then(r => { if (sequence.current === seq) { const list = viewport.current;
        if (loadedSignature.current !== signature && list) list.scrollTop = 0;
        else if (list && list.scrollTop > 0) {
          const top = list.getBoundingClientRect().top;
          const row = [...list.querySelectorAll<HTMLElement>("[data-record]")].find(row => row.getBoundingClientRect().bottom > top);
          if (row) anchor.current = {key:row.dataset.record!,top:row.getBoundingClientRect().top};
        }
        loadedSignature.current = signature; visibleCount.current = Math.max(40, r.items.length); setResult(r); } })
      .catch(e => { if (sequence.current === seq) { setListError(errorText(e)); if (loadedSignature.current !== signature) setResult({ items: [], next_offset: null }); } })
      .finally(() => { if (sequence.current === seq) setLoading(false); });
    return () => { ++sequence.current; };
  }, [signature, revision, invalidDates]);
  async function more() {
    if (!matchingResults || loading || loadingMore.current || result.next_offset === null) return;
    loadingMore.current = true; setLoading(true);
    setListError("");
    const seq = sequence.current;
    try {
      const r = await call<Page>("library_query", { query: { ...options, offset: result.next_offset, limit: 40 } });
      if (seq === sequence.current) setResult(old => {
        const seen = new Set(old.items.map(r => keyOf(r.key)));
        const items = [...old.items, ...r.items.filter(r => !seen.has(keyOf(r.key)))];
        visibleCount.current = items.length;
        return { ...r, items };
      });
    } catch (e) { if (seq === sequence.current) setListError(errorText(e)); }
    finally { if (seq === sequence.current) { loadingMore.current = false; setLoading(false); } }
  }
  function clearSearch() { setSearch(""); setQuery(""); searchField.current?.focus(); }
  function resetFilters() { setOrigin(""); setProject(""); setSince(""); setUntil(""); }
  const filtering = !!(origin || project || since || until);
  function refresh() {
    void expireQueries(["library_query", "library_projects"])
      .then(() => { setReload(value => value + 1); onRefresh(); })
      .catch(e => setListError(errorText(e)));
  }
  return (
    <section
      className="library-list-pane"
      aria-label={trash ? t("list.trashAria") : t("list.libraryAria")}
    >
      <div className="library-list-heading">
        <h1>{trash ? t("list.trashTitle") : collectionId ? t("list.collectionTitle") : t("list.libraryTitle")}</h1>
        <MoreMenu>
          {!trash && onImport && <button onClick={onImport}>{t("import.entry")}</button>}
          {!trash && onCopyContext && <button onClick={onCopyContext}>{t("contextExport.title")}</button>}
          <button onClick={refresh}>{t("list.refresh")}</button>
        </MoreMenu>
      </div>
      <div className="library-search">
        <Icon name="search" size={15} />
        <input ref={searchField} type="search" aria-label={t("list.search")} placeholder={t("list.search")} value={search}
          maxLength={512} onChange={e => setSearch(e.target.value)}
          onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} />
        {search && <button type="button" className="icon-button" aria-label={t("list.clearSearch")} onClick={clearSearch}><Icon name="close" size={14} /></button>}
      </div>
      <div className="filter-bar">
        <span>{visibleQuery ? t("list.searchResults") : trash ? t("list.trashSubtitle") : t("list.recentSubtitle")}</span>
        <button
          aria-expanded={filtersOpen}
          aria-controls={filtersOpen ? filterId : undefined}
          onClick={() => setFiltersOpen((v) => !v)}
        >
          {filtering ? t("list.filterActive") : t("list.filter")}
          <Icon name="chevron" size={12} />
        </button>
      </div>
      {filtersOpen && (
        <div className="library-filters" id={filterId}>
          <label>
            {t("list.sourceLabel")}
            <Select
              aria-label={t("list.sourceFilterAria")}
              value={origin}
              onChange={(e) => setOrigin(e.target.value)}
            >
              <option value="">{t("list.allSources")}</option>
              <option value="user">{t("list.myNotes")}</option>
              <option value="imported">{t("import.sourceFilter")}</option>
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
      <ErrorNotice text={invalidDates ? t("list.invalidDates") : pendingSearch ? "" : listError} />
      <div
        className="library-rows"
        aria-busy={waiting}
        ref={viewport}
      >
        {matchingResults && result.items.map((r) => (
          <button
            aria-current={active && selected && keyOf(r.key) === keyOf(selected) ? "true" : undefined}
            className={`library-row ${active && selected && keyOf(r.key) === keyOf(selected) ? "selected" : ""}`}
            key={keyOf(r.key)}
            data-record={keyOf(r.key)}
            onClick={() => onSelect(r.key, query)}
          >
            <div className="row-title">
              <strong>
                {query ? <Highlight text={r.title} query={query} /> : r.title}
              </strong>
            </div>
            <p>
              {query ? <Highlight text={r.snippet} query={query} /> : r.snippet}
            </p>
            <div className="row-meta">
              <span>
                {sourceName(r.origin)}
              </span>
              <time>{date(r.updated_at)}</time>
            </div>
          </button>
        ))}
        {matchingResults && !result.items.length && !waiting && !listError && (
          <Empty
            title={
              query
                ? t("list.emptySearchTitle")
                : filtering
                ? t("list.emptyFilteredTitle")
                : trash
                  ? t("list.emptyTrashTitle")
                  : collectionId ? t("list.emptyCollectionTitle") : t("list.emptyLibraryTitle")
            }
            text={
              query
                ? t("list.emptySearchText")
                : filtering
                ? t("list.emptyFilteredText")
                : trash
                  ? t("list.emptyTrashText")
                  : collectionId ? t("list.emptyCollectionText") : t("list.emptyLibraryText")
            }
          >
            {query && <button className="outline-button" onClick={clearSearch}>{t("list.clearSearch")}</button>}
            {filtering ? (
              <button className="outline-button" onClick={resetFilters}>
                {t("list.clearFilters")}
              </button>
            ) : (
              !query && !trash && !collectionId && (
                <button className="outline-button" onClick={onCapture}>
                  {t("list.captureIdea")}
                </button>
              )
            )}
          </Empty>
        )}
        {waiting && !matchingResults && (
          <p className="list-progress" role="status">
            {visibleQuery ? t("list.searching") : t("list.loading")}
          </p>
        )}
        {matchingResults && result.next_offset !== null && (
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
