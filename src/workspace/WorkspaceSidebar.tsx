import logo from "../../design-demo/brand/memivy-logo.svg";
import { Icon } from "../ui";
import { useTranslation } from "react-i18next";
import { keyOf, type Collection, type Key, type Row, type Topic } from "./api";

export type WorkspacePage = "library" | "trash" | "topic" | "activity" | "collection";
export default function WorkspaceSidebar({ page, topic, topics, selected, pins, collections, collectionId,
  onLibrary, onTrash, onTopic, onDesktop, onSettings, onActivity, onPin, onCollection, onNewCollection }: {
  page: WorkspacePage; topic: Topic | null; topics: Topic[];
  selected: Key | null; pins: Row[]; collections: Collection[]; collectionId: string | null;
  onLibrary: () => void; onTrash: () => void; onTopic: (topic: Topic) => void;
  onDesktop: () => void; onSettings: () => void; onActivity: () => void;
  onPin: (key: Key) => void; onCollection: (id: string) => void; onNewCollection: () => void;
}) {
  const { t } = useTranslation("workspace");
  return <aside className="sidebar" id="workspace-sidebar">
    <div className="brand"><img src={logo} alt="Memivy" /></div>
    <nav aria-label={t("nav.main")}>
      <button className={page === "library" ? "selected" : ""} aria-current={page === "library" ? "page" : undefined} onClick={onLibrary}><Icon name="book" />{t("nav.library")}</button>
      <button className={page === "activity" ? "selected" : ""} aria-current={page === "activity" ? "page" : undefined} onClick={onActivity}><Icon name="activity" />{t("nav.activity")}</button>
    </nav>
    <div className="sidebar-sections">
      {!!pins.length && <details className="sidebar-group" open>
        <summary>{t("nav.pinned")} <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{pins.map(r => <button key={keyOf(r.key)} title={r.title} onClick={() => onPin(r.key)}
          className={selected && keyOf(selected) === keyOf(r.key) && page !== "topic" ? "selected" : ""}><Icon name="pin" size={14} /><span>{r.title}</span></button>)}
        </div>
      </details>}
      <section className="sidebar-collections">
        <button className="add-collection" aria-label={t("nav.newCollection")} title={t("nav.newCollection")} onClick={onNewCollection}><Icon name="plus" size={13} /></button>
        <details className="sidebar-group" open>
          <summary>{t("nav.collections")} <Icon name="chevron" size={12} /></summary>
          <div className="topic-list">{collections.map(c => <button key={c.id} title={c.name} onClick={() => onCollection(c.id)} className={page === "collection" && collectionId === c.id ? "selected" : ""} aria-current={page === "collection" && collectionId === c.id ? "page" : undefined}><Icon name="folder" size={14} /><span>{c.name}</span><small>{c.count}</small></button>)}
          </div>
        </details>
      </section>
      {!!topics.length && <details className="sidebar-group" open>
        <summary>{t("nav.recentDiscussions")} <Icon name="chevron" size={12} /></summary>
        <div className="topic-list">{topics.map(t => <button key={t.id} title={t.title} onClick={() => onTopic(t)}
          className={topic?.id === t.id && page === "topic" ? "selected" : ""} aria-current={topic?.id === t.id && page === "topic" ? "page" : undefined}>
          <Icon name="chat" size={15} /><span>{t.title}</span>{t.collection_id && <Icon name="folder" size={11} />}
        </button>)}</div>
      </details>}
    </div>
    <div className="sidebar-bottom">
      <button className={page === "trash" ? "selected" : ""} aria-current={page === "trash" ? "page" : undefined} onClick={onTrash}><Icon name="trash" />{t("nav.trash")}</button>
      <button onClick={onDesktop}><Icon name="leaf" />{t("nav.quickEntry")}</button>
      <button onClick={onSettings}><Icon name="settings" />{t("nav.settings")}</button>
    </div>
  </aside>;
}
