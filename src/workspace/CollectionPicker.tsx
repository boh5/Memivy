import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { message } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { call, errorText, keyOf, type Collection, type Page, type Row } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useResourceVersion } from "./resources";

const pageSize = 30;

export default function CollectionPicker({ collection, onClose }: { collection: Collection; onClose: () => void }) {
  const { t } = useTranslation(["workspace", "common"]);
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [offset, setOffset] = useState(0);
  const [attempt, retry] = useState(0);
  const [page, setPage] = useState<{ request: string; value: Page } | null>(null);
  const [readError, setReadError] = useNotice();
  const [writeError, setWriteError] = useNotice();
  const [notice, setNotice] = useNotice();
  const [adding, setAdding] = useState<string | null>(null);
  const lock = useRef(false);
  const composing = useRef(false);
  const search = useRef<HTMLInputElement>(null);
  const revision = useResourceVersion([{ domain: "memory" }, { domain: "collection" }, { domain: "search" }]);
  const request = JSON.stringify([collection.id, query, offset, revision, attempt]);
  const results = page?.request === request ? page.value : null;
  const canGoBack = !adding && offset > 0;
  const nextOffset = results?.next_offset;
  const canGoForward = !adding && nextOffset != null;

  useEffect(() => {
    let active = true;
    setReadError("");
    const timer = setTimeout(() => {
      void call<Page>("library_query", { query: {
        query, trash: false, keyword_only: true, exclude_collection_id: collection.id, offset, limit: pageSize,
      } }).then(value => {
        if (!active) return;
        if (!value.items.length && offset > 0) setOffset(Math.max(0, offset - pageSize));
        else setPage({ request, value });
      }).catch(error => { if (active) setReadError(errorText(error)); });
    }, query ? 120 : 0);
    return () => { active = false; clearTimeout(timer); };
  }, [request, query, offset, collection.id]);

  function searchFor(value: string) {
    const term = value.trim();
    if (term === query) return;
    setQuery(term);
    setOffset(0);
    setReadError("");
  }

  async function add(row: Row, button: HTMLButtonElement) {
    if (lock.current || !results?.items.includes(row)) return;
    lock.current = true;
    setAdding(keyOf(row.key));
    setNotice("");
    setWriteError("");
    try {
      await call("navigation_collect", { collection: collection.id, key: row.key, included: true });
      setNotice(message("workspace", "collection.pickerAdded", { title: row.title }));
      if (document.activeElement === button || document.activeElement === document.body) search.current?.focus({ preventScroll: true });
      retry(value => value + 1);
    } catch (error) { setWriteError(errorText(error)); }
    finally { lock.current = false; setAdding(null); }
  }

  return <Modal title={t("collection.pickerTitle", { name: collection.name })}
    className="collection-dialog suggestions-dialog" onClose={() => { if (!lock.current) onClose(); }}>
    <input className="memory-picker-query" ref={search} data-modal-autofocus
      aria-label={t("input.searchMemory")} placeholder={t("input.searchMemory")} value={input}
      onCompositionStart={() => { composing.current = true; }}
      onCompositionEnd={event => { composing.current = false; searchFor(event.currentTarget.value); }}
      onChange={event => { setInput(event.target.value); if (!composing.current) searchFor(event.target.value); }} />
    <p className="collection-picker-notice" role="status">{notice}</p>
    <ErrorNotice text={writeError} />
    <ErrorNotice text={readError} />
    {readError && <button className="outline-button" onClick={() => retry(value => value + 1)}>{t("retry", { ns: "common" })}</button>}
    {!results && !readError && <p className="field-help" role="status">{t("list.loading")}</p>}
    {results && !results.items.length && <p className="field-help">{t("collection.pickerEmpty")}</p>}
    <div className="suggestion-rows" aria-busy={!results && !readError}>
      {results?.items.map(row => <article key={keyOf(row.key)}>
        <div><h3>{row.title}</h3><p>{row.snippet}</p></div>
        <button className="toolbar-button" disabled={!!adding}
          aria-label={t("collection.pickerAdd", { title: row.title })}
          onClick={event => void add(row, event.currentTarget)}>
          {adding === keyOf(row.key) ? t("collection.pickerAdding") : t("collection.add")}
        </button>
      </article>)}
    </div>
    <div className="action-row collection-picker-nav">
      <button className="outline-button" aria-disabled={!canGoBack} onClick={() => { if (canGoBack) setOffset(Math.max(0, offset - pageSize)); }}>{t("activity.previous")}</button>
      <button className="outline-button" aria-disabled={!canGoForward} onClick={() => { if (canGoForward) setOffset(nextOffset); }}>{t("activity.next")}</button>
      <button className="send-button" disabled={!!adding} onClick={onClose}>{t("collection.done")}</button>
    </div>
  </Modal>;
}
