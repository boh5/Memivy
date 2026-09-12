import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { remarkUnderline } from "./remarkUnderline";
import { Highlight } from "./components";
import { Children, cloneElement, createContext, isValidElement, memo, useContext, type ReactNode } from "react";
import "./markdown.css";
import { useTranslation } from 'react-i18next';

// ReactMarkdown uses these functions as component types. Their identities must
// stay stable across parent renders, including changes to the highlight query.
const QueryContext = createContext("");
function ImagePlaceholder({ alt }: { alt?: string }) {
  const { t } = useTranslation('editor');
  return <span className="markdown-image">{t('image', { alt: alt || t('imageLink') })}</span>;
}
function Highlighted({ children }: { children: ReactNode }) {
  const query = useContext(QueryContext);
  function highlight(nodes: ReactNode): ReactNode {
    return Children.map(nodes, child => typeof child === "string" ? <Highlight text={child} query={query} /> :
      isValidElement<{ children?: ReactNode }>(child) ? cloneElement(child, {}, highlight(child.props.children)) : child);
  }
  return <>{highlight(children)}</>;
}
const components: Components = {
  p: ({ children }) => <p><Highlighted>{children}</Highlighted></p>,
  li: ({ children, className }) => <li className={className}><Highlighted>{children}</Highlighted></li>,
  h1: ({ children }) => <h1><Highlighted>{children}</Highlighted></h1>,
  h2: ({ children }) => <h2><Highlighted>{children}</Highlighted></h2>,
  h3: ({ children }) => <h3><Highlighted>{children}</Highlighted></h3>,
  a: ({ children, href }) => href && /^https?:\/\//i.test(href) ? <a href={href} target="_blank" rel="noreferrer noopener">{children}</a> : <span>{children}</span>,
  img: ImagePlaceholder,
  table: ({ children }) => <div className="markdown-table-scroll"><table>{children}</table></div>,
};
const plugins = [remarkGfm, remarkUnderline];
export default memo(function Markdown({ text, query = "" }: { text: string; query?: string }) {
  return <QueryContext value={query}><div className="markdown-prose"><ReactMarkdown remarkPlugins={plugins} skipHtml components={components}>{text}</ReactMarkdown></div></QueryContext>;
});
