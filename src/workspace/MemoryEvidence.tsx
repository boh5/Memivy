import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNotice } from "../i18n/react";
import { Icon } from "../ui";
import { call, date, fullDate, sourceName, errorText, type Detail, type Key, type Version } from "./api";
import { Empty, ErrorNotice, Highlight } from "./components";
import Markdown from "./Markdown";
import { MemoryChangeHistory } from "./MemoryChanges";
import OrganizationReceipt from "./OrganizationReceipt";

// Archive reads never replace the displayed head or remount its editor.
export default function MemoryEvidence({ detail, revision, query, busy, onRefresh, onOpenDiscussion, onRestoreArchive, onRestoreVersion }: {
  detail: Detail; revision: number; query: string; busy: boolean;
  onRefresh: (key?: Key) => void;
  onOpenDiscussion: (id: string) => Promise<void>;
  onRestoreArchive: (archive: { id: string; text: string }) => void;
  onRestoreVersion: (version: Version) => void;
}) {
  const { t } = useTranslation("workspace");
  const id = useId();
  const record = detail.key, trashed = detail.state === "trashed";
  const [tab, setTab] = useState<"sources" | "history" | null>(null);
  const [result, setResult] = useState<{ identity: string; detail: Detail } | null>(null);
  const [version, setVersion] = useState<Version | null>(null);
  const [loading, setLoading] = useState(false), [error, setError] = useNotice();
  const [retry, setRetry] = useState(0);
  const identity = `${record.kind}:${record.id}:${detail.current?.id || "original"}:${revision}:${retry}`;
  const archives = result?.detail.key.id === record.id && result.detail.key.kind === record.kind ? result.detail : null;
  const currentArchive = result?.identity === identity && archives?.state === detail.state &&
    archives?.current?.id === detail.current?.id;
  const expanded = tab !== null;
  useEffect(() => {
    if (!expanded || result?.identity === identity) return;
    let active = true;
    setLoading(true); setError("");
    void call<Detail>("library_detail", { key: record, archives: true })
      .then(value => { if (active) {
        setResult({ identity, detail: value });
        setVersion(selected => selected && value.history.some(version => version.id === selected.id) ? selected : null);
      } })
      .catch(error => { if (active) setError(errorText(error)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [expanded, identity]);
  return <section className="memory-evidence" aria-label={t("detail.evidenceAria")}>
    {(["sources", "history"] as const).map(section => <div className="evidence-section" key={section}>
      <button className="evidence-toggle" aria-expanded={tab === section} aria-controls={`${id}-${section}`} onClick={() => setTab(tab === section ? null : section)}>
        <Icon name={section === "sources" ? "link" : "history"} size={16} />
        <span>{t(section === "sources" ? "detail.sourcesTab" : "detail.historyTab")}</span>
        <span className="evidence-count">{section === "sources" ? detail.source_count ?? detail.sources.length : detail.history_count ?? detail.history.length}</span>
        <Icon name="chevron" size={14} />
      </button>
      <div id={`${id}-${section}`} hidden={tab !== section}>
        {tab === section && <>
          {loading && <p role="status" className="field-help">{t("detail.readingArchive")}</p>}
          <ErrorNotice text={error} />
          {error && <button className="text-button" onClick={() => setRetry(value => value + 1)}>{t("detail.retryEvidence")}</button>}
          {archives && section === "sources" && (
                <div className="source-list">
                  {!trashed && record.kind === "memory" && <MemoryChangeHistory memoryId={record.id} onOpenRecord={onRefresh} onRefresh={() => onRefresh(record)} />}
                  {archives.sources.map((s) => (
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
                          {!trashed && detail.current && <button className="outline-button" disabled={busy || !currentArchive} onClick={() => {
                            onRestoreArchive({ id: s.id, text: s.capture!.text });
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
          {archives && section === "history" && <>
            {!trashed && <OrganizationReceipt record={record} onOpen={key => onRefresh(key)} onRefresh={() => onRefresh(record)} />}
            {archives.history.length ? (
                  <div className="history-view">
                    <div className="history-list">
                      {archives.history.map((v, i) => (
                        <button
                          key={v.id}
                          className={version?.id === v.id ? "selected" : ""}
                          onClick={() => {
                            setVersion(v);
                          }}
                        >
                          <span>
                            {t("detail.historyEntry", { version: archives.history.length - i, actor: v.reason === "cleanup" ? t("detail.historyCleanup") : v.actor === "user" ? t("detail.historyMe") : t("detail.historyAi"), current: v.id === detail.current?.id ? t("detail.currentSuffix") : "" })}
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
                              disabled={busy || !currentArchive}
                              onClick={() => onRestoreVersion(version)}
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
                )}
          </>}
        </>}
      </div>
    </div>)}
  </section>;
}
