import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { call, errorText, native, type Collection, type Key } from "./api";
import { useNotice } from "../i18n/react";

type Suggestion = { collection: Collection; reason: string };

export default function CollectionRecommendations({ record, currentVersion, members, collections, onRefresh }: {
  record: Key; currentVersion: string; members: string[]; collections: Collection[]; onRefresh: () => void;
}) {
  const { t } = useTranslation("workspace");
  const [rows, setRows] = useState<Suggestion[] | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const [reasonFor, setReasonFor] = useState<string | null>(null);
  const alive = useRef(true), lock = useRef(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  async function recommend() {
    if (lock.current || !alive.current) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const result = await call<Suggestion[]>("collection_recommendations", { memoryId: record.id, expectedVersion: currentVersion });
      if (alive.current) { setRows(result); setReasonFor(null); }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  async function accept(suggestion: Suggestion) {
    if (lock.current || !alive.current) return;
    lock.current = true; setBusy(true); setError("");
    try {
      await call("collection_accept_recommendation", { memoryId: record.id, expectedVersion: currentVersion, collection: suggestion.collection.id, revision: suggestion.collection.revision });
      if (alive.current) {
        setRows(value => value?.filter(s => s.collection.id !== suggestion.collection.id) ?? null);
        onRefresh();
      }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { lock.current = false; if (alive.current) setBusy(false); }
  }
  if (!native) return null;
  const visible = rows?.filter(row => !members.includes(row.collection.id) && collections.some(collection => collection.id === row.collection.id && collection.revision === row.collection.revision));
  const reason = visible?.find(row => row.collection.id === reasonFor)?.reason;
  return <section className="collection-recommendations" aria-label={t("recommendations.aria")}>
    <div className="recommendation-tags">
      <button className="quiet" disabled={busy} onClick={() => void recommend()}>{busy ? t("recommendations.working") : t("recommendations.request")}</button>
      {visible?.map(s => <div className="recommendation-chip" key={s.collection.id}>
        <button disabled={busy} onClick={() => void accept(s)}>＋ {s.collection.name}</button>
        <button className="recommendation-why" aria-label={t("recommendations.why", { name: s.collection.name })} aria-expanded={reasonFor === s.collection.id} onClick={() => setReasonFor(v => v === s.collection.id ? null : s.collection.id)}>?</button>
      </div>)}
      {!!visible?.length && <button className="quiet" disabled={busy} onClick={() => { setRows(null); setReasonFor(null); }}>{t("recommendations.dismiss")}</button>}
    </div>
    {rows && !visible?.length && !busy && <p className="recommendation-reason">{t("recommendations.empty")}</p>}
    {reason && <p className="recommendation-reason">{reason}</p>}
    {error && <p role="status">{error}</p>}
  </section>;
}
