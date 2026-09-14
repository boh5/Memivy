import { useEffect, useRef, useState } from "react";
import { useTranslation } from 'react-i18next';
import i18n from '../i18n';
import { message } from '../i18n/messages';
import { useNotice } from '../i18n/react';
import type { CrepeBuilder } from "@milkdown/crepe/builder";
import "@milkdown/crepe/theme/common/prosemirror.css";
import { createRoot } from "react-dom/client";
import FormattingToolbar from "./FormattingToolbar";
import Markdown from "./Markdown";
import type { Format } from "./editorFormatting";
import "@milkdown/crepe/theme/common/link-tooltip.css";
import "@milkdown/crepe/theme/common/table.css";
import "./markdown.css";

type Props = { value: string; onChange: (value: string) => void; label: string; disabled?: boolean; autoFocus?: boolean; onSave?: () => void };

export default function MarkdownEditor(props: Props) {
  const { t } = useTranslation('editor');
  const { value, label, disabled = false, autoFocus = false } = props;
  const root = useRef<HTMLDivElement>(null), instance = useRef<CrepeBuilder | null>(null);
  const applied = useRef(value), sync = useRef<((value: string) => void) | null>(null), replacing = useRef(false);
  const composing = useRef(false);
  const latest = useRef(props); latest.current = props;
  const [ready, setReady] = useState(false), [error, setError] = useNotice(), [attempt, setAttempt] = useState(0);
  const updateLabels = useRef<(() => void) | null>(null);
  // Typing must never recreate the editor or replace its selection.
  useEffect(() => {
    if (!root.current) return;
    let cancelled = false;
    const host = root.current;
    let editor: CrepeBuilder | undefined;
    let created = false;
    let stopLabels: (() => void) | undefined;
    let stage = "resources";
    setReady(false); setError("");
    void (async () => {
      const [{ CrepeBuilder }, { TooltipProvider }, { linkTooltip }, core, { $prose, replaceAll }, { Plugin }, formatting, link, { table }, extensions] = await Promise.all([
        import("@milkdown/crepe/builder"), import("@milkdown/kit/plugin/tooltip"),
        import("@milkdown/crepe/feature/link-tooltip"), import("@milkdown/kit/core"),
        import("@milkdown/kit/utils"), import("@milkdown/kit/prose/state"), import("./editorFormatting"), import("@milkdown/kit/component/link-tooltip"),
        import("@milkdown/crepe/feature/table"), import("./editorExtensions"),
      ]);
      if (cancelled) return;
      stage = "initialize";
      applied.current = latest.current.value;
      editor = new CrepeBuilder({ root: host, defaultValue: latest.current.value })
        .addFeature(linkTooltip, { inputPlaceholder: i18n.t('linkPlaceholder', { ns: 'editor' }) })
        .addFeature(table, extensions.tableFeatureConfig);
      editor.editor.use([extensions.underlineSchema, extensions.underlineMarkdown, extensions.taskListInput, extensions.numberedListInput, extensions.tableInput, extensions.editorShortcuts, extensions.editorListView, extensions.safeImageView].flat());
      editor.editor.config(ctx => ctx.update(core.editorViewOptionsCtx, prev => ({
        ...prev,
        attributes: () => ({ role: "textbox", "aria-label": latest.current.label, "aria-multiline": "true", spellcheck: "false", "data-placeholder": i18n.t('emptyPlaceholder', { ns: 'editor' }) }),
        handlePaste: (_view, event) => Array.from(event.clipboardData?.files || []).length > 0,
        handleDrop: (_view, event) => Array.from(event.dataTransfer?.files || []).length > 0,
        handleClickOn: (_view, _pos, _node, _nodePos, event) => { if ((event.target as Element).closest("a")) { event.preventDefault(); return true; } return false; },
      })));
      // Listener.markdownUpdated is delayed; publish directly from document updates
      // so immediate Save, navigation and app-close flush include the last keystroke.
      editor.editor.use($prose(ctx => new Plugin({ view: view => {
        const content = document.createElement("div");
        content.className = "memivy-selection-tools";
        const toolbarRoot = createRoot(content);
        const provider = new TooltipProvider({ content, debounce: 20, offset: 8, shouldShow: view =>
          view.editable && !view.state.selection.empty && !!view.state.doc.textBetween(view.state.selection.from, view.state.selection.to) &&
          (view.hasFocus() || content.contains(document.activeElement)) });
        // Focus changes need not dispatch a ProseMirror transaction. Refresh
        // explicitly, after keyboard focus has reached a toolbar button.
        const updateFocus = () => queueMicrotask(() => { if (!cancelled && !view.isDestroyed) provider.update(view); });
        host.addEventListener("focusin", updateFocus);
        host.addEventListener("focusout", updateFocus);
        const apply = (format: Format) => {
          if (cancelled || latest.current.disabled || view.composing) return;
          if (format === "link") { ctx.get(core.commandsCtx).call(link.toggleLinkCommand.key); }
          else formatting.applyFormat(view, format);
        };
        const refresh = () => {
          const state = formatting.readFormat(view.state);
          toolbarRoot.render(<FormattingToolbar state={state} onFormat={apply} />);
        };
        refresh(); provider.update(view);
        return { update(view, previous) {
          if (cancelled) return;
          refresh(); provider.update(view);
          if (!created || replacing.current || view.state.doc.eq(previous.doc)) return;
          const markdown = ctx.get(core.serializerCtx)(view.state.doc);
          applied.current = markdown;
          if (markdown !== latest.current.value) latest.current.onChange(markdown);
        }, destroy() {
          host.removeEventListener("focusin", updateFocus);
          host.removeEventListener("focusout", updateFocus);
          provider.destroy(); content.remove();
          // React may be disposing the enclosing editor in the same commit.
          queueMicrotask(() => toolbarRoot.unmount());
        } };
      } })));
      await editor.create();
      if (cancelled) { await editor.destroy(); return; }
      created = true;
      instance.current = editor;
      const localize = () => {
        if (cancelled || !editor) return;
        editor.editor.action(ctx => {
          const view = ctx.get(core.editorViewCtx);
          // Attribute-only updates avoid a transaction during IME composition.
          view.dom.setAttribute('aria-label', latest.current.label);
          view.dom.setAttribute('data-placeholder', i18n.t('emptyPlaceholder', { ns: 'editor' }));
          ctx.update(link.linkTooltipConfig.key, previous => ({ ...previous, inputPlaceholder: i18n.t('linkPlaceholder', { ns: 'editor' }) }));
        });
        extensions.localizeEditorDom(host);
      };
      updateLabels.current = localize;
      i18n.on('languageChanged', localize);
      const observer = new MutationObserver(() => extensions.localizeEditorDom(host));
      observer.observe(host, { childList: true, subtree: true });
      stopLabels = () => { i18n.off('languageChanged', localize); observer.disconnect(); };
      localize();
      sync.current = value => { replacing.current = true; try { editor!.editor.action(replaceAll(value, true)); applied.current = value; } finally { replacing.current = false; } };
      editor.setReadonly(!!latest.current.disabled);
      setReady(true);
      if (autoFocus) editor.editor.action(ctx => ctx.get(core.editorViewCtx).focus());
    })().catch(async () => {
      if (!cancelled) {
        instance.current = null; sync.current = null;
        setReady(false);
        // Do not log the exception message: parser errors can contain memory text.
        console.warn(`[Memivy editor] ${stage} failed`);
        setError(message('editor', stage === 'resources' ? 'resourceError' : 'contentError'));
      }
      try { await editor?.destroy(); } catch { /* A partially created editor may already be disposed. */ }
    });
    return () => { cancelled = true; stopLabels?.(); updateLabels.current = null; instance.current = null; sync.current = null; if (created && editor) void editor.destroy(); };
  }, [attempt]);
  useEffect(() => { updateLabels.current?.(); }, [label, ready]);
  useEffect(() => { if (ready && !composing.current && value !== applied.current) sync.current?.(value); }, [value, ready]);
  useEffect(() => { instance.current?.setReadonly(disabled); }, [disabled, ready]);
  return <div className="markdown-editor" onCompositionStart={() => { composing.current=true; }} onCompositionEnd={() => {
    composing.current=false;
    // ProseMirror publishes the final composition transaction through onChange.
    // Do not replay an older controlled value before that transaction settles.
  }} onKeyDown={e => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s" && !e.nativeEvent.isComposing) {
      e.preventDefault(); if (!disabled && ready) latest.current.onSave?.();
    }
  }}>
    <div className="markdown-editor-heading"><span>{label}</span></div>
    {error && <p className="field-help" role="alert">{error} <button type="button" className="quiet" onClick={() => setAttempt(n => n + 1)}>{t('retry')}</button></p>}
    <div key={attempt} ref={root} className="markdown-editor-host" hidden={!!error} aria-busy={!ready && !error} />
    {error && <section aria-label={t('preview')}><Markdown text={value} /></section>}
    {!ready && !error && <p className="field-help">{t('preparing')}</p>}
    <div className="markdown-editor-hint"><span className="markdown-hint-dot" />{t('hint')}</div>
  </div>;
}
