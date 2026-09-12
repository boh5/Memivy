import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../ui";
import { native } from "./api";

/** Window chrome only: question drafts and RAG remain owned by CaptureForm/App. */
export default function WorkspaceTopBar({ open, preview, scopeLabel, scope, onOpen, onClose, onCapture, triggerRef, children }: {
  open: boolean;
  preview: string;
  scopeLabel?: string;
  scope: ReactNode;
  onOpen: () => void;
  onClose: () => void;
  onCapture: () => void;
  children: ReactNode;
  triggerRef?: RefObject<HTMLButtonElement | null>;
}) {
  const { t } = useTranslation("workspace");
  const query = useRef<HTMLDivElement>(null), localTrigger = useRef<HTMLButtonElement>(null);
  const trigger = triggerRef || localTrigger;
  const composing = useRef(false);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      // Do not consume the click: readers can go straight back to the document.
      if (event.target instanceof Node && !query.current?.contains(event.target)) onClose();
    };
    window.addEventListener("pointerdown", outside);
    return () => window.removeEventListener("pointerdown", outside);
  }, [open, onClose]);
  function dismiss() { onClose(); trigger.current?.focus(); }
  const triggerLabel = preview.trim()
    ? scopeLabel ? t("topbar.searchTriggerScopedDraft", { scope: scopeLabel }) : t("topbar.searchTriggerDraft")
    : scopeLabel ? t("topbar.searchTriggerScoped", { scope: scopeLabel }) : t("topbar.searchTrigger");
  return <header className={`workspace-topbar${native ? " native-titlebar" : ""}`}>
    <div className="titlebar-leading" data-tauri-drag-region aria-hidden="true" />
    <div className="titlebar-center" data-tauri-drag-region>
      <div className="titlebar-query" ref={query}
        onCompositionStart={() => { composing.current = true; }}
        onCompositionEnd={() => { composing.current = false; }}
        onKeyDown={event => {
          if (event.key === "Escape" && !event.nativeEvent.isComposing && !composing.current && event.keyCode !== 229) {
            event.preventDefault(); event.stopPropagation(); dismiss();
          }
        }}
        onBlur={event => {
          if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) onClose();
        }}>
        <button className="recall-trigger" ref={trigger} aria-label={triggerLabel}
          aria-haspopup="dialog" aria-expanded={open} aria-controls="recall-panel" onClick={onOpen}>
          <Icon name="search" size={16} />
          {scopeLabel && <span className="recall-scope-badge" title={scopeLabel}>{scopeLabel}</span>}
          <span className="recall-trigger-text">{preview.trim() || t("topbar.placeholder")}</span>
          {preview.trim() && <span className="recall-draft-label">{t("topbar.draft")}</span>}
          <kbd>⌘ K</kbd>
        </button>
        {/* Keep the form mounted: dismissal must not interrupt a draft write or submission. */}
        <div id="recall-panel" className="recall-panel" role="dialog" aria-label={t("topbar.panelAria")} hidden={!open}>
          <div className="recall-panel-heading">
            <span><Icon name="spark" size={14} />{t("topbar.panelHeading")}</span>
            <button className="recall-dismiss" onClick={dismiss} aria-label={t("topbar.dismiss")} title={t("topbar.dismissTitle")}><Icon name="close" size={14} /></button>
          </div>
          {scope}
          {children}
        </div>
      </div>
    </div>
    <div className="titlebar-actions" data-tauri-drag-region>
      <button className="new-capture-button" onClick={onCapture} aria-haspopup="dialog">
        <Icon name="plus" size={15} />{t("topbar.newCapture")}<kbd>⌘ N</kbd>
      </button>
    </div>
  </header>;
}
