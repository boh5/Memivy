import { useEffect, useRef, useState, type PointerEvent } from "react";
import { useTranslation } from "react-i18next";

type Pane = "sidebar" | "list";
const limits = { sidebar: { min: 164, max: 280, initial: 204 }, list: { min: 230, max: 380, initial: 276 } };

export function readPaneWidth(pane: Pane) {
  const range = limits[pane];
  try {
    const saved = Number(window.localStorage.getItem(`memivy.${pane}-width`));
    if (Number.isFinite(saved) && saved >= range.min && saved <= range.max) return saved;
  } catch { /* Window layout remains usable when preferences cannot be read. */ }
  return range.initial;
}

// Drag updates only a CSS custom property; editor and list trees do not rerender.
export default function PaneResizeHandle({ pane, targetKey }: { pane: Pane; targetKey?: string }) {
  const { t } = useTranslation("workspace");
  const range = limits[pane];
  const [width, setWidth] = useState(() => readPaneWidth(pane));
  const [maximum, setMaximum] = useState(range.max);
  const handle = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; width: number; next: number } | null>(null);
  const property = `--preferred-${pane}-width`;
  function apply(value: number, persist = false) {
    const root = handle.current?.closest<HTMLElement>(".memory-app");
    const available = pane === "sidebar" ? (root?.clientWidth || 0) * .24 : (root?.querySelector<HTMLElement>(".library-layout")?.clientWidth || 0) * .34;
    const upper = available ? Math.max(range.min, Math.min(range.max, Math.floor(available))) : range.max;
    const next = Math.round(Math.min(upper, Math.max(range.min, value)));
    root?.style.setProperty(property, `${next}px`);
    if (persist) {
      setWidth(next);
      try { window.localStorage.setItem(`memivy.${pane}-width`, String(next)); }
      catch { /* A layout preference never blocks reading or editing. */ }
    }
    return next;
  }
  useEffect(() => {
    const root = handle.current?.closest<HTMLElement>(".memory-app");
    // Preserve the preferred width; CSS clamps it to the current window.
    root?.style.setProperty(property, `${readPaneWidth(pane)}px`);
    const target = root?.querySelector<HTMLElement>(pane === "sidebar" ? ".sidebar" : ".library-list-pane");
    if (!target || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      const actual = target.getBoundingClientRect().width;
      if (!actual || drag.current) return;
      setWidth(Math.round(actual));
      const available = pane === "sidebar" ? root!.clientWidth * .24 : (root!.querySelector<HTMLElement>(".library-layout")?.clientWidth || 0) * .34;
      setMaximum(Math.max(range.min, Math.min(range.max, Math.floor(available))));
    });
    observer.observe(target);
    if (root) observer.observe(root);
    const layout = root?.querySelector(".library-layout");
    if (layout) observer.observe(layout);
    return () => observer.disconnect();
  }, [pane, targetKey]);
  function finish(event: PointerEvent<HTMLDivElement>, cancelled = false) {
    if (!drag.current) return;
    apply(cancelled ? drag.current.width : drag.current.next, true);
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  }
  return <div ref={handle} className={`pane-resizer ${pane}-resizer`} role="separator" tabIndex={0}
    aria-label={t(pane === "sidebar" ? "nav.resizeSidebar" : "nav.resizeList")}
    aria-orientation="vertical" aria-valuemin={range.min} aria-valuemax={maximum} aria-valuenow={width}
    onPointerDown={event => {
      if (event.button !== 0) return;
      const root = event.currentTarget.closest(".memory-app");
      const target = root?.querySelector(pane === "sidebar" ? ".sidebar" : ".library-list-pane");
      const actual = target?.getBoundingClientRect().width || width;
      event.preventDefault(); event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { x: event.clientX, width: actual, next: actual };
    }}
    onPointerMove={event => {
      if (drag.current) drag.current.next = apply(drag.current.width + event.clientX - drag.current.x);
    }}
    onPointerUp={event => finish(event)} onPointerCancel={event => finish(event, true)}
    onLostPointerCapture={() => { if (drag.current) { apply(drag.current.next, true); drag.current = null; } }}
    onDoubleClick={() => apply(range.initial, true)}
    onKeyDown={event => {
      if (event.nativeEvent.isComposing || event.keyCode === 229) return;
      const next = event.key === "ArrowLeft" ? width - 10 : event.key === "ArrowRight" ? width + 10 : event.key === "Home" ? range.min : event.key === "End" ? range.max : null;
      if (next !== null) { event.preventDefault(); apply(next, true); }
    }} />;
}
