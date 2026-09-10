import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type Draft } from "./api";
import { DraftQueue } from "./draftQueue";

const drafts = new DraftQueue(call);
export const flushDrafts = () => drafts.flushAll();
export const flushDraft = (key: string) => drafts.flush(key);
export const refreshDrafts = () => drafts.refresh();

export function useDraft(
  key: string,
  initial: Omit<Draft, "key" | "request_id">,
) {
  const empty = useRef<Draft>({ ...initial, key, request_id: uid() });
  // Defaults belong to the current capture session. A real draft, including one
  // whose body was erased, keeps its own origin until it is consumed.
  empty.current = { ...initial, key, request_id: empty.current.request_id };
  const [draft, setDraft] = useState<Draft | null>(null);
  const [ready, setReady] = useState(false),
    [saved, setSaved] = useState(true),
    [error, setError] = useState("");
  const current = useRef<Draft | null>(null);
  useEffect(() => {
    let alive = true;
    setReady(false);
    const unsubscribe = drafts.subscribe(key, (snapshot) => {
      current.current = snapshot.draft;
      setDraft(current.current);
      setSaved(snapshot.saved);
      setError(snapshot.error ? errorText(snapshot.error) : "");
    });
    void drafts.read(key)
      .then(() => {
        if (alive) setReady(true);
      })
      .catch((e) => {
        if (alive) setError(errorText(e));
      });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [key]);
  function update(patch: Partial<Draft>) {
    const next = { ...(current.current || empty.current), ...patch, key, request_id: uid() };
    // Publish to every current mount and retain failures for retry.
    void drafts.write(next).catch(() => {});
  }
  async function flush(persistInitial = false) {
    if (persistInitial && !current.current) await drafts.write(empty.current);
    await drafts.flush(key);
    return current.current || empty.current;
  }
  async function clear(
    requestId = (current.current || empty.current).request_id,
    keepContext = false,
  ) {
    const replacement = keepContext
      ? { ...(current.current || empty.current), body: "", request_id: uid() }
      : null;
    return drafts.consume(key, requestId, replacement);
  }
  return { value: draft || empty.current, ready, saved, error, update, flush, clear,
    resolve: (keepLocal: boolean, expected: string | null) => drafts.resolve(key, keepLocal, expected) };
}
