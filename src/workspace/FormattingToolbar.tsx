import { Fragment } from "react";
import type { Format, FormatState } from "./editorFormatting";

const groups: [Format, string, string][][] = [
  [["h1", "一级标题", "H1"], ["h2", "二级标题", "H2"], ["h3", "三级标题", "H3"],
    ["strong", "加粗", "B"], ["emphasis", "斜体", "I"], ["underline", "下划线", "U"], ["strike_through", "删除线", "S"], ["clear", "清除格式", "clear"], ["inlineCode", "行内代码", "inline"]],
  [["ordered_list", "编号列表", "ordered"], ["bullet_list", "项目列表", "bullet"], ["task_list", "待办列表", "task"]],
  [["blockquote", "引用", "quote"], ["code_block", "代码块", "code"], ["link", "链接", "link"], ["table", "表格", "table"]],
];
const paths: Record<string, string> = {
  link: "M10 13a5 5 0 0 0 7 .1l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7-.1l-3 3a5 5 0 0 0 7 7l2-2",
  clear: "M5 5h14M12 5l-3 14M6 19h6M3 3l18 18",
  bullet: "M9 6h12M9 12h12M9 18h12M3 6h1M3 12h1M3 18h1",
  ordered: "M10 6h11M10 12h11M10 18h11M3 4h1v5M2 9h4M2 14c0-3 4-3 4 0 0 1-4 4-4 5h4",
  task: "M10 6h11M10 12h11M10 18h11M2 5l2 2 3-4M2 11l2 2 3-4M2 17l2 2 3-4",
  quote: "M9 5C5 5 4 8 4 12v5h5v-6H4M20 5c-4 0-5 3-5 7v5h5v-6h-5",
  inline: "M7 7l-5 5 5 5M17 7l5 5-5 5M14 4l-4 16",
  code: "M3 6l2-2M8 4l2 2M14 4h5a2 2 0 0 1 2 2v13H3V10",
  table: "M3 4h18v16H3V4ZM3 9h18M3 14h18M10 4v16",
};

export default function FormattingToolbar({ state, onFormat, disabled = false }: { state: FormatState; onFormat: (format: Format) => void; disabled?: boolean }) {
  return <div className="formatting-toolbar" role="toolbar" aria-label="选区格式">
    {groups.map((actions, index) => <Fragment key={index}>
      {index > 0 && <span className="format-divider" />}
      {actions.map(([key, label, icon]) => {
        const active = key in state ? state[key as keyof FormatState] === true : state.block === key;
        return <button key={key} type="button" title={active && ["h1", "h2", "h3", "ordered_list", "bullet_list", "task_list", "blockquote", "code_block"].includes(key) ? `${label} · 再次点击恢复普通文本` : label}
          aria-label={label} disabled={disabled} aria-pressed={["link", "clear", "table"].includes(key) ? undefined : active}
          onMouseDown={e => e.preventDefault()} onClick={() => onFormat(key)} className={`format-${key}`}>
          {paths[icon] ? <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d={paths[icon]} /></svg> : icon}
        </button>;
      })}
    </Fragment>)}
  </div>;
}
