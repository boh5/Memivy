export type ActivityDay = { date: string; count: number };
export type ActivitySummary = { memory_count: number; days: ActivityDay[] };

export function dayKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
export function localDay(key: string): Date { return new Date(`${key}T00:00:00`); }
export function moveDay(key: string, offset: number): string {
  const date = localDay(key);
  date.setDate(date.getDate() + offset);
  return dayKey(date);
}
export function dayBounds(key: string) {
  return { since: localDay(key).getTime(), until: localDay(moveDay(key, 1)).getTime() };
}
export function streaks(days: ActivityDay[], today: string) {
  const dates = days.filter(day => day.count > 0 && day.date <= today).map(day => day.date).sort();
  let longest = 0, run = 0, previous = "";
  for (const date of dates) {
    run = previous && moveDay(previous, 1) === date ? run + 1 : 1;
    longest = Math.max(longest, run); previous = date;
  }
  return { longest, current: previous === today || previous === moveDay(today, -1) ? run : 0 };
}
export function calendarDays(period: string, today: string) {
  const start = period === "recent" ? moveDay(today, -364) : `${period}-01-01`;
  // Keep annual grids the same width in January and December. Future days
  // reserve their columns but have no visible or interactive cell.
  const end = period === "recent" ? today : `${period}-12-31`;
  const first = moveDay(start, -localDay(start).getDay());
  const last = moveDay(end, 6 - localDay(end).getDay());
  const cells: {date: string; visible: boolean}[] = [];
  for (let date = first; date <= last; date = moveDay(date, 1)) cells.push({date, visible: date >= start && date <= end && date <= today});
  return cells;
}
export function activityLevel(count: number): number {
  return count === 0 ? 0 : count === 1 ? 1 : count <= 3 ? 2 : count <= 6 ? 3 : 4;
}
