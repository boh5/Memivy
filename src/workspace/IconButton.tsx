import { useEffect, useId, useRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Icon } from "../ui";
import "./cleanup.css";

/** Position top-layer action surfaces within the current window, including narrow panels. */
export function positionActionPopup(anchor: HTMLElement, popup: HTMLElement, centered = false) {
  const edge = 10, gap = 6, r = anchor.getBoundingClientRect();
  popup.style.maxWidth = `${Math.max(0, window.innerWidth - edge * 2)}px`;
  popup.style.maxHeight = `${Math.max(0, window.innerHeight - edge * 2)}px`;
  const bounds = popup.getBoundingClientRect();
  const below = window.innerHeight - r.bottom - gap - edge, above = r.top - gap - edge;
  const down = bounds.height <= below || below >= above;
  popup.style.maxHeight = `${Math.max(0, down ? below : above)}px`;
  const height = Math.min(bounds.height, Math.max(0, down ? below : above));
  const left = centered ? r.left + (r.width - bounds.width) / 2 : r.right - bounds.width;
  popup.style.left = `${Math.max(edge, Math.min(left, window.innerWidth - bounds.width - edge))}px`;
  popup.style.top = `${Math.max(edge, down ? r.bottom + gap : r.top - gap - height)}px`;
}

/** A named control with the same tooltip for pointer and keyboard users. */
export function ActionTooltip({ label, children, tooltipId, suppressed = false }: { label: string; children: ReactNode; tooltipId?: string; suppressed?: boolean }) {
  const generatedId = useId();
  const id = tooltipId ?? generatedId;
  const anchor = useRef<HTMLSpanElement>(null), popup = useRef<HTMLSpanElement>(null), open = useRef(false);
  function hide() { if (open.current) { popup.current?.hidePopover(); open.current = false; } }
  function show() {
    if (suppressed || !anchor.current || !popup.current) return;
    popup.current.showPopover(); open.current = true;
    positionActionPopup(anchor.current, popup.current, true);
  }
  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => { if (event.key === "Escape") hide(); };
    window.addEventListener("keydown", dismiss);
    window.addEventListener("scroll", hide, true); window.addEventListener("resize", hide);
    return () => { window.removeEventListener("keydown", dismiss); window.removeEventListener("scroll", hide, true); window.removeEventListener("resize", hide); };
  }, []);
  useEffect(() => { if (suppressed) hide(); }, [suppressed]);
  return <span ref={anchor} className="action-tooltip"
    onMouseEnter={show} onMouseLeave={hide} onFocus={show} onBlur={hide} onPointerDown={hide}>
    {children}<span ref={popup} id={id} role="tooltip" popover="manual">{label}</span>
  </span>;
}
export default function IconButton({ label, icon, className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; icon: string }) {
  const id = useId();
  return <ActionTooltip label={label} tooltipId={id}><button {...props} aria-describedby={id} className={`detail-icon-button ${className}`} aria-label={label}><Icon name={icon} size={18} /></button></ActionTooltip>;
}
