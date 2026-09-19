import {invoke} from '../nativeIpc';
import { QueryClient, isCancelledError } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useSyncExternalStore } from "react";

export type Dependency = { domain: string; entity?: string };
type Cursor = { epoch: string; sequence: number };
type Changes = { cursor: Cursor; reset: boolean; changes: Dependency[] };
const client = new QueryClient({ defaultOptions: { queries: {
  staleTime: Infinity, gcTime: 5 * 60_000, retry: false, networkMode: "always",
  refetchOnWindowFocus: false, refetchOnReconnect: false,
} } });
const keyOf = (key: unknown) => {
  const k = key as {kind?: string; id?: string} | undefined;
  return k?.kind && k.id ? `${k.kind}:${k.id}` : undefined;
};
export function dependencies(name: string, args: Record<string, unknown> = {}): Dependency[] | undefined {
  const record = keyOf(args.key);
  switch (name) {
    case "activity_summary": case "activity_records": return [{domain:"memory"}];
    case "library_detail": return [{domain:"memory",entity:record}];
    case "library_query": case "library_projects":
      return [{domain:"memory"},{domain:"navigation"},{domain:"collection"}];
    case "library_topics": return [{domain:"discussion"},{domain:"collection"}];
    case "discussion_messages":
      return [{domain:"discussion",entity:String(args.topicId ?? args.id ?? "*")}];
    case "navigation_collections": return [{domain:"collection"},{domain:"memory"}];
    case "navigation_record": return [{domain:"navigation",entity:record},{domain:"collection"},{domain:"memory",entity:record}];
    case "organization_jobs": return [{domain:"organization",entity:record},{domain:"memory",entity:record}];
    case "organization_states": return (args.keys as unknown[] ?? []).flatMap(key => [{domain:"organization",entity:keyOf(key)},{domain:"memory",entity:keyOf(key)}]);
    case "memory_related": return [{domain:"memory"},{domain:"collection"},{domain:"navigation"}];
    case "workspace_settings": return [{domain:"settings"}];
    default: return undefined;
  }
}
export function affected(wants: Dependency[], changes: Dependency[]): boolean {
  return wants.some(want => changes.some(change => want.domain === change.domain &&
    (!want.entity || want.entity === "*" || !change.entity || change.entity === "*" || want.entity === change.entity)));
}
const signals = new Set<{ deps: Dependency[]; version: number; listeners: Set<() => void> }>();
let version = 0;
export function useResourceVersion(deps: Dependency[]) {
  const signature = JSON.stringify(deps);
  const state = useMemo(() => {
    const signal = {deps: JSON.parse(signature) as Dependency[], version, listeners: new Set<() => void>()};
    return {
      snapshot: () => signal.version,
      subscribe: (callback: () => void) => {
        signal.listeners.add(callback); signals.add(signal); signal.version = version;
        return () => { signal.listeners.delete(callback); if (!signal.listeners.size) signals.delete(signal); };
      },
    };
  }, [signature]);
  return useSyncExternalStore(state.subscribe, state.snapshot, () => 0);
}
export async function readResource<T>(name: string, args: Record<string, unknown> | undefined, deps: Dependency[]): Promise<T> {
  try {
    return await client.fetchQuery<T>({queryKey:[name,args ?? {}], meta:{dependencies:deps}, queryFn:() => invoke<T>(name,args)});
  } catch (error) {
    // Tauri invoke cannot be aborted, but cancelled results cannot enter the cache.
    if (isCancelledError(error)) { await invalidations; return readResource(name,args,deps); }
    if (error && typeof error === "object" && "code" in error && error.code === "unavailable") {
      client.removeQueries({queryKey:[name,args ?? {}],exact:true});
    }
    throw error;
  }
}
let invalidations = Promise.resolve();
export function invalidateResources(changes: Dependency[], reset = false): Promise<void> {
  const work = async () => {
    const predicate = (q: {meta?: Record<string,unknown>}) => reset || affected(q.meta?.dependencies as Dependency[] ?? [], changes);
    await client.cancelQueries({predicate});
    await client.invalidateQueries({predicate,refetchType:"none"});
    version++;
    for (const signal of signals) if (reset || affected(signal.deps,changes)) {
      signal.version = version; signal.listeners.forEach(notify => notify());
    }
  };
  invalidations = invalidations.then(work,work);
  return invalidations;
}
// Explicit retry refreshes only these query families; it does not broadcast a
// fictitious domain mutation or reset other features' interaction state.
export function expireQueries(names: string[], scope?: Dependency[]): Promise<void> {
  const work = async () => {
    const predicate = (q: {queryKey: readonly unknown[]; meta?: Record<string, unknown>}) => names.includes(String(q.queryKey[0])) &&
      (!scope || affected(q.meta?.dependencies as Dependency[] ?? [], scope));
    await client.cancelQueries({predicate});
    await client.invalidateQueries({predicate,refetchType:"none"});
  };
  invalidations = invalidations.then(work,work);
  return invalidations;
}
let cursor: Cursor | null = null, pending: Promise<void> | undefined, again = false;
export function syncResources(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  again = true;
  if (!pending) pending = (async () => {
    while (again) {
      again = false;
      const result = await invoke<Changes>("library_changes",{cursor});
      if (result.reset || result.changes.length) await invalidateResources(result.changes,result.reset);
      cursor = result.cursor;
    }
  })().finally(() => { pending = undefined; });
  return pending;
}
export function useResourceBridge(onError: (error: unknown) => void) {
  useEffect(() => {
    if (!isTauri()) return;
    let active = true, failures = 0;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const sync = () => {
      clearTimeout(retry);
      void syncResources().then(() => { failures = 0; }).catch(error => {
        if (!active) return;
        failures++;
        if (failures === 3) onError(error);
        // The wake-up hint may already be consumed. Retry from the unchanged
        // durable cursor even if there are no further commits or focus events.
        retry = setTimeout(sync, Math.min(5000, 250 * 2 ** Math.min(failures, 5)));
      });
    };
    const off = Promise.all([
      listen("resources-changed",sync),
      listen("settings-changed",() => { void invalidateResources([{domain:"settings"}]); }),
    ]);
    // Register first, then catch up; events are hints, the cursor is authoritative.
    void off.then(() => { if(active) sync(); }).catch(error => { if(active) onError(error); });
    window.addEventListener("focus",sync);
    return () => { active=false; clearTimeout(retry); window.removeEventListener("focus",sync); void off.then(stops=>stops.forEach(stop=>stop())).catch(() => {}); };
  }, []);
}
const mutations = new Set([
  "library_capture","library_edit","library_action","library_rebuild",
  "discussion_open","discussion_submit","discussion_retry","discussion_cancel","discussion_undo","discussion_save_text",
  "organization_retry","organization_collect","organization_dismiss","cleanup_save",
  "navigation_pin","navigation_collect","navigation_save_collection","navigation_archive_collection",
]);
export async function resourceCall<T>(name: string, args?: Record<string,unknown>): Promise<T> {
  const deps = dependencies(name,args);
  if (deps) return readResource(name,args,deps);
  try { return await invoke<T>(name,args); }
  finally {
    // A commit may have succeeded even when its IPC acknowledgement failed.
    if (mutations.has(name)) await syncResources().catch(() => {});
  }
}
