import type { Parent, PhrasingContent, Root } from "mdast";
import type { Plugin } from "unified";

interface Underline extends Parent { type: "underline"; children: PhrasingContent[] }
declare module "mdast" {
  interface PhrasingContentMap { underline: Underline }
  interface RootContentMap { underline: Underline }
}

// Markdown has no underline delimiter. Recognize only paired, attribute-free
// <u> tags; arbitrary HTML remains disabled in the reader.
export const remarkUnderline: Plugin<[], Root> = function () {
  const data = this.data();
  (data.toMarkdownExtensions ||= []).push({ handlers: {
    underline(node, _parent, state, info) {
      return `<u>${state.containerPhrasing(node as Underline, { ...info, before: ">", after: "<" })}</u>`;
    },
  } });
  return tree => {
    function transform(parent: Parent) {
      const children = parent.children;
      for (let index = 0; index < children.length; index++) {
        const child = children[index];
        if ("children" in child) transform(child as Parent);
        if (child.type !== "html" || child.value !== "<u>") continue;
        let depth = 1, end = index + 1;
        for (; end < children.length; end++) {
          const next = children[end];
          if (next.type === "html" && next.value === "<u>") depth++;
          if (next.type === "html" && next.value === "</u>" && --depth === 0) break;
        }
        if (end === children.length) continue;
        const underline: Underline = { type: "underline", children: children.slice(index + 1, end) as PhrasingContent[], data: { hName: "u" } };
        transform(underline);
        children.splice(index, end - index + 1, underline);
      }
    }
    transform(tree);
  };
};
