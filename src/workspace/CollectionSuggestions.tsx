import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { call, errorText, keyOf, type Collection, type Row } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useNotice } from "../i18n/react";

export default function CollectionSuggestions({ collection, onChanged, onClose }: {
  collection: Collection; onChanged: () => void; onClose: () => void;
}) {
  const [rows, setRows] = useState<Row[]>([]), [loading, setLoading] = useState(true);
  const [error, setError] = useNotice(), [attempt, setAttempt] = useState(0);
  const [added, setAdded] = useState<string[]>([]), [busy, setBusy] = useState(false);
  const { t } = useTranslation("workspace");
  const lock = useRef(false);
  useEffect(() => {
    let alive = true; setLoading(true); setError("");
    void call<Row[]>("navigation_suggest", { collection: collection.id }).then(r => { if (alive) setRows(r); })
      .catch(e => { if (alive) setError(errorText(e)); }).finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [collection.id, attempt]);
  async function add(row: Row) {
    if (lock.current || added.includes(keyOf(row.key))) return;
    lock.current = true; setBusy(true); setError("");
    try { await call("navigation_collect", { collection: collection.id, key: row.key, included: true }); setAdded(v => [...v, keyOf(row.key)]); onChanged(); }
    catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  return <Modal title={t("collection.findMemoriesTitle")} onClose={() => { if (!lock.current) onClose(); }} className="collection-dialog suggestions-dialog">
    <p className="field-help">{t("collection.findMemoriesDescription", { name: collection.name })}</p>
    {loading && <p className="suggestions-progress" role="status">{t("collection.findingMemories")}</p>}
    {!loading && !error && !rows.length && <p className="field-help">{t("collection.noMoreMemories")}</p>}
    <div className="suggestion-rows">{rows.map(r => <article key={keyOf(r.key)}><div><h3>{r.title}</h3><p>{r.snippet}</p></div><button className="toolbar-button" disabled={busy || added.includes(keyOf(r.key))} onClick={() => void add(r)}>{added.includes(keyOf(r.key)) ? t("collection.added") : t("collection.add")}</button></article>)}</div>
    <ErrorNotice text={error} />
    <div className="action-row">{error && !loading && <button className="outline-button" onClick={() => setAttempt(v => v + 1)}>{t("collection.retrySuggestions")}</button>}<button className="send-button" disabled={busy} onClick={onClose}>{t("collection.done")}</button></div>
  </Modal>;
}
