import type { Draft } from "./api";

type Call = (name: string, args: Record<string, unknown>) => Promise<unknown>;
type Snapshot = { draft: Draft | null; error: unknown };
type Entry = {
  base: string | null;
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
        base: null,
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
        if (entry.revision === revision) {
          entry.base = draft?.request_id || null;
          entry.current = draft;
          entry.error = null;
        }
      }
      this.notify(entry);
    });
  }
  private async persist(entry: Entry, draft: Draft) {
    try {
      const written = await this.call("draft_write", { draft, expectedRequest: entry.base });
      if (written === false) throw DRAFT_CONFLICT;
      entry.base = draft.request_id;
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
    // Keep a failed-save warning visible until a write actually succeeds.
    // Typing is not evidence of recovery.
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
      const consumed = replacement
        ? await this.call("draft_write", { draft: replacement, expectedRequest: entry.base })
        : await this.call("draft_clear", { key, request: requestId });
      if (consumed === false) {
        // The submitted content was saved, but another window now owns a newer draft.
        if (entry.current === submitted) {
          const disk = (await this.call("draft_read", { key })) as Draft | null;
          if (entry.current === submitted) {
            entry.current = disk;
            entry.base = disk?.request_id || null;
            entry.pending = null;
            entry.error = null;
            entry.revision++;
            this.notify(entry);
          }
        }
        return false;
      }
      entry.base = replacement?.request_id || null;
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
  refresh(changedKey?: string) {
    // A keystroke in the other window must not reload every visited editor.
    // Unmounted drafts are read afresh when subscribed again.
    return Promise.all([...this.entries].filter(([key, entry]) =>
      entry.listeners.size > 0 && (changedKey === undefined || key === changedKey)
    ).map(([key]) => this.read(key)));
  }
  resolve(key: string, keepLocal: boolean, expected: string | null) {
    const entry = this.entry(key);
    return this.enqueue(entry, async () => {
      const disk = await this.call("draft_read", { key }) as Draft | null;
      if ((disk?.request_id || null) !== expected) throw DRAFT_CONFLICT;
      entry.base = disk?.request_id || null;
      if (keepLocal && entry.current) await this.persist(entry, entry.current);
      else {
        entry.current = disk;
        entry.pending = null;
        entry.error = null;
        entry.revision++;
        this.notify(entry);
      }
    });
  }
}
export const DRAFT_CONFLICT = Object.freeze({ code: 'draft_conflict' });
