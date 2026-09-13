import { message } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import { unavailable } from "./api";
import { useResourceVersion } from "./resources";
import "./cleanup.css";
import MemoryCleanup from "./MemoryCleanup";
import IconButton from "./IconButton";
import RecordNavigation from "./RecordNavigation";
import MarkdownEditor from "./MarkdownEditor";
import Markdown from "./Markdown";
import RelatedMemories from "./RelatedMemories";
import OrganizationReceipt from "./OrganizationReceipt";
import MemoryCollections from "./MemoryCollections";
import { MemoryChangeHistory } from "./MemoryChanges";
import { notify } from "./Toast";
import { useEffect, useRef, useState } from "react";
import { Icon } from "../ui";
import {
  call,
  date,
  fullDate,
  errorText,
  keyOf,
  sourceName,
  uid,
  type Detail,
  type Source,
  type Key,
  type Receipt,
  type Version,
} from "./api";
import { Empty, ErrorNotice, Highlight, Modal, MoreMenu } from "./components";
import { useDraft } from "./useDraft";

function Editor({
  detail,
  onSaved,
  onClose,
}: {
  detail: Detail;
  onSaved: (r: Receipt) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("workspace");
  const draft = useDraft(keyOf(detail.key), {
    title: detail.title,
    body: detail.body,
    expected_version: detail.current?.id || null,
  });
  const [busy, setBusy] = useState(false),
    [error, setError] = useNotice(),
    [discard, setDiscard] = useState(false),
    [latest, setLatest] = useState(false);
  const lock = useRef(false);
  const conflict =
    draft.ready &&
    draft.value.expected_version !== (detail.current?.id || null);
  async function save() {
    if (lock.current || !draft.ready || conflict || !draft.value.body.trim() || !draft.value.title.trim()) return;
    lock.current = true;
    setBusy(true);
    setError("");
    try {
      const value = await draft.flush();
      const r = await call<Receipt>("library_edit", { draft: value });
      await draft.clear(value.request_id);
      onSaved(r);
    } catch (e) {
      setError(errorText(e));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <div className="memory-editor">
      <div className="section-heading">
        <h2>{t("detail.editCurrent")}</h2>
        <span>
          {!draft.ready
            ? t("detail.loadingDraft")
            : t("detail.draftSavedLocally")}
        </span>
      </div>
      <ErrorNotice text={draft.error || error} />
      {conflict && (
        <div className="workspace-warning">
          {t("detail.editConflict")}
          <button onClick={() => setLatest(true)}>{t("detail.viewLatest")}</button>
        </div>
      )}
      <label>
        {t("detail.titleLabel")}
        <input
          aria-label={t("detail.titleAria")}
          value={draft.value.title}
          disabled={!draft.ready || busy}
          onChange={(e) => draft.update({ title: e.target.value })}
        />
      </label>
      {draft.ready && <MarkdownEditor label={t("detail.bodyLabel")} value={draft.value.body} disabled={busy} autoFocus
        onChange={body => draft.update({ body })} onSave={() => void save()} />}
      <div className="action-row">
        <button
          className="send-button"
          title={t("detail.saveHelp")}
          aria-keyshortcuts="Meta+s"
          disabled={
            busy ||
            !draft.ready ||
            conflict ||
            !draft.value.body.trim() ||
            !draft.value.title.trim()
          }
          onClick={() => void save()}
        >
          {busy ? t("detail.saving") : detail.current ? t("detail.saveVersion") : t("detail.saveAsNew")}
        </button>
        <button className="outline-button" disabled={busy} onClick={onClose}>
          {t("detail.continueLater")}
        </button>
        <button
          className="quiet"
          disabled={busy}
          onClick={() => setDiscard(true)}
        >
          {t("detail.discard")}
        </button>
      </div>
      {discard && (
        <Modal title={t("detail.discardTitle")} onClose={() => setDiscard(false)}>
          <p>{t("detail.discardBody")}</p>
          <div className="action-row">
            <button
              className="outline-button"
              onClick={() => setDiscard(false)}
            >
              {t("detail.continueEditing")}
            </button>
            <button
              className="send-button"
              onClick={() =>
                void draft
                  .clear()
                  .then(onClose)
                  .catch((e) => setError(errorText(e)))
              }
            >
              {t("detail.discardDraft")}
            </button>
          </div>
        </Modal>
      )}
      {latest && (
        <Modal title={t("detail.latestTitle")} onClose={() => setLatest(false)}>
          <h3>{detail.title}</h3>
          <Markdown text={detail.body} />
        </Modal>
      )}
    </div>
  );
}

export default function MemoryDetail({
  record,
  revision: requestedRevision = 0,
  query,
  initialReceipt,
  onChanged,
  onDiscuss,
  onOpenDiscussion,
  onBack,
  collectionId,
}: {
  collectionId?: string;
  record: Key;
  initialReceipt: Receipt | null;
  revision?: number;
  query: string;
  onChanged: (key?: Key, receipt?: Receipt) => void;
  onBack: () => void;
  onDiscuss: (detail: Detail, related?: Source[]) => Promise<void>;
  onOpenDiscussion: (id: string) => Promise<void>;
}) {
  const { t } = useTranslation("workspace");
  const revision = useResourceVersion([{domain:"memory",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [cleaning, setCleaning] = useState(false);
  const [cleanupToolbar, setCleanupToolbar] = useState<HTMLDivElement | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null),
    [error, setError] = useNotice(),
    [loading, setLoading] = useState(true),
    [, setExportNotice, exportMessage] = useNotice();
  const [tab, setTab] = useState<"current" | "sources" | "history">("current"),
    [editing, setEditing] = useState(false),
    [version, setVersion] = useState<Version | null>(null);
  const [archive, setArchive] = useState<{ id: string; text: string } | null>(null);
  const [confirmation, setConfirmation] = useState<
      "trash" | "purge" | "restore_version" | "restore_archive" | null
    >(null),
    [busy, setBusy] = useState(false),
    [receipt, setReceipt] = useState<Receipt | null>(
      initialReceipt?.action === "undo" ? null : initialReceipt,
    ),
    [, setNotice, noticeMessage] = useNotice(
      initialReceipt?.action === "undo"
        ? message("workspace", "detail.undoNotice")
        : initialReceipt
          ? message("workspace", "detail.savedNewVersion")
          : "",
    );
  useEffect(() => { if (noticeMessage) notify(noticeMessage); }, [noticeMessage, receipt?.request_id]);
  useEffect(() => { if (exportMessage) notify(exportMessage); }, [exportMessage]);
  const loadedRecord = useRef<string | null>(null);
  const [pendingDetail, setPendingDetail] = useState<Detail | null>(null);
  const [archiveLoading, setArchiveLoading] = useState(false);
  const currentDetail = useRef(detail); currentDetail.current = detail;
  const hasUnsavedReview = useRef(false);
  hasUnsavedReview.current = cleaning || editing;
  const actionId = useRef(uid()),
    undoId = useRef(uid()),
    actionLock = useRef(false);
  useEffect(() => {
    let alive = true;
    setLoading(loadedRecord.current !== keyOf(record));
    setError(""); setArchiveLoading(tab !== "current");
    void call<Detail>("library_detail", { key: record, archives: tab !== "current" })
      .then((d) => {
        if (alive) {
          const previous = currentDetail.current;
          const backgroundHead = loadedRecord.current === keyOf(record) && previous?.state === "active" && d.state === "active" &&
            previous.current?.id !== d.current?.id && tab === "current" && !hasUnsavedReview.current && !actionLock.current;
          loadedRecord.current = keyOf(record);
          if (backgroundHead) setPendingDetail(d);
          else { setDetail(d); setPendingDetail(null); }
        }
      })
      .catch((e) => {
        if (alive) {
          // A failed background refresh must not unmount an editor or its review.
          // The core still validates the head/draft before any write.
          setDetail(current => loadedRecord.current === keyOf(record) && (!unavailable(e) || hasUnsavedReview.current) ? current : null);
          if (unavailable(e)) setPendingDetail(null);
          setError(errorText(e));
        }
      })
      .finally(() => {
        if (alive) { setLoading(false); setArchiveLoading(false); }
      });
    return () => {
      alive = false;
    };
  }, [record.id, record.kind, revision, tab]);
  async function action(
    kind: "trash" | "purge" | "restore" | "restore_version" | "restore_archive" | "undo",
  ) {
    if (!detail || actionLock.current) return;
    actionLock.current = true;
    setBusy(true);
    setError("");
    try {
      const payload =
        kind === "restore_archive" ? {
          action: "restore_archive", request_id: actionId.current, memory_id: record.id,
          expected: detail.current?.id, capture_id: archive?.id,
        } : kind === "restore_version"
          ? {
              action: "restore_version",
              request_id: actionId.current,
              memory_id: record.id,
              expected: detail.current?.id,
              version_id: version?.id,
            }
          : kind === "undo"
            ? {
                action: "undo",
                request_id: undoId.current,
                original_request: receipt?.request_id,
              }
            : {
                action: kind,
                key: record,
                expected: detail.current?.id || null,
              };
      const r = await call<Receipt | null>("library_action", {
        action: payload,
      });
      actionId.current = uid();
      undoId.current = uid();
      setConfirmation(null);
      if (kind === "trash" || kind === "purge" || kind === "restore") {
        onChanged();
        onBack();
      } else if (
        kind === "undo" &&
        !receipt?.before_version &&
        receipt?.capture_id
      ) {
        // The first manual version disappears on undo; open the retained raw input.
        onChanged({ kind: "capture", id: receipt.capture_id }, r || undefined);
      } else {
        setReceipt(kind === "undo" ? null : r);
        setNotice(
          kind === "undo" ? message("workspace", "detail.undoNotice") : message("workspace", "detail.restoredVersion"),
        );
        setVersion(null);
        setTab("current");
        onChanged(record, r || undefined);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      actionLock.current = false;
      setBusy(false);
    }
  }
  async function exportRecord() {
    if (!detail || editing || actionLock.current) return;
    actionLock.current = true;
    setBusy(true);
    setError("");
    setExportNotice("");
    try {
      const path = await call<string | null>("memory_export", {
        key: record,
        expectedVersion: detail.current?.id || null,
      });
      if (path) setExportNotice(message("workspace", "detail.exportedTo", { path }));
    } catch (e) {
      setError(errorText(e));
    } finally {
      actionLock.current = false;
      setBusy(false);
    }
  }
  if (detail?.state === "merged") return <section className="memory-detail-pane" aria-label={t("detail.mergedAria")}>
    <div className="memory-detail-toolbar"><button className="detail-back" onClick={onBack}><Icon name="chevron" size={15} />{t("detail.backToList")}</button><span>{t("detail.mergedStatus")}</span></div>
    <div className="memory-detail-scroll"><header className="memory-title-block"><h1>{detail.title}</h1><p>{t("detail.mergedText")}</p></header>
      <OrganizationReceipt record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />
      <ErrorNotice text={error} />
    </div>
  </section>;
  const trashed = detail?.state === "trashed";
  return (
    <section className="memory-detail-pane" aria-label={t("detail.memoryAria")}>
      <div className="memory-detail-toolbar">
        <button className="detail-back" onClick={onBack}>
          <Icon name="chevron" size={15} />
          {t("detail.backToList")}
        </button>
        <span>
          {trashed ? t("detail.trashStatus") : detail && !detail.current ? t("detail.originalStatus") : null}
        </span>
        {pendingDetail && <button className="quiet" onClick={() => { setDetail(pendingDetail); setPendingDetail(null); }}>{t("detail.newVersion")}</button>}
        <div className="toolbar-actions" hidden={cleaning}>
          {detail &&
            (trashed ? (
              <>
                <button disabled={busy} onClick={() => void action("restore")}>
                  {t("detail.restoreMemory")}
                </button>
                <button
                  className="danger-text"
                  disabled={busy}
                  onClick={() => setConfirmation("purge")}
                >
                  {t("detail.deleteForever")}
                </button>
              </>
            ) : !detail.current ? null : (
              <>
                <IconButton label={t("detail.discuss")} icon="chat"
                  disabled={busy || loading || editing}
                  onClick={() => {
                    setError("");
                    setBusy(true);
                    void onDiscuss(detail)
                      .catch((e) => setError(errorText(e)))
                      .finally(() => setBusy(false));
                  }}
                />
                {detail.current && <IconButton label={t("detail.cleanup")} icon="wand" disabled={busy || loading} onClick={() => { setTab("current"); setCleaning(true); }} />}
                <IconButton label={t("detail.edit")} icon="pencil"
                  disabled={busy || loading}
                  onClick={() => {
                    setTab("current");
                    setEditing(true);
                  }}
                />
                <span className="toolbar-divider" aria-hidden="true" />
                <RecordNavigation record={record} onChanged={() => onChanged()} />
                <MoreMenu>
                  <button
                    disabled={busy || loading || editing}
                    title={
                      editing
                        ? t("detail.exportWhileEditingTitle")
                        : t("detail.exportTitle")
                    }
                    onClick={() => void exportRecord()}
                  >
                    {t("detail.exportMarkdown")}
                  </button>
                  <button
                    disabled={busy}
                    onClick={() => setConfirmation("trash")}
                  >
                    {t("detail.moveToTrash")}
                  </button>
                </MoreMenu>
              </>
            ))}
        </div>
        {cleaning && <div className="cleanup-toolbar" ref={setCleanupToolbar} />}
      </div>
      <ErrorNotice text={error} />
      {!detail ? (
        <Empty
          title={loading ? t("detail.opening") : t("detail.unavailable")}
          text={loading ? t("detail.readLocal") : t("detail.unavailableText")}
        />
      ) : (
        <>
          <div className="memory-detail-scroll">
            <header className="memory-title-block">
              <span className="eyebrow">
                <Icon name="leaf" size={14} />
                {detail.current
                  ? t("detail.versionMeta", { actor: detail.current.reason === "cleanup" ? t("detail.cleanupConfirmed") : detail.current.actor === "user" ? (detail.current.reason === "create" ? t("detail.recordedByMe") : t("detail.editedByMe")) : t("detail.aiOrganized"), date: date(detail.current.created_at) })
                  : t("detail.originalSaved")}
              </span>
              <h1>
                <Highlight text={detail.title} query={query} />
              </h1>
              {!detail.current && <p>{t("detail.archiveOnly")}</p>}
              {!trashed && <OrganizationReceipt presentation="status" record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />}
            </header>
            {!trashed && detail.current && <MemoryCollections record={record} currentVersion={detail.current?.id} onRefresh={() => onChanged()} />}
            <div className="detail-tabs" hidden={cleaning} role="tablist" aria-label={t("detail.tabsAria")}>
              <button
                role="tab"
                aria-selected={tab === "current"}
                onClick={() => setTab("current")}
              >
                {t("detail.currentTab")}
              </button>
              <button
                role="tab"
                aria-selected={tab === "sources"}
                onClick={() => setTab("sources")}
              >
                {t("detail.sourcesTab")} <span>{detail.source_count ?? detail.sources.length}</span>
              </button>
              <button
                role="tab"
                aria-selected={tab === "history"}
                onClick={() => setTab("history")}
              >
                {t("detail.historyTab")} <span>{detail.history_count ?? detail.history.length}</span>
              </button>
            </div>
            <div role="tabpanel">
              {archiveLoading && <p role="status" className="field-help">{t("detail.readingArchive")}</p>}
              {!cleaning && !editing && !trashed && receipt && detail.current?.reason === "cleanup" && receipt.after_version === detail.current.id && <div className="cleanup-receipt" role="status">{t("detail.cleanupReceipt")}<button className="quiet" disabled={busy} onClick={() => void action("undo")}>{t("detail.undoCleanup")}</button></div>}
              {tab === "history" && !trashed && <>
                {receipt && <button className="toolbar-button" disabled={busy} onClick={() => void action("undo")}>{t("detail.undoChange")}</button>}
                <OrganizationReceipt record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />
              </>}
              {cleaning && cleanupToolbar && <MemoryCleanup key={detail.key.id} detail={detail} toolbar={cleanupToolbar}
                onClose={() => setCleaning(false)}
                onSaved={r => { setCleaning(false); setEditing(false); setReceipt(r); undoId.current = uid(); setNotice(message("workspace", "detail.cleanupSavedNotice")); onChanged(record, r); }} />}
              {!cleaning && tab === "current" &&
                (editing && !trashed ? (
                  <Editor
                    detail={detail}
                    onClose={() => setEditing(false)}
                    onSaved={(r) => {
                      setEditing(false);
                      setReceipt(r);
                      undoId.current = uid();
                      setNotice(message("workspace", "detail.savedNewVersion"));
                      onChanged(
                        r.memory_id
                          ? { kind: "memory", id: r.memory_id }
                          : record,
                        r,
                      );
                    }}
                  />
                ) : (
                  <>
                    <div>{detail.current ? <Markdown text={detail.body} query={query} /> : <div className="readable-text"><Highlight text={detail.body} query={query} /></div>}</div>
                    {!trashed && detail.current && <RelatedMemories
                      collectionId={collectionId} paused={!!pendingDetail}
                      key={detail.current.id} memoryId={detail.key.id} versionId={detail.current.id}
                      onOpen={onChanged} onDiscuss={sources => onDiscuss(detail, sources)} />}
                  </>
                ))}
              {!cleaning && tab === "sources" && (
                <div className="source-list">
                  {!trashed && record.kind === "memory" && <MemoryChangeHistory memoryId={record.id} onOpenRecord={onChanged} onRefresh={() => onChanged(record)} />}
                  {detail.sources.map((s) => (
                    <article key={s.id} className="source-block">
                      <div className="section-heading">
                        <strong>
                          {s.capture
                            ? sourceName(s.capture.origin)
                            : t("detail.sourceUnavailable")}
                        </strong>
                        <span>{s.capture && date(s.capture.created_at)}</span>
                      </div>
                      {s.capture ? (
                        <>
                          <div className="readable-text">
                            <Highlight text={s.capture.text} query={query} />
                          </div>
                          {s.capture.origin.conversation_id && <p className="source-metadata">
                            {s.conversation_available ? <button className="quiet" onClick={() => void onOpenDiscussion(s.capture!.origin.conversation_id!)}>{t("input.openOriginalDiscussion")}</button> : t("input.originalDiscussionUnavailable")}
                          </p>}
                          {s.capture.origin.project && (
                            <p className="source-metadata">
                              {t("detail.project", { project: s.capture.origin.project })}
                            </p>
                          )}
                          {s.capture.origin.uri && (
                            <p className="source-metadata">
                              {t("detail.attachedSource", { uri: s.capture.origin.uri })}
                            </p>
                          )}
                          {!trashed && detail.current && <button className="outline-button" disabled={busy || editing} onClick={() => {
                            setArchive({ id: s.id, text: s.capture!.text }); actionId.current = uid(); setConfirmation("restore_archive");
                          }}>{t("detail.restoreArchive")}</button>}
                          <small>
                            {t("detail.originalInput", { date: fullDate(s.capture.created_at) })}
                          </small>
                        </>
                      ) : (
                        <p className="field-help">
                          {t("detail.deletedSource")}
                        </p>
                      )}
                    </article>
                  ))}
                </div>
              )}
              {tab === "history" &&
                (detail.history.length ? (
                  <div className="history-view">
                    <div className="history-list">
                      {detail.history.map((v, i) => (
                        <button
                          key={v.id}
                          className={version?.id === v.id ? "selected" : ""}
                          onClick={() => {
                            setVersion(v);
                            actionId.current = uid();
                          }}
                        >
                          <span>
                            {t("detail.historyEntry", { version: detail.history.length - i, actor: v.reason === "cleanup" ? t("detail.historyCleanup") : v.actor === "user" ? t("detail.historyMe") : t("detail.historyAi"), current: v.id === detail.current?.id ? t("detail.currentSuffix") : "" })}
                          </span>
                          <small>{fullDate(v.created_at)}</small>
                        </button>
                      ))}
                    </div>
                    {version ? (
                      <article className="history-preview">
                        <div className="section-heading">
                          <h3>{version.title}</h3>
                          {version.id !== detail.current?.id && !trashed && (
                            <button
                              className="outline-button"
                              disabled={busy}
                              onClick={() => setConfirmation("restore_version")}
                            >
                              {t("detail.restoreVersion")}
                            </button>
                          )}
                        </div>
                        <Markdown text={version.body} />
                        <p className="field-help">
                          {t("detail.historyBasis", { count: version.capture_ids.length })}
                        </p>
                      </article>
                    ) : (
                      <p className="field-help">
                        {t("detail.selectVersion")}
                      </p>
                    )}
                  </div>
                ) : (
                  <Empty
                    title={t("detail.originalAlways")}
                    text={t("detail.historyEmpty")}
                  />
                ))}
            </div>
          </div>
        </>
      )}
      {confirmation && (
        <Modal
          title={
            confirmation === "trash"
              ? t("detail.trashTitle")
              : confirmation === "purge"
                ? t("detail.purgeTitle")
                : confirmation === "restore_archive" ? t("detail.restoreArchiveTitle") : t("detail.restoreVersionTitle")
          }
          onClose={() => {
            if (!busy) setConfirmation(null);
          }}
        >
          <p>
            {confirmation === "trash"
              ? t("detail.trashBody")
              : confirmation === "purge"
                ? t("detail.purgeBody", { product: "Memivy" })
                : t("detail.restoreBody")}
          </p>
          {confirmation === "restore_archive" && archive && <div className="readable-text">{archive.text}</div>}
          <ErrorNotice text={error} />
          <div className="action-row">
            <button
              className="outline-button"
              disabled={busy}
              data-modal-autofocus
              onClick={() => setConfirmation(null)}
            >
              {t("detail.cancel")}
            </button>
            <button
              className={
                confirmation === "purge"
                  ? "send-button destructive"
                  : "send-button"
              }
              disabled={busy}
              onClick={() => void action(confirmation)}
            >
              {busy
                ? t("detail.processing")
                : confirmation === "trash"
                  ? t("detail.moveToTrash")
                  : confirmation === "purge"
                    ? t("detail.deleteForever")
                    : t("detail.confirmRestore")}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
