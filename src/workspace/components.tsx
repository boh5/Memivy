import { ActionTooltip, positionActionPopup } from "./IconButton";
import { Children, cloneElement, isValidElement, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Icon } from "../ui";
import { useTranslation } from "react-i18next";

export function Highlight({ text, query }: { text: string; query: string }) {
  const terms = query
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);
  if (!terms.length) return <>{text}</>;
  const regex = new RegExp(
    `(${terms.map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`,
    "ig",
  );
  return (
    <>
      {text.split(regex).map((s, i) => (i % 2 ? <mark key={i}>{s}</mark> : s))}
    </>
  );
}
export function Modal({
  title,
  children,
  onClose,
  className = "",
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  className?: string;
}) {
  const { t } = useTranslation("workspace");
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropPointer = useRef<number | null>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const element = dialog.current;
    element?.showModal();
    // Focus only after entering the top layer; React's child autoFocus runs too early.
    element?.querySelector<HTMLElement>("[data-modal-autofocus]:not(:disabled)")?.focus({ preventScroll: true });
    return () => {
      element?.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus({ preventScroll: true });
    };
  }, []);
  return (
    <dialog
      className={`workspace-dialog ${className}`.trim()}
      ref={dialog}
      aria-label={title}
      onCancel={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onClose();
      }}
      onKeyDown={(e) => {
        if (
          e.key !== "Escape" ||
          e.nativeEvent.isComposing ||
          e.keyCode === 229
        ) {
          return;
        }
        e.preventDefault();
        e.stopPropagation();
        if (!e.repeat) onClose();
      }}
      onPointerDown={(e) => {
        // A dialog's backdrop targets the dialog itself. Its inner padding does too.
        const bounds = e.currentTarget.getBoundingClientRect();
        backdropPointer.current =
          e.target === e.currentTarget &&
          e.button === 0 &&
          (e.clientX < bounds.left ||
            e.clientX > bounds.right ||
            e.clientY < bounds.top ||
            e.clientY > bounds.bottom)
            ? e.pointerId
            : null;
      }}
      onPointerUp={(e) => {
        const startedOutside = backdropPointer.current === e.pointerId;
        backdropPointer.current = null;
        const bounds = e.currentTarget.getBoundingClientRect();
        if (
          startedOutside &&
          e.target === e.currentTarget &&
          (e.clientX < bounds.left ||
            e.clientX > bounds.right ||
            e.clientY < bounds.top ||
            e.clientY > bounds.bottom)
        ) {
          e.stopPropagation();
          onClose();
        }
      }}
      onPointerCancel={() => {
        backdropPointer.current = null;
      }}
    >
      <header>
        <h2>{title}</h2>
        <button
          className="icon-button"
          aria-label={t("components.close", { title })}
          onClick={onClose}
        >
          <Icon name="close" />
        </button>
      </header>
      {children}
    </dialog>
  );
}
export function Empty({
  title,
  text,
  children,
}: {
  title: string;
  text: string;
  children?: ReactNode;
}) {
  return (
    <div className="workspace-empty">
      <Icon name="leaf" size={30} />
      <h2>{title}</h2>
      <p>{text}</p>
      {children}
    </div>
  );
}
export function ErrorNotice({ text }: { text: string }) {
  return text ? (
    <p className="workspace-error" role="alert">
      {text}
    </p>
  ) : null;
}

export function MoreMenu({ children }: { children: ReactNode }) {
  const { t } = useTranslation("workspace");
  const tooltipId = useId(), menuId = useId();
  const trigger = useRef<HTMLButtonElement>(null), menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  function close(focus = false) {
    menu.current?.hidePopover(); setOpen(false);
    if (focus) trigger.current?.focus({ preventScroll: true });
  }
  const items = () => Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") || []);
  function show(last = false) {
    const button = trigger.current, popup = menu.current;
    if (!button || !popup) return;
    button.focus({ preventScroll: true });
    popup.showPopover(); positionActionPopup(button, popup); setOpen(true);
    const buttons = items(); buttons[last ? buttons.length - 1 : 0]?.focus({ preventScroll: true });
  }
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: Event) => {
      if (event.target instanceof Node && menu.current?.contains(event.target)) return;
      close();
    };
    window.addEventListener("resize", dismiss); window.addEventListener("scroll", dismiss, true);
    return () => { window.removeEventListener("resize", dismiss); window.removeEventListener("scroll", dismiss, true); };
  }, [open]);
  return <span className="record-more">
    <ActionTooltip label={t("components.more")} tooltipId={tooltipId} suppressed={open}>
      <button type="button" ref={trigger} className="record-more-trigger" aria-label={t("components.more")} aria-describedby={tooltipId}
        aria-haspopup="menu" aria-expanded={open} aria-controls={menuId}
        onClick={() => open ? close() : show()} onKeyDown={event => {
          if (event.nativeEvent.isComposing) return;
          if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); show(event.key === "ArrowUp"); }
          if (event.key === "Escape" && open) { event.preventDefault(); event.stopPropagation(); close(true); }
        }}><Icon name="ellipsis" size={18} /></button>
    </ActionTooltip>
    <div id={menuId} ref={menu} popover="auto" className="record-more-actions" role="menu" aria-label={t("components.more")}
      onToggle={event => setOpen(event.newState === "open")}
      onKeyDown={event => {
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); return; }
        if (event.key === "Tab") { close(true); return; }
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault(); const buttons = items(), current = buttons.indexOf(document.activeElement as HTMLButtonElement);
          const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
          buttons[next]?.focus();
        }
      }} onClick={event => {
        if ((event.target as HTMLElement).closest("button:not(:disabled)")) { event.stopPropagation(); close(true); }
      }}>
      {Children.map(children, child => isValidElement<{ role?: string; tabIndex?: number }>(child) && child.type === "button" ? cloneElement(child, { role: "menuitem", tabIndex: -1 }) : child)}
    </div>
  </span>;
}
