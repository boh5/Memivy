import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type Draft } from "./api";
import { DraftQueue } from "./draftQueue";

const drafts = new DraftQueue(call);
export const flushDrafts = () => drafts.flushAll();

export function useDraft(
  key: string,
  initial: Omit<Draft, "key" | "request_id">,
) {
  const empty = useRef<Draft>({ ...initial, key, request_id: uid() });
  const [value, setValue] = useState(empty.current);
  const [ready, setReady] = useState(false),
    [saved, setSaved] = useState(true),
    [error, setError] = useState("");
  const current = useRef(value);
  useEffect(() => {
    let alive = true;
    const unsubscribe = drafts.subscribe(key, (snapshot) => {
      current.current = snapshot.draft || empty.current;
      setValue(current.current);
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
    const next = { ...current.current, ...patch, key, request_id: uid() };
    // Publish to every current mount and retain failures for retry.
    void drafts.write(next).catch(() => {});
  }
  async function flush() {
    await drafts.flush(key);
    return current.current;
  }
  async function clear(
    requestId = current.current.request_id,
    keepContext = false,
  ) {
    const replacement = keepContext
      ? { ...current.current, body: "", request_id: uid() }
      : null;
    return drafts.consume(key, requestId, replacement);
  }
  return { value, ready, saved, error, update, flush, clear };
}
