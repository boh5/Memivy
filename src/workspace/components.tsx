import { useEffect, useRef, type ReactNode } from "react";
import { Icon } from "../ui";

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
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropPointer = useRef<number | null>(null);
  useEffect(() => {
    const previous = document.activeElement;
    const element = dialog.current;
    element?.showModal();
    return () => {
      element?.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
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
          aria-label={`关闭${title}`}
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
  const menu = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    const close = (e: PointerEvent) => {
      if (!menu.current?.contains(e.target as Node))
        menu.current?.removeAttribute("open");
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, []);
  return (
    <details
      className="record-more"
      ref={menu}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          menu.current?.removeAttribute("open");
          menu.current?.querySelector("summary")?.focus();
        }
      }}
    >
      <summary aria-label="更多操作" title="更多操作">
        •••
      </summary>
      <div
        className="record-more-actions"
        onClick={(e) => {
          if ((e.target as HTMLElement).closest("button:not(:disabled)"))
            menu.current?.removeAttribute("open");
        }}
      >
        {children}
      </div>
    </details>
  );
}
