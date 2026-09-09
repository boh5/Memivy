import { useEffect, useId, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Icon } from "../ui";

/** A named control with the same tooltip for pointer and keyboard users. */
export function ActionTooltip({ label, children, tooltipId }: { label: string; children: ReactNode; tooltipId?: string }) {
  const generatedId = useId();
  const id = tooltipId ?? generatedId;
  const [dismissed, setDismissed] = useState(false);
  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => { if (event.key === "Escape") setDismissed(true); };
    window.addEventListener("keydown", dismiss);
    return () => window.removeEventListener("keydown", dismiss);
  }, []);
  return <span className={`action-tooltip ${dismissed ? "tooltip-dismissed" : ""}`}
    onMouseEnter={() => setDismissed(false)} onFocus={() => setDismissed(false)}>
    {children}<span id={id} role="tooltip">{label}</span>
  </span>;
}
export default function IconButton({ label, icon, className = "", ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; icon: string }) {
  const id = useId();
  return <ActionTooltip label={label} tooltipId={id}><button {...props} aria-describedby={id} className={`detail-icon-button ${className}`} aria-label={label}><Icon name={icon} size={18} /></button></ActionTooltip>;
}
