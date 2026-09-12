/** Apply only to the recording's draft revision; another editor always wins. */
export function voiceBody(current: string, previous: string, base: string, incoming: string): string {
  if (current === incoming) return current;
  if (current !== previous && current !== base) throw { code: 'voice_draft_conflict' };
  return incoming;
}
