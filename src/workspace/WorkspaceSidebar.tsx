import logo from "../../design-demo/brand/memivy-logo.svg";
import { Icon } from "../ui";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation("workspace");
  return <aside className="sidebar">
    <div className="brand"><img src={logo} alt="Memivy" /></div>
    <nav aria-label={t("nav.main")}>
      <button className={page === "library" ? "selected" : ""} aria-current={page === "library" ? "page" : undefined} onClick={onLibrary}><Icon name="book" />{t("nav.library")}</button>
      <button className={page === "review" ? "selected" : ""} aria-current={page === "review" ? "page" : undefined} onClick={onReview}><Icon name="history" />{t("nav.review")}</button>
    </nav>
    <div className="sidebar-sections">
      <details className="sidebar-group" open>
        <summary>{t("nav.pinned")} <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{pins.map(r => <button key={keyOf(r.key)} title={r.title} onClick={() => onPin(r.key)}
          className={selected && keyOf(selected) === keyOf(r.key) && page !== "topic" ? "selected" : ""}><Icon name="pin" size={14} /><span>{r.title}</span></button>)}
          {!pins.length && <p>{t("nav.pinnedEmpty")}</p>}
        </div>
      </details>
      <section className="sidebar-collections">
        <button className="add-collection" aria-label={t("nav.newCollection")} title={t("nav.newCollection")} onClick={onNewCollection}><Icon name="plus" size={13} /></button>
        <details className="sidebar-group" open>
          <summary>{t("nav.collections")} <Icon name="chevron" size={12} /></summary>
          <div className="topic-list">{collections.map(c => <button key={c.id} title={c.name} onClick={() => onCollection(c.id)} className={page === "collection" && collectionId === c.id ? "selected" : ""} aria-current={page === "collection" && collectionId === c.id ? "page" : undefined}><Icon name="folder" size={14} /><span>{c.name}</span><small>{c.count}</small></button>)}
            {!collections.length && <p>{t("nav.collectionsEmpty")}</p>}
          </div>
        </details>
      </section>
      <details className="sidebar-group" open>
        <summary>{t("nav.recentDiscussions")} <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{topics.map(t => <button key={t.id} title={t.title} onClick={() => onTopic(t)}
          className={topic?.id === t.id && page === "topic" ? "selected" : ""} aria-current={topic?.id === t.id && page === "topic" ? "page" : undefined}>
          <Icon name="chat" size={15} /><span>{t.title}</span>{t.collection_id && <Icon name="folder" size={11} />}
        </button>)}{!topics.length && <p>{t("nav.recentDiscussionsEmpty")}</p>}</div>
      </details>
    </div>
    <div className="sidebar-bottom">
      <button className={page === "trash" ? "selected" : ""} aria-current={page === "trash" ? "page" : undefined} onClick={onTrash}><Icon name="trash" />{t("nav.trash")}</button>
      <button onClick={onDesktop}><Icon name="leaf" />{t("nav.quickEntry")}</button>
      <button onClick={onSettings}><Icon name="settings" /><span className="settings-label">{t("nav.settings")}<small>{configured ? t("nav.modelConfigured") : t("nav.localAvailable")}</small></span></button>
      <div className="local-storage-label"><i />{t("nav.localStorage")}</div>
    </div>
  </aside>;
}
