import { useResourceVersion } from "./resources";
import { useEffect, useState } from "react";
import { call, keyOf, type Collection, type Key, type Receipt, type RecordNavigation } from "./api";
import OrganizationCollections from "./OrganizationCollections";

type Job = { status: string; receipt: Receipt | null };
export default function MemoryCollections({ record, revision: requestedRevision = 0, currentVersion, onRefresh }: { record: Key; revision?: number; currentVersion?: string; onRefresh: () => void }) {
  const revision = useResourceVersion([{domain:"collection"},{domain:"navigation",entity:`${record.kind}:${record.id}`},{domain:"organization",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [result, setResult] = useState<{owner:string; collections:Collection[]; receipt:Receipt|null}|null>(null);
  const owner = keyOf(record);
  useEffect(() => {
    let active = true;
    void Promise.all([call<Collection[]>("navigation_collections"),call<RecordNavigation>("navigation_record",{key:record}),call<Job[]>("organization_jobs",{key:record})]).then(([all, nav, jobs]) => {
      if (active) setResult({owner,collections:all.filter(c => nav.collections.includes(c.id)),receipt:jobs[0]?.status === "done" && jobs[0]?.receipt?.status === "applied" ? jobs[0].receipt : null});
    }).catch(() => { /* A failed refresh does not remove known memberships. */ });
    return () => {active=false;};
  },[owner,revision]);
  if(result?.owner !== owner)return null;
  return <div className="memory-collections">
    {result.collections.length>0 && <div className="memory-collection-tags" aria-label="所属专题">{result.collections.map(c => <span key={c.id} className="collection-tag">{c.name}</span>)}</div>}
    {result.receipt?.memory_id && result.receipt.after_version === currentVersion && <OrganizationCollections key={result.receipt.request_id} receipt={result.receipt.request_id} record={{kind:"memory",id:result.receipt.memory_id}} disabled={false} onRefresh={onRefresh} />}
  </div>;
}
