export type DiffPart = { kind: "same" | "remove" | "add"; text: string };
/** Bounded lookahead, never quadratic in document length. Coalesce adjacent spans. */
export function cleanupDiff(before: string, after: string): DiffPart[] {
  if (before === after) return [{ kind: "same", text: before }];
  const a = before.split(/(?<=\n)/), b = after.split(/(?<=\n)/);
  // Pathological one-character lines still render in a bounded number of DOM nodes.
  if (a.length + b.length > 4000) return [{ kind: "remove", text: before }, { kind: "add", text: after }];
  const result: DiffPart[] = [];
  const push = (kind: DiffPart["kind"], text: string) => {
    if (!text) return;
    const last = result.at(-1);
    if (last?.kind === kind) last.text += text;
    else result.push({ kind, text });
  };
  let i = 0, j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) { push("same", a[i++]); j++; continue; }
    let remove = -1, add = -1;
    for (let d = 1; d <= 32; d++) {
      if (remove < 0 && j < b.length && i + d < a.length && a[i + d] === b[j]) remove = d;
      if (add < 0 && i < a.length && j + d < b.length && b[j + d] === a[i]) add = d;
    }
    if (remove > 0 && (add < 0 || remove <= add)) { while (remove-- > 0) push("remove", a[i++]); }
    else if (add > 0) { while (add-- > 0) push("add", b[j++]); }
    else { if (i < a.length) push("remove", a[i++]); if (j < b.length) push("add", b[j++]); }
  }
  return result;
}
