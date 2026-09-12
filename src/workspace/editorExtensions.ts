import i18n from "../i18n";
import type editorCatalog from '../../locales/en/editor.json';
import { $inputRule, $markSchema, $prose, $remark, $view } from "@milkdown/kit/utils";
import { InputRule, wrappingInputRule } from "@milkdown/kit/prose/inputrules";
import { Plugin, TextSelection, type Command } from "@milkdown/kit/prose/state";
import { splitListItem } from "@milkdown/kit/prose/schema-list";
import { toggleMark } from "@milkdown/kit/prose/commands";
import { schemaCtx } from "@milkdown/kit/core";
import type { NodeViewConstructor } from "@milkdown/kit/prose/view";
import { imageSchema, listItemSchema } from "@milkdown/kit/preset/commonmark";
import { remarkUnderline } from "./remarkUnderline";
import { insertEditorTable } from "./editorFormatting";

export const underlineSchema = $markSchema("underline", () => ({
  parseDOM: [{ tag: "u" }, { style: "text-decoration", getAttrs: value => String(value).includes("underline") ? null : false }],
  toDOM: () => ["u", 0],
  parseMarkdown: {
    match: node => node.type === "underline",
    runner: (state, node, type) => { state.openMark(type); state.next(node.children); state.closeMark(type); },
  },
  toMarkdown: {
    match: mark => mark.type.name === "underline",
    runner: (state, mark) => { state.withMark(mark, "underline"); },
  },
}));
export const underlineMarkdown = $remark("memivyUnderline", () => remarkUnderline);

// Mem also accepts [] + space directly at the start of a paragraph.
export const createTaskInputRule = () => new InputRule(/^\[( |x|X)?\] $/, (state, match, start, end) => {
  // Run the same conversion as the direct button on one transaction, so one
  // backspace/undo reverts the whole Markdown shortcut.
  const tr = state.tr.delete(start, end);
  const paragraph = tr.doc.resolve(start);
  if (paragraph.parent.type.name !== "paragraph") return null;
  const item = state.schema.nodes.list_item;
  const list = state.schema.nodes.bullet_list;
  if (!item || !list) return null;
  if (paragraph.depth > 1 && paragraph.node(-1).type === item) {
    return tr.setNodeMarkup(paragraph.before(paragraph.depth - 1), undefined, { ...paragraph.node(-1).attrs, checked: /x/i.test(match[1] || "") });
  }
  const pos = paragraph.before();
  if (!paragraph.node(-1).canReplaceWith(paragraph.index(-1), paragraph.index(-1) + 1, list)) return null;
  const node = list.create(null, item.create({ checked: /x/i.test(match[1] || ""), listType: "bullet", label: "•" }, paragraph.parent));
  return tr.replaceWith(pos, pos + paragraph.parent.nodeSize, node).setSelection(TextSelection.create(tr.doc, pos + 3));
});
export const taskListInput = $inputRule(createTaskInputRule);

export const numberedListInput = $inputRule(ctx => wrappingInputRule(/^1 $/, ctx.get(schemaCtx).nodes.ordered_list, { order: 1 }));
export const tableInput = $inputRule(() => new InputRule(/^\| $/, (state, _match, start, end) => {
  const tr = state.tr.delete(start, end);
  const next = state.apply(tr);
  insertEditorTable(next, inserted => {
    inserted.steps.forEach(step => tr.step(step));
    tr.setSelection(TextSelection.near(tr.doc.resolve(inserted.selection.from)));
  });
  return tr;
}));

export const splitTaskListItem: Command = (state, dispatch, view) => {
  const { $from } = state.selection;
  if ($from.depth < 2 || $from.node(-1).type.name !== "list_item" || $from.node(-1).attrs.checked == null) return false;
  return splitListItem(state.schema.nodes.list_item)(state, dispatch && (tr => {
    const pos = tr.selection.$from;
    if (pos.depth >= 2 && pos.node(-1).type.name === "list_item") {
      tr.setNodeMarkup(pos.before(pos.depth - 1), undefined, { ...pos.node(-1).attrs, checked: false });
    }
    dispatch(tr);
  }), view);
};

export const editorShortcuts = $prose(() => new Plugin({ props: {
  handleKeyDown(view, event) {
    if (!view.editable || view.composing || event.isComposing) return false;
    if ((event.metaKey || event.ctrlKey) && !event.altKey && event.key.toLowerCase() === "u") {
      event.preventDefault();
      return toggleMark(view.state.schema.marks.underline)(view.state, view.dispatch);
    }
    // Mem leaves a code block after a third Return at its end.
    const { $from, empty } = view.state.selection;
    if (event.key === "Enter" && !event.shiftKey && !event.metaKey && !event.ctrlKey &&
      splitTaskListItem(view.state, view.dispatch, view)) return true;
    if (event.key === "Enter" && !event.shiftKey && !event.metaKey && !event.ctrlKey && empty &&
      $from.parent.type.name === "code_block" && $from.parentOffset === $from.parent.content.size && $from.parent.textContent.endsWith("\n\n")) {
      const tr = view.state.tr.delete($from.pos - 2, $from.pos);
      const after = tr.mapping.map($from.after());
      tr.insert(after, view.state.schema.nodes.paragraph.create());
      view.dispatch(tr.setSelection(TextSelection.create(tr.doc, after + 1)).scrollIntoView());
      return true;
    }
    return false;
  },
} }));

// Native checkbox semantics also allow keyboard activation; checkbox clicks do
// not disturb the text selection or bypass the editor's document/save flow.
export const listItemView: NodeViewConstructor = (initial, view, getPos) => {
  let node = initial;
  const dom = document.createElement("li");
  const checkbox = document.createElement("input");
  checkbox.type = "checkbox"; checkbox.contentEditable = "false"; checkbox.dataset.editorLabel = "complete"; checkbox.setAttribute("aria-label", i18n.t("complete", { ns: "editor" }));
  const contentDOM = document.createElement("div");
  dom.append(checkbox, contentDOM);
  function refresh() {
    const task = node.attrs.checked != null;
    dom.className = task ? "editor-task-item" : "editor-list-item";
    checkbox.hidden = !task; checkbox.checked = node.attrs.checked === true; checkbox.disabled = !view.editable;
    dom.dataset.checked = String(node.attrs.checked === true);
  }
  checkbox.addEventListener("change", () => {
    const pos = getPos();
    if (!view.editable || view.composing || pos == null) { refresh(); return; }
    view.dispatch(view.state.tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: checkbox.checked }));
    view.focus();
  });
  refresh();
  return { dom, contentDOM, update(next) { if (next.type !== initial.type) return false; node = next; refresh(); return true; },
    stopEvent: event => event.target === checkbox,
    ignoreMutation: mutation => mutation.type !== "selection" && !contentDOM.contains(mutation.target),
  };
};

export const editorListView = $view(listItemSchema.node, () => listItemView);
export const safeImageView = $view(imageSchema.node, () => node => {
  const dom = document.createElement("span");
  dom.className = "markdown-image";
  dom.dataset.editorImageAlt = node.attrs.alt || "";
  dom.textContent = i18n.t("image", { ns: "editor", alt: node.attrs.alt || i18n.t("imageLink", { ns: "editor" }) });
  return { dom };
});

const tableIcon = (key: keyof typeof editorCatalog, path: string) => { const label = i18n.t(key, { ns: "editor" }); return `<svg data-editor-label="${key}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" role="img" aria-label="${label}"><title>${label}</title><path d="${path}" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>`; };
export const tableFeatureConfig = {
  get addRowIcon() { return tableIcon("addRow", "M5 12h14M12 5v14"); },
  get addColIcon() { return tableIcon("addCol", "M5 12h14M12 5v14"); },
  get deleteRowIcon() { return tableIcon("deleteRow", "M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 10v7M14 10v7"); },
  get deleteColIcon() { return tableIcon("deleteCol", "M5 7h14M9 7V4h6v3M7 7l1 13h8l1-13M10 10v7M14 10v7"); },
  get alignLeftIcon() { return tableIcon("alignLeft", "M4 5h16M4 10h10M4 15h16M4 20h10"); },
  get alignCenterIcon() { return tableIcon("alignCenter", "M4 5h16M7 10h10M4 15h16M7 20h10"); },
  get alignRightIcon() { return tableIcon("alignRight", "M4 5h16M10 10h10M4 15h16M10 20h10"); },
};

/** Update presentation DOM only; never dispatch a document or selection transaction. */
export function localizeEditorDom(host: HTMLElement) {
  for (const node of host.querySelectorAll<HTMLElement>('[data-editor-label]')) {
    const label = i18n.t(node.dataset.editorLabel as keyof typeof editorCatalog, { ns: 'editor' });
    if (node.getAttribute('aria-label') !== label) node.setAttribute('aria-label', label);
    const title = node.querySelector('title');
    if (title && title.textContent !== label) title.textContent = label;
  }
  for (const node of host.querySelectorAll<HTMLElement>('[data-editor-image-alt]')) {
    const text = i18n.t('image', { ns: 'editor', alt: node.dataset.editorImageAlt || i18n.t('imageLink', { ns: 'editor' }) });
    if (node.textContent !== text) node.textContent = text;
  }
  for (const input of host.querySelectorAll<HTMLInputElement>('.milkdown-link-edit input')) {
    input.placeholder = i18n.t('linkPlaceholder', { ns: 'editor' });
  }
}
