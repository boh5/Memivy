import logo from "../../design-demo/brand/memivy-logo.svg";
import { Icon } from "../ui";
import { keyOf, type Collection, type Key, type Row, type Topic } from "./api";

export type WorkspacePage = "library" | "trash" | "topic" | "review" | "collection";
export default function WorkspaceSidebar({ page, topic, topics, configured, selected, pins, collections, collectionId,
  onLibrary, onTrash, onTopic, onDesktop, onSettings, onReview, onPin, onCollection, onNewCollection }: {
  page: WorkspacePage; topic: Topic | null; topics: Topic[]; configured: boolean;
  selected: Key | null; pins: Row[]; collections: Collection[]; collectionId: string | null;
  onLibrary: () => void; onTrash: () => void; onTopic: (topic: Topic) => void;
  onDesktop: () => void; onSettings: () => void; onReview: () => void;
  onPin: (key: Key) => void; onCollection: (id: string) => void; onNewCollection: () => void;
}) {
  return <aside className="sidebar">
    <div className="brand"><img src={logo} alt="Memivy" /></div>
    <nav aria-label="主导航">
      <button className={page === "library" ? "selected" : ""} aria-current={page === "library" ? "page" : undefined} onClick={onLibrary}><Icon name="book" />全部记忆</button>
      <button className={page === "review" ? "selected" : ""} aria-current={page === "review" ? "page" : undefined} onClick={onReview}><Icon name="history" />回顾</button>
    </nav>
    <div className="sidebar-sections">
      <details className="sidebar-group" open>
        <summary>置顶 <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{pins.map(r => <button key={keyOf(r.key)} title={r.title} onClick={() => onPin(r.key)}
          className={selected && keyOf(selected) === keyOf(r.key) && page !== "topic" ? "selected" : ""}><Icon name="pin" size={14} /><span>{r.title}</span></button>)}
          {!pins.length && <p>把常用记忆固定在手边。</p>}
        </div>
      </details>
      <section className="sidebar-collections">
        <button className="add-collection" aria-label="新建专题" title="新建专题" onClick={onNewCollection}><Icon name="plus" size={13} /></button>
        <details className="sidebar-group" open>
          <summary>专题 <Icon name="chevron" size={12} /></summary>
          <div className="topic-list">{collections.map(c => <button key={c.id} title={c.name} onClick={() => onCollection(c.id)} className={page === "collection" && collectionId === c.id ? "selected" : ""} aria-current={page === "collection" && collectionId === c.id ? "page" : undefined}><Icon name="folder" size={14} /><span>{c.name}</span><small>{c.count}</small></button>)}
            {!collections.length && <p>围绕一件事，积累想法。</p>}
          </div>
        </details>
      </section>
      <details className="sidebar-group" open>
        <summary>最近讨论 <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{topics.map(t => <button key={t.id} title={t.title} onClick={() => onTopic(t)}
          className={topic?.id === t.id && page === "topic" ? "selected" : ""} aria-current={topic?.id === t.id && page === "topic" ? "page" : undefined}>
          <Icon name="chat" size={15} /><span>{t.title}</span>{t.collection_id && <Icon name="folder" size={11} />}
        </button>)}{!topics.length && <p>聊过的思路会留在这里。</p>}</div>
      </details>
    </div>
    <div className="sidebar-bottom">
      <button className={page === "trash" ? "selected" : ""} aria-current={page === "trash" ? "page" : undefined} onClick={onTrash}><Icon name="trash" />回收站</button>
      <button onClick={onDesktop}><Icon name="leaf" />快捷入口</button>
      <button onClick={onSettings}><Icon name="settings" /><span className="settings-label">设置与数据<small>{configured ? "已配置模型" : "本地记录可用"}</small></span></button>
      <div className="local-storage-label"><i />记忆留在这台 Mac</div>
    </div>
  </aside>;
}
