/** Apply only to the recording's draft revision; another editor always wins. */
export function voiceBody(current: string, previous: string, base: string, incoming: string): string {
  if (current === incoming) return current;
  if (current !== previous && current !== base) throw new Error("草稿已在其他位置修改。录音文字已保留，请核对后插入。");
  return incoming;
}
