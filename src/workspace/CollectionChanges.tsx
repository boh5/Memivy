import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { call, unavailable, type CollectionChange, type Detail, type Key } from "./api";
import { Icon } from "../ui";
import { useResourceVersion } from "./resources";

type MemberTitle = { state: "available"; title: string } | { state: "unavailable" | "failed" };

function ChangedMembers({ ids, added, onOpenRecord }: { ids: string[]; added: boolean; onOpenRecord: (key: Key) => void }) {
  const { t } = useTranslation("workspace");
  const [expanded, setExpanded] = useState(false);
  const [titles, setTitles] = useState<Record<string, MemberTitle>>({});
  const [retry, setRetry] = useState(0), [loading, setLoading] = useState(false);
  const identity = ids.join(":");
  const revision = useResourceVersion(ids.map(id => ({ domain: "memory", entity: `memory:${id}` })));
  useEffect(() => {
    if (!expanded) return;
    let active = true;
    setLoading(true);
    void Promise.all(ids.map(async id => {
      try { return [id, { state: "available", title: (await call<Detail>("library_detail", { key: { kind: "memory", id }, archives: false })).title } satisfies MemberTitle] as const; }
      catch (error) { return [id, { state: unavailable(error) ? "unavailable" : "failed" } satisfies MemberTitle] as const; }
    })).then(entries => { if (active) { setTitles(Object.fromEntries(entries)); setLoading(false); } });
    return () => { active = false; };
  }, [expanded, identity, retry, revision]);
  if (!ids.length) return null;
  return <div className={`collection-member-delta ${added ? "added" : "removed"}`}>
    <button className="collection-members-toggle" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>
      <span aria-hidden="true">{added ? "+" : "−"}</span>
      {t(added ? "collectionChanges.addedMembers" : "collectionChanges.removedMembers", { count: ids.length })}
      <Icon name="chevron" size={12} />
    </button>
    {expanded && <ul>{ids.map((id, index) => <li key={id}><button disabled={titles[id]?.state !== "available"} onClick={() => onOpenRecord({ kind: "memory", id })}>
      {titles[id] === undefined ? t("collectionChanges.loadingMember", { count: index + 1 }) : titles[id].state === "available" ? titles[id].title : t(titles[id].state === "unavailable" ? "collectionChanges.unavailableMember" : "collectionChanges.memberReadFailed")}
    </button></li>)}</ul>}
    {expanded && ids.some(id => titles[id]?.state === "failed") && <button className="quiet" disabled={loading} onClick={() => setRetry(value => value + 1)}>{t("detail.retryEvidence")}</button>}
  </div>;
}

export default function CollectionChanges({ changes, onOpenRecord }: { changes: CollectionChange[]; onOpenRecord: (key: Key) => void }) {
  const { t } = useTranslation("workspace");
  return <div className="collection-change-list">{changes.map((change, index) => {
    const created = !change.before;
    const renamed = change.before && change.before.name !== change.after.name;
    const descriptionChanged = change.before?.description !== change.after.description;
    return <section key={`${change.collection_id}:${index}`} className="collection-change">
      <div className="collection-change-heading"><Icon name="folder" size={17} /><h3>{change.after.name}</h3>{created && <span>{t("collectionChanges.created")}</span>}</div>
      {renamed && <div className="collection-name-change"><span>{change.before!.name}</span><Icon name="arrow" size={12} /><strong>{change.after.name}</strong></div>}
      {descriptionChanged && (change.before?.description || change.after.description) && <div className="collection-description-change">
        <h4>{t("collection.focusAria")}</h4>
        {!created && <p className="before"><span>{t("receipt.before")}</span>{change.before?.description || t("collectionChanges.noDescription")}</p>}
        <p><span>{t("receipt.after")}</span>{change.after.description || t("collectionChanges.noDescription")}</p>
      </div>}
      <ChangedMembers ids={change.added_memory_ids} added onOpenRecord={onOpenRecord} />
      <ChangedMembers ids={change.removed_memory_ids} added={false} onOpenRecord={onOpenRecord} />
    </section>;
  })}</div>;
}
