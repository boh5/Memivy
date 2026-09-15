import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { useNotice } from "../i18n/react";
import { call, errorText, sourceName, type Key, type Row } from "./api";
import { ErrorNotice } from "./components";
import Select from "./Select";
import { expireQueries, useResourceVersion } from "./resources";
import { activityLevel, calendarDays, dayBounds, dayKey, localDay, streaks, type ActivitySummary } from "./activityCalendar";
import "./activity.css";

type Records = { items: (Row & {id: string})[]; next_offset: number | null };
const clockKey = () => `${dayKey(new Date())}|${Intl.DateTimeFormat().resolvedOptions().timeZone}|${new Date().getTimezoneOffset()}`;

export default function Activity({ onOpenRecord }: { onOpenRecord: (key: Key) => void }) {
  const { t, i18n } = useTranslation("workspace");
  const revision = useResourceVersion([{domain: "memory"}]);
  const [clock, setClock] = useState(clockKey), [reload, setReload] = useState(0);
  const today = clock.split("|")[0];
  const [summary, setSummary] = useState<ActivitySummary | null>(null), [error, setError] = useNotice();
  const [period, setPeriod] = useState("recent"), [selected, setSelected] = useState<string | null>(null);
  const [hovered, setHovered] = useState<string | null>(null), [focused, setFocused] = useState(today);
  const [records, setRecords] = useState<Records>({items: [], next_offset: null});
  const [offset, setOffset] = useState(0), [loading, setLoading] = useState(false), [recordError, setRecordError] = useNotice();
  const grid = useRef<HTMLDivElement>(null), chart = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const viewport = chart.current;
    if (!viewport) return;
    const showRecent = () => {
      const current = grid.current?.querySelector<HTMLElement>(".today");
      // A current-year grid includes empty future columns; reveal today, not
      // the far-right December edge, when the viewport becomes narrower.
      if (current) viewport.scrollLeft += current.getBoundingClientRect().right - viewport.getBoundingClientRect().right + 3;
      else viewport.scrollLeft = viewport.scrollWidth - viewport.clientWidth;
    };
    showRecent();
    const observer = new ResizeObserver(showRecent);
    observer.observe(viewport);
    return () => observer.disconnect();
  }, [period, today]);
  useEffect(() => {
    const update = () => setClock(clockKey());
    const timer = setInterval(update, 30_000);
    window.addEventListener("focus", update);
    return () => { clearInterval(timer); window.removeEventListener("focus", update); };
  }, []);
  useEffect(() => {
    let alive = true;
    setError("");
    // The calendar can change without a storage mutation (midnight or timezone).
    void expireQueries(["activity_summary"]).then(() => call<ActivitySummary>("activity_summary"))
      .then(value => { if (alive) setSummary(value); }).catch(e => { if (alive) setError(errorText(e)); });
    return () => { alive = false; };
  }, [revision, clock, reload]);
  useEffect(() => { setOffset(0); }, [selected, revision, clock]);
  useEffect(() => {
    if (!selected) return;
    let alive = true;
    setLoading(true); setRecordError("");
    // A page replaces the prior page so a mutation cannot duplicate shifted rows.
    setRecords({items: [], next_offset: null});
    void expireQueries(["activity_records"]).then(() => call<Records>("activity_records", {...dayBounds(selected), offset}))
      .then(value => { if (alive) setRecords(value); }).catch(e => { if (alive) setRecordError(errorText(e)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [selected, offset, revision, clock, reload]);
  const days = useMemo(() => new Map(summary?.days.map(day => [day.date, day.count]) ?? []), [summary]);
  const cells = useMemo(() => calendarDays(period, today), [period, today]);
  const years = useMemo(() => {
    const current = localDay(today).getFullYear();
    const first = Math.min(current, Number(summary?.days[0]?.date.slice(0, 4) ?? current));
    return Array.from({length: current - first + 1}, (_, index) => String(current - index));
  }, [summary, today]);
  const streak = streaks(summary?.days ?? [], today);
  const locale = i18n.resolvedLanguage;
  const formatDay = (date: string) => localDay(date).toLocaleDateString(locale, {year: "numeric", month: "long", day: "numeric"});
  const description = (date: string) => t("activity.dayCount", {date: formatDay(date), count: days.get(date) ?? 0});
  const visible = cells.filter(cell => cell.visible);
  const tabDate = visible.some(cell => cell.date === focused) ? focused : visible[visible.length - 1].date;
  const activeDays = visible.filter(cell => days.has(cell.date)).length;
  const total = visible.reduce((sum, cell) => sum + (days.get(cell.date) ?? 0), 0);
  const inspect = hovered ?? selected;
  const metrics = [
    ["memories", summary?.memory_count], ["days", summary?.days.length],
    ["current", summary ? streak.current : undefined], ["longest", summary ? streak.longest : undefined],
  ] as const;
  return <section className="activity-page" aria-label={t("activity.title")}>
    <div className="activity-content">
      <header className="activity-heading"><div><span className="eyebrow">{t("activity.eyebrow")}</span><h1>{t("activity.title")}</h1><p>{t("activity.subtitle")}</p></div><span className="activity-mark" aria-hidden="true"><Icon name="activity" size={26} /></span></header>
      <ErrorNotice text={error} />
      {error && <button className="text-button" onClick={() => setReload(value => value + 1)}>{t("activity.retry")}</button>}
      <dl className="activity-metrics">{metrics.map(([label, value]) => <div key={label}><dt>{t(`activity.${label}`)}</dt><dd>{value === undefined ? "—" : value.toLocaleString(locale)}{label !== "memories" && <small>{t("activity.dayUnit", {count: value ?? 0})}</small>}</dd></div>)}</dl>
      <section className="activity-calendar" aria-label={t("activity.calendar")} aria-busy={!summary && !error}>
        <header><div><h2>{t("activity.calendar")}</h2><p>{summary ? t("activity.periodCount", {count: total, days: t("activity.dayTotal", {count: activeDays})}) : t("activity.loading")}</p></div>
          <Select value={period} aria-label={t("activity.period")} onChange={event => {setPeriod(event.target.value); setSelected(null); setHovered(null); setOffset(0);}}>
            <option value="recent">{t("activity.recentYear")}</option>{years.map(year => <option key={year} value={year}>{year}</option>)}
          </Select>
        </header>
        <div className="activity-calendar-scroll" ref={chart}>
          <div className="activity-calendar-body" style={{"--weeks": cells.length / 7} as React.CSSProperties}>
            <div className="activity-months" aria-hidden="true">{cells.filter((_, index) => index % 7 === 0).map((cell, index) => {
              const date = localDay(index === 0 ? visible[0].date : cell.date);
              const firstMonth = localDay(visible[0].date).getMonth();
              return <span key={cell.date} style={{gridColumn: index + 1}}>{(index === 0 || (date.getDate() <= 7 && (index > 1 || date.getMonth() !== firstMonth))) && index <= cells.length / 7 - 2 ? date.toLocaleDateString(locale, {month: "short"}) : ""}</span>;
            })}</div>
            <div className="activity-weekdays" aria-hidden="true">{[0, 1, 2, 3, 4, 5, 6].map(day => <span key={day}>{day % 2 === 1 ? new Date(2026, 0, 4 + day).toLocaleDateString(locale, {weekday: "short"}) : ""}</span>)}</div>
            <div className="activity-grid" ref={grid} onMouseLeave={() => setHovered(null)}>{cells.map(cell => cell.visible ? <button key={cell.date} type="button" data-date={cell.date} data-level={activityLevel(days.get(cell.date) ?? 0)} className={`activity-cell${cell.date === today ? " today" : ""}${selected === cell.date ? " selected" : ""}`}
              aria-label={description(cell.date)} aria-pressed={selected === cell.date} title={description(cell.date)} tabIndex={cell.date === tabDate ? 0 : -1}
              onMouseEnter={() => setHovered(cell.date)} onFocus={() => {setFocused(cell.date); setHovered(cell.date);}} onBlur={() => setHovered(null)}
              onClick={() => {setOffset(0); setSelected(cell.date);}}
              onKeyDown={event => {
                const shift = {ArrowLeft: -7, ArrowRight: 7, ArrowUp: -1, ArrowDown: 1}[event.key];
                if (shift === undefined && event.key !== "Home" && event.key !== "End") return;
                event.preventDefault();
                const index = visible.findIndex(day => day.date === cell.date);
                const next = event.key === "Home" ? 0 : event.key === "End" ? visible.length - 1 : Math.max(0, Math.min(visible.length - 1, index + shift!));
                grid.current?.querySelector<HTMLButtonElement>(`[data-date="${visible[next].date}"]`)?.focus();
              }} /> : <span key={cell.date} className="activity-cell outside" />)}</div>
          </div>
        </div>
        <footer><span className="activity-inspect">{inspect ? description(inspect) : t("activity.chooseDay")}</span><span className="activity-legend" aria-label={t("activity.legend")}><span>{t("activity.less")}</span>{[0, 1, 2, 3, 4].map(level => <i key={level} data-level={level} />)}<span>{t("activity.more")}</span></span></footer>
      </section>
      <p className="activity-counting-note">{t("activity.countingNote")}</p>
      {summary?.days.length === 0 && !error && <div className="activity-empty"><Icon name="leaf" size={24} /><h2>{t("activity.emptyTitle")}</h2><p>{t("activity.emptyText")}</p></div>}
      {selected && <section className="activity-records" aria-label={formatDay(selected)} aria-busy={loading}>
        <header><h2>{formatDay(selected)}</h2><span>{t("activity.recordCount", {count: days.get(selected) ?? 0})}</span></header>
        <ErrorNotice text={recordError} />
        {recordError && <button className="text-button" onClick={() => setReload(value => value + 1)}>{t("activity.retry")}</button>}
        {loading ? <p role="status" className="activity-record-status">{t("activity.loading")}</p> : !recordError && !records.items.length ? <p className="activity-record-status">{t("activity.noRecords")}</p> : records.items.map(record => <button className="activity-record" key={record.id} onClick={() => onOpenRecord(record.key)}><span className="activity-record-icon"><Icon name="note" size={17} /></span><span><strong>{record.title}</strong><small>{sourceName(record.origin)}<span> · </span>{new Date(record.updated_at).toLocaleTimeString(locale, {hour: "2-digit", minute: "2-digit"})}</small></span><Icon name="chevron" size={14} /></button>)}
        {(offset > 0 || records.next_offset !== null) && <div className="activity-pagination"><button className="text-button" disabled={loading || offset === 0} onClick={() => setOffset(value => Math.max(0, value - 40))}>{t("activity.previous")}</button><button className="text-button" disabled={loading || records.next_offset === null} onClick={() => setOffset(records.next_offset!)}>{t("activity.next")}</button></div>}
      </section>}
    </div>
  </section>;
}
