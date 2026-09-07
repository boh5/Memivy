import type { Draft } from "./api";

type Call = (name: string, args: Record<string, unknown>) => Promise<unknown>;
type Snapshot = { draft: Draft | null; saved: boolean; error: unknown };
type Entry = {
  current: Draft | null;
  pending: Draft | null;
  revision: number;
  tail: Promise<unknown>;
  error: unknown;
  listeners: Set<(snapshot: Snapshot) => void>;
};

// Each record owns its queue across component mounts. Other records can still
// save if one draft is invalid. Persistence continues through MemoryStore IPC.
export class DraftQueue {
  private entries = new Map<string, Entry>();
  private call: Call;
  constructor(call: Call) {
    this.call = call;
  }
  private entry(key: string) {
    let entry = this.entries.get(key);
    if (!entry) {
      entry = {
        current: null,
        pending: null,
        revision: 0,
        tail: Promise.resolve(),
        error: null,
        listeners: new Set(),
      };
      this.entries.set(key, entry);
    }
    return entry;
  }
  private notify(entry: Entry) {
    const snapshot = {
      draft: entry.current,
      saved: !entry.pending,
      error: entry.error,
    };
    entry.listeners.forEach((listener) => listener(snapshot));
  }
  private enqueue<T>(entry: Entry, work: () => Promise<T>): Promise<T> {
    const operation = entry.tail.then(work);
    entry.tail = operation.catch(() => {});
    return operation;
  }
  subscribe(key: string, listener: (snapshot: Snapshot) => void) {
    const entry = this.entry(key);
    entry.listeners.add(listener);
    return () => {
      entry.listeners.delete(listener);
    };
  }
  read(key: string) {
    const entry = this.entry(key);
    return this.enqueue(entry, async () => {
      if (!entry.pending) {
        const revision = entry.revision;
        const draft = (await this.call("draft_read", { key })) as Draft | null;
        // An edit made during the read is newer than that read's snapshot.
        if (entry.revision === revision) entry.current = draft;
      }
      this.notify(entry);
    });
  }
  private async persist(entry: Entry, draft: Draft) {
    try {
      await this.call("draft_write", { draft });
      if (entry.pending === draft) {
        entry.pending = null;
        entry.error = null;
      }
    } catch (error) {
      if (entry.current === draft) entry.error = error;
      throw error;
    } finally {
      this.notify(entry);
    }
  }
  write(draft: Draft) {
    const entry = this.entry(draft.key);
    entry.current = entry.pending = draft;
    entry.revision++;
    entry.error = null;
    this.notify(entry);
    return this.enqueue(entry, () => this.persist(entry, draft));
  }
  async flush(key: string) {
    const entry = this.entry(key);
    do {
      await this.enqueue(entry, async () => {
        if (entry.pending) await this.persist(entry, entry.pending);
      });
    } while (entry.pending);
  }
  async flushAll() {
    do {
      await Promise.all([...this.entries.keys()].map((key) => this.flush(key)));
    } while ([...this.entries.values()].some((entry) => entry.pending));
  }
  consume(key: string, requestId: string, replacement: Draft | null) {
    const entry = this.entry(key);
    return this.enqueue(entry, async () => {
      const submitted = entry.current;
      if (!submitted || submitted.request_id !== requestId) return false;
      // Ordered with reads/writes from every mount of this key.
      if (replacement) await this.call("draft_write", { draft: replacement });
      else await this.call("draft_clear", { key });
      // An edit can arrive during IPC; its queued write must win.
      if (entry.current === submitted) {
        entry.current = replacement;
        entry.pending = null;
        entry.error = null;
        entry.revision++;
        this.notify(entry);
      }
      return true;
    });
  }
}
