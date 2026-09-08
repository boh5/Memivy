import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { remarkUnderline } from "./remarkUnderline";
import { Highlight } from "./components";
import { Children, cloneElement, isValidElement, type ReactNode } from "react";
import "./markdown.css";

export default function Markdown({ text, query = "" }: { text: string; query?: string }) {
  function highlight(children: ReactNode): ReactNode {
    return Children.map(children, child => typeof child === "string" ? <Highlight text={child} query={query} /> :
      isValidElement<{ children?: ReactNode }>(child) ? cloneElement(child, {}, highlight(child.props.children)) : child);
  }
  return <div className="markdown-prose"><ReactMarkdown remarkPlugins={[remarkGfm, remarkUnderline]} skipHtml components={{
    p: ({ children }) => <p>{highlight(children)}</p>,
    li: ({ children, className }) => <li className={className}>{highlight(children)}</li>,
    h1: ({ children }) => <h1>{highlight(children)}</h1>,
    h2: ({ children }) => <h2>{highlight(children)}</h2>,
    h3: ({ children }) => <h3>{highlight(children)}</h3>,
    a: ({ children, href }) => href && /^https?:\/\//i.test(href) ? <a href={href} target="_blank" rel="noreferrer noopener">{children}</a> : <span>{children}</span>,
    img: ({ alt }) => <span className="markdown-image">[图片：{alt || "图片链接"}]</span>,
    table: ({ children }) => <div className="markdown-table-scroll"><table>{children}</table></div>,
  }}>{text}</ReactMarkdown></div>;
}
