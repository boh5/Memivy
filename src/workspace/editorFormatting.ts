import type { Command, Transaction } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import { lift, setBlockType, toggleMark, wrapIn } from "@milkdown/kit/prose/commands";
import { liftListItem, wrapInList } from "@milkdown/kit/prose/schema-list";
import { AllSelection, EditorState, TextSelection } from "@milkdown/kit/prose/state";

export type Format = "paragraph" | "h1" | "h2" | "h3" | "blockquote" | "bullet_list" | "ordered_list" | "task_list" | "code_block" | "strong" | "emphasis" | "underline" | "strike_through" | "inlineCode" | "link" | "table" | "clear";
export type FormatState = { block: string; strong: boolean; emphasis: boolean; underline: boolean; strike_through: boolean; inlineCode: boolean };
export const emptyFormat: FormatState = { block: "paragraph", strong: false, emphasis: false, underline: false, strike_through: false, inlineCode: false };
export function readFormat(state: EditorState): FormatState {
  const { selection, storedMarks } = state;
  const $from = selection.$from.depth ? selection.$from : TextSelection.near(selection.$from).$from;
  const parent = $from.parent;
  let block = parent.type.name === "heading" ? `h${parent.attrs.level}` : parent.type.name;
  for (let depth = $from.depth; depth > 0; depth--) {
    const name = $from.node(depth).type.name;
    if (name === "list_item" && $from.node(depth).attrs.checked != null) { block = "task_list"; break; }
    if (["bullet_list", "ordered_list", "blockquote"].includes(name)) { block = name; break; }
  }
  const marked = (name: string) => selection.empty ? !!(storedMarks || selection.$from.marks()).find(mark => mark.type.name === name) : !!state.schema.marks[name] && state.doc.rangeHasMark(selection.from, selection.to, state.schema.marks[name]);
  return { block, strong: marked("strong"), emphasis: marked("emphasis"), underline: marked("underline"), strike_through: marked("strike_through"), inlineCode: marked("inlineCode") };
}

// Insert ahead of the selected block, never replace the user's selected words.
export const insertEditorTable: Command = (state, dispatch) => {
  const { schema, selection } = state;
  const { table, table_header_row, table_row, table_header, table_cell } = schema.nodes;
  if (!table || !table_header_row || !table_row || !table_header || !table_cell) return false;
  const node = table.create(null, [
    table_header_row.create(null, [table_header.createAndFill()!, table_header.createAndFill()!]),
    table_row.create(null, [table_cell.createAndFill()!, table_cell.createAndFill()!]),
  ]);
  const { $from } = selection;
  let depth = $from.depth;
  while (depth > 0 && !$from.node(depth - 1).canReplaceWith($from.index(depth - 1), $from.index(depth - 1), table)) depth--;
  if (depth === 0 && !state.doc.canReplaceWith(0, 0, table)) return false;
  const pos = depth === 0 ? 0 : $from.before(depth);
  const tr = state.tr.insert(pos, node);
  dispatch?.(tr.setSelection(TextSelection.near(tr.doc.resolve(pos + 1))).scrollIntoView());
  return true;
};

type FormatTarget = Pick<EditorView, "state" | "dispatch">;

function unwrapSelectedBlocks(view: FormatTarget) {
  // Work on each selected text block, including wrappers after an ordinary
  // paragraph. Map the original selection through each lift rather than moving
  // the user's selection to the block being processed.
  const tr = view.state.tr;
  const { schema } = view.state;
  for (;;) {
    const { doc, selection } = tr;
    let lifted = false;
    doc.nodesBetween(selection.from, selection.to, (node, pos) => {
      if (lifted || !node.isTextblock) return !lifted;
      const $pos = doc.resolve(pos + 1);
      for (let depth = $pos.depth - 1; depth > 0; depth--) {
        const name = $pos.node(depth).type.name;
        if (name !== "list_item" && name !== "blockquote") continue;
        const local = EditorState.create({ schema, doc, selection: TextSelection.create(doc, pos + 1) });
        const command = name === "list_item" ? liftListItem(schema.nodes.list_item) : lift;
        command(local, next => {
          if (next.doc.eq(doc)) return;
          next.steps.forEach(step => tr.step(step));
          tr.setSelection(selection.getBookmark().map(next.mapping).resolve(tr.doc));
          lifted = true;
        });
        break;
      }
      return false;
    });
    if (!lifted) break;
  }
  if (tr.docChanged) view.dispatch(tr);
}

export function applyFormat(view: EditorView, format: Format) {
  if (!view.editable || view.composing) return;
  // One button press is one document transaction: no intermediate drafts or
  // separate undo steps while unwrapping blocks and removing text marks.
  const tr = view.state.tr;
  const target: FormatTarget = {
    state: view.state,
    dispatch(next: Transaction) {
      next.steps.forEach(step => tr.step(step));
      tr.setSelection(next.selection.getBookmark().resolve(tr.doc));
      if (next.storedMarksSet) tr.setStoredMarks(next.storedMarks);
      target.state = EditorState.create({ schema: view.state.schema, doc: tr.doc, selection: tr.selection, storedMarks: tr.storedMarks });
    },
  };
  applyToSelection(target, format);
  if (tr.docChanged || tr.selectionSet || tr.storedMarksSet) view.dispatch(tr.scrollIntoView());
  view.focus();
}

function applyToSelection(view: FormatTarget, format: Format) {
  if (view.state.selection instanceof AllSelection) {
    const { doc, tr } = view.state;
    const start = TextSelection.near(doc.resolve(0), 1).from;
    const end = TextSelection.near(doc.resolve(doc.content.size), -1).to;
    view.dispatch(tr.setSelection(TextSelection.between(doc.resolve(start), doc.resolve(end))));
  }
  const run = (command: Command) => command(view.state, view.dispatch);
  const { schema } = view.state;
  if (["strong", "emphasis", "underline", "strike_through", "inlineCode"].includes(format)) {
    if (schema.marks[format]) run(toggleMark(schema.marks[format]));
  } else if (format === "clear") {
    applyToSelection(view, "paragraph");
    const { from, to, empty } = view.state.selection;
    const tr = view.state.tr;
    if (empty) tr.setStoredMarks([]); else tr.removeMark(from, to);
    view.dispatch(tr);
  } else if (format === "table") {
    run(insertEditorTable);
  } else if (format !== "link") {
    const active = readFormat(view.state).block;
    const target = format === active && format !== "paragraph" ? "paragraph" : format;
    unwrapSelectedBlocks(view);
    if (/^h[1-6]$/.test(target)) run(setBlockType(schema.nodes.heading, { level: Number(target.slice(1)) }));
    else {
      run(setBlockType(schema.nodes[target === "code_block" ? "code_block" : "paragraph"]));
      if (target === "blockquote") run(wrapIn(schema.nodes.blockquote));
      if (target === "bullet_list" || target === "ordered_list") run(wrapInList(schema.nodes[target]));
      if (target === "task_list") {
        run(wrapInList(schema.nodes.bullet_list));
        const { from, to } = view.state.selection;
        const tr = view.state.tr;
        view.state.doc.nodesBetween(from, to, (node, pos) => {
          if (node.type.name === "list_item") tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: false });
        });
        view.dispatch(tr);
      }
    }
  }
}
