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
  type Draft,
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
  const draft = useDraft(keyOf(detail.key), {
    title: detail.reviewed_conclusion?.title || detail.title,
    body: detail.reviewed_conclusion?.body || detail.body,
    expected_version: detail.current?.id || null,
  });
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
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
        <h2>编辑当前内容</h2>
        <span>
          {!draft.ready
            ? "载入草稿…"
            : "草稿自动保存在本机"}
        </span>
      </div>
      <ErrorNotice text={draft.error || error} />
      {conflict && (
        <div className="workspace-warning">
          这条记忆已有新版本。你的草稿仍然保留，请先核对最新内容。
          <button onClick={() => setLatest(true)}>查看最新内容</button>
        </div>
      )}
      <label>
        标题
        <input
          aria-label="编辑记忆标题"
          value={draft.value.title}
          disabled={!draft.ready || busy}
          onChange={(e) => draft.update({ title: e.target.value })}
        />
      </label>
      {draft.ready && <MarkdownEditor label="编辑记忆内容" value={draft.value.body} disabled={busy} autoFocus
        onChange={body => draft.update({ body })} onSave={() => void save()} />}
      <p className="field-help">保存会建立新版本，原话保持原样。⌘ S 保存。</p>
      <div className="action-row">
        <button
          className="send-button"
          disabled={
            busy ||
            !draft.ready ||
            conflict ||
            !draft.value.body.trim() ||
            !draft.value.title.trim()
          }
          onClick={() => void save()}
        >
          {busy ? "保存中…" : detail.current ? "保存版本" : "确认另存为新记忆"}
        </button>
        <button className="outline-button" disabled={busy} onClick={onClose}>
          稍后继续
        </button>
        <button
          className="quiet"
          disabled={busy}
          onClick={() => setDiscard(true)}
        >
          放弃修改
        </button>
      </div>
      {discard && (
        <Modal title="放弃这份编辑草稿？" onClose={() => setDiscard(false)}>
          <p>当前已保存的记忆和原话会保留。</p>
          <div className="action-row">
            <button
              className="outline-button"
              onClick={() => setDiscard(false)}
            >
              继续编辑
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
              放弃草稿
            </button>
          </div>
        </Modal>
      )}
      {latest && (
        <Modal title="最新保存的内容" onClose={() => setLatest(false)}>
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
}) {
  const revision = useResourceVersion([{domain:"memory",entity:`${record.kind}:${record.id}`}]) + requestedRevision;
  const [cleaning, setCleaning] = useState(false);
  const [cleanupToolbar, setCleanupToolbar] = useState<HTMLDivElement | null>(null);
  const [detail, setDetail] = useState<Detail | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(true),
    [exportNotice, setExportNotice] = useState("");
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
    [notice, setNotice] = useState(
      initialReceipt?.action === "undo"
        ? "已撤销，原话仍保留。"
        : initialReceipt
          ? "当前内容已保存为新版本，原话保持原样。"
          : "",
    );
  useEffect(() => { if (notice) notify(notice); }, [notice, receipt?.request_id]);
  useEffect(() => { if (exportNotice) notify(exportNotice); }, [exportNotice]);
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
          kind === "undo" ? "已撤销，原话仍保留。" : "已恢复为新版本。",
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
      if (path) setExportNotice(`这一篇已导出到 ${path}`);
    } catch (e) {
      setError(errorText(e));
    } finally {
      actionLock.current = false;
      setBusy(false);
    }
  }
  if (detail?.state === "merged") return <section className="memory-detail-pane" aria-label="归并回执">
    <div className="memory-detail-toolbar"><button className="detail-back" onClick={onBack}><Icon name="chevron" size={15} />返回列表</button><span>已归并</span></div>
    <div className="memory-detail-scroll"><header className="memory-title-block"><h1>{detail.title}</h1><p>这条内容已归入另一篇记忆。可以查看目标，或通过回执撤销这次整理。</p></header>
      <OrganizationReceipt record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />
      <ErrorNotice text={error} />
    </div>
  </section>;
  const trashed = detail?.state === "trashed";
  return (
    <section className="memory-detail-pane" aria-label="记忆正文">
      <div className="memory-detail-toolbar">
        <button className="detail-back" onClick={onBack}>
          <Icon name="chevron" size={15} />
          返回列表
        </button>
        <span>
          {trashed ? "回收站" : detail?.current ? "当前记忆" : "原始记录"}
        </span>
        {pendingDetail && <button className="quiet" onClick={() => { setDetail(pendingDetail); setPendingDetail(null); }}>有新版本，查看</button>}
        <div className="toolbar-actions" hidden={cleaning}>
          {detail &&
            (trashed ? (
              <>
                <button disabled={busy} onClick={() => void action("restore")}>
                  恢复记忆
                </button>
                <button
                  className="danger-text"
                  disabled={busy}
                  onClick={() => setConfirmation("purge")}
                >
                  永久删除
                </button>
              </>
            ) : !detail.current ? null : (
              <>
                <IconButton label="接着想" icon="chat"
                  disabled={busy || loading || editing}
                  onClick={() => {
                    setError("");
                    setBusy(true);
                    void onDiscuss(detail)
                      .catch((e) => setError(errorText(e)))
                      .finally(() => setBusy(false));
                  }}
                />
                {detail.current && <IconButton label="整理正文" icon="wand" disabled={busy || loading} onClick={() => { setTab("current"); setCleaning(true); }} />}
                <IconButton label="编辑正文" icon="pencil"
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
                        ? "保存当前编辑后再导出"
                        : "导出这一篇的标题与当前正文为 Markdown"
                    }
                    onClick={() => void exportRecord()}
                  >
                    导出 Markdown
                  </button>
                  <button
                    disabled={busy}
                    onClick={() => setConfirmation("trash")}
                  >
                    移到回收站
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
          title={loading ? "正在打开…" : "内容暂不可用"}
          text={loading ? "读取本地记忆" : "内容可能已被删除，请返回列表刷新。"}
        />
      ) : (
        <>
          <div className="memory-detail-scroll">
            <header className="memory-title-block">
              <span className="eyebrow">
                <Icon name="leaf" size={14} />
                {detail.current
                  ? `${detail.current.reason === "cleanup" ? "AI 整理，经我确认" : detail.current.actor === "user" ? (detail.current.reason === "create" ? "我记录的" : "我编辑的") : "AI 整理"} · ${date(detail.current.created_at)}`
                  : "原话已保存在本机"}
              </span>
              <h1>
                <Highlight text={detail.title} query={query} />
              </h1>
              <p>
                {detail.current
                  ? `${detail.current.capture_ids.length} 份输入归档`
                  : "输入归档，仅供核对与恢复，不参与检索。"}
              </p>
              {!trashed && <OrganizationReceipt presentation="status" record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />}
            </header>
            {!trashed && detail.current && <MemoryCollections record={record} currentVersion={detail.current?.id} onRefresh={() => onChanged()} />}
            {detail.reviewed_conclusion && !cleaning && !editing && <div className="workspace-warning">
              <p>目标记忆在确认保存前发生了变化。你的审核稿已保存在本机，尚未写入目标记忆。</p>
              <details><summary>查看保留的完整审核稿</summary><h3>{detail.reviewed_conclusion.title}</h3><p className="readable-text">{detail.reviewed_conclusion.body}</p></details>
              <p className="field-help">请从原讨论重新打开结论审核；输入归档仅供核对。</p>
            </div>}
            <div className="detail-tabs" hidden={cleaning} role="tablist" aria-label="记忆内容">
              <button
                role="tab"
                aria-selected={tab === "current"}
                onClick={() => setTab("current")}
              >
                当前内容
              </button>
              <button
                role="tab"
                aria-selected={tab === "sources"}
                onClick={() => setTab("sources")}
              >
                输入归档与来源 <span>{detail.source_count ?? detail.sources.length}</span>
              </button>
              <button
                role="tab"
                aria-selected={tab === "history"}
                onClick={() => setTab("history")}
              >
                版本历史 <span>{detail.history_count ?? detail.history.length}</span>
              </button>
            </div>
            <div role="tabpanel">
              {archiveLoading && <p role="status" className="field-help">正在读取归档…</p>}
              {!cleaning && !editing && !trashed && receipt && detail.current?.reason === "cleanup" && receipt.after_version === detail.current.id && <div className="cleanup-receipt" role="status">整理已保存为新版本<button className="quiet" disabled={busy} onClick={() => void action("undo")}>撤销这次整理</button></div>}
              {tab === "history" && !trashed && <>
                {receipt && <button className="toolbar-button" disabled={busy} onClick={() => void action("undo")}>撤销这次修改</button>}
                <OrganizationReceipt record={record} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />
              </>}
              {cleaning && cleanupToolbar && <MemoryCleanup key={detail.key.id} detail={detail} toolbar={cleanupToolbar}
                onClose={() => setCleaning(false)}
                onSaved={r => { setCleaning(false); setEditing(false); setReceipt(r); undoId.current = uid(); setNotice("整理已保存，可在下方撤销这次整理。"); onChanged(record, r); }} />}
              {!cleaning && tab === "current" &&
                (editing && !trashed ? (
                  <Editor
                    detail={detail}
                    onClose={() => setEditing(false)}
                    onSaved={(r) => {
                      setEditing(false);
                      setReceipt(r);
                      undoId.current = uid();
                      setNotice("当前内容已保存为新版本，原话保持原样。");
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
                  {detail.sources.map((s) => (
                    <article key={s.id} className="source-block">
                      <div className="section-heading">
                        <strong>
                          {s.capture
                            ? sourceName(s.capture.origin)
                            : "来源不可用"}
                        </strong>
                        <span>{s.capture && date(s.capture.created_at)}</span>
                      </div>
                      {s.capture ? (
                        <>
                          <div className="readable-text">
                            <Highlight text={s.capture.text} query={query} />
                          </div>
                          {s.capture.origin.project && (
                            <p className="source-metadata">
                              项目 · {s.capture.origin.project}
                            </p>
                          )}
                          {s.capture.origin.uri && (
                            <p className="source-metadata">
                              附带来源 · <span>{s.capture.origin.uri}</span>
                            </p>
                          )}
                          {!trashed && detail.current && <button className="outline-button" disabled={busy || editing} onClick={() => {
                            setArchive({ id: s.id, text: s.capture!.text }); actionId.current = uid(); setConfirmation("restore_archive");
                          }}>恢复这份输入为正文</button>}
                          <small>
                            原始输入 · {fullDate(s.capture.created_at)}
                          </small>
                        </>
                      ) : (
                        <p className="field-help">
                          这段原话已删除或不可用，无法展示原文。
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
                            v{detail.history.length - i} ·{" "}
                            {v.reason === "cleanup" ? "AI 整理，经我确认" : v.actor === "user" ? "我" : "AI"}
                            {v.id === detail.current?.id ? " · 当前" : ""}
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
                              恢复此版本
                            </button>
                          )}
                        </div>
                        <Markdown text={version.body} />
                        <p className="field-help">
                          依据 {version.capture_ids.length}{" "}
                          段原话。恢复会新增版本，保留现有历史。
                        </p>
                      </article>
                    ) : (
                      <p className="field-help">
                        选择一个版本，核对当时保存的内容。
                      </p>
                    )}
                  </div>
                ) : (
                  <Empty
                    title="原话始终保留"
                    text="编辑并保存当前内容后，版本会出现在这里。"
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
              ? "将这条记忆移到回收站？"
              : confirmation === "purge"
                ? "永久删除这条记忆？"
                : confirmation === "restore_archive" ? "恢复这份输入为正文？" : "恢复所选版本？"
          }
          onClose={() => {
            if (!busy) setConfirmation(null);
          }}
        >
          <p>
            {confirmation === "trash"
              ? "当前内容、专属原话和历史会进入回收站，不再参与普通搜索。共享来源会保留，你可以随时恢复。"
              : confirmation === "purge"
                ? "这会永久擦除当前内容、专属原话和历史，无法通过 Memivy 恢复。"
                : "所选内容会成为一个新版本；现在的版本仍保留在历史中。"}
          </p>
          {confirmation === "restore_archive" && archive && <div className="readable-text">{archive.text}</div>}
          <ErrorNotice text={error} />
          <div className="action-row">
            <button
              className="outline-button"
              disabled={busy}
              autoFocus
              onClick={() => setConfirmation(null)}
            >
              取消
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
                ? "处理中…"
                : confirmation === "trash"
                  ? "移到回收站"
                  : confirmation === "purge"
                    ? "永久删除"
                    : "确认恢复"}
            </button>
          </div>
        </Modal>
      )}
    </section>
  );
}
