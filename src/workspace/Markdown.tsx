import ReactMarkdown, { defaultUrlTransform, type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { remarkUnderline } from "./remarkUnderline";
import { Highlight } from "./components";
import { Children, cloneElement, createContext, isValidElement, memo, useContext, type ReactNode } from "react";
import "./markdown.css";
import type { Source } from "./api";
import { useTranslation } from 'react-i18next';

// ReactMarkdown uses these functions as component types. Their identities must
// stay stable across parent renders, including changes to the highlight query.
const QueryContext = createContext("");
const CitationContext = createContext<{ sources: { source: Source; available: boolean }[]; onSource?: (source: Source) => void }>({ sources: [] });
function SourceLink({ children, href }: { children?: ReactNode; href?: string }) {
  const citations = useContext(CitationContext);
  const match = href?.match(/^memivy:\/\/source\/(capture|version)\/([0-9a-f-]{36})$/i);
  if (match) {
    const source: Source = { kind: match[1] as Source["kind"], id: match[2] };
    const available = citations.sources.some(c => c.available && c.source.kind === source.kind && c.source.id === source.id);
    return <button className="markdown-citation" disabled={!available || !citations.onSource} onClick={() => citations.onSource?.(source)}>{children}</button>;
  }
  return href && /^https?:\/\//i.test(href) ? <a href={href} target="_blank" rel="noreferrer noopener">{children}</a> : <span>{children}</span>;
}
function urlTransform(url: string) { return /^memivy:\/\/source\/(capture|version)\/[0-9a-f-]{36}$/i.test(url) ? url : defaultUrlTransform(url); }
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
  a: SourceLink,
  img: ImagePlaceholder,
  table: ({ children }) => <div className="markdown-table-scroll"><table>{children}</table></div>,
};
const plugins = [remarkGfm, remarkUnderline];
export default memo(function Markdown({ text, query = "", sources = [], onSource }: { text: string; query?: string; sources?: { source: Source; available: boolean }[]; onSource?: (source: Source) => void }) {
  return <QueryContext value={query}><CitationContext value={{ sources, onSource }}><div className="markdown-prose"><ReactMarkdown remarkPlugins={plugins} skipHtml components={components} urlTransform={urlTransform}>{text}</ReactMarkdown></div></CitationContext></QueryContext>;
});
