import { useResourceVersion } from "./resources";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { call, keyOf, type Collection, type Key, type RecordNavigation } from "./api";
import CollectionRecommendations from "./CollectionRecommendations";

export default function MemoryCollections({ record, revision: requestedRevision = 0, currentVersion, onRefresh }: { record: Key; revision?: number; currentVersion: string; onRefresh: () => void }) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{domain:"collection"},{domain:"navigation",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [result, setResult] = useState<{owner:string; collections:Collection[]; members:string[]}|null>(null);
  const owner = keyOf(record);
  useEffect(() => {
    let active = true;
    void Promise.all([call<Collection[]>("navigation_collections"),call<RecordNavigation>("navigation_record",{key:record})]).then(([all, nav]) => {
      if (active) setResult({owner,collections:all,members:nav.collections});
    }).catch(() => { /* A failed refresh does not remove known memberships. */ });
    return () => {active=false;};
  },[owner,revision]);
  if(result?.owner !== owner)return null;
  const memberships = result.collections.filter(c => result.members.includes(c.id));
  return <div className="memory-collections">
    {memberships.length>0 && <div className="memory-collection-tags" aria-label={t("collection.membershipAria")}>{memberships.map(c => <span key={c.id} className="collection-tag">{c.name}</span>)}</div>}
    <CollectionRecommendations key={`${owner}:${currentVersion}`} record={record} currentVersion={currentVersion} members={result.members} collections={result.collections} onRefresh={onRefresh} />
  </div>;
}
