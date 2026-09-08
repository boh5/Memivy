import RelatedMemories from "./RelatedMemories";
import OrganizationReceipt from "./OrganizationReceipt";
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
  const bodyInput = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (draft.ready) bodyInput.current?.focus();
  }, [draft.ready]);
  const conflict =
    draft.ready &&
    draft.value.expected_version !== (detail.current?.id || null);
  async function save() {
    if (lock.current || !draft.ready) return;
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
            : draft.saved
              ? "草稿已保存在本机"
              : "保存草稿中…"}
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
      <label>
        当前内容
        <textarea
          ref={bodyInput}
          aria-label="编辑记忆内容"
          value={draft.value.body}
          disabled={!draft.ready || busy}
          onChange={(e) => draft.update({ body: e.target.value })}
          onKeyDown={(e) => {
            if (e.metaKey && e.key === "s") {
              e.preventDefault();
              void save();
            }
          }}
        />
      </label>
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
          <div className="readable-text">{detail.body}</div>
        </Modal>
      )}
    </div>
  );
}

export default function MemoryDetail({
  record,
  revision,
  query,
  initialReceipt,
  onChanged,
  onDiscuss,
  onBack,
}: {
  record: Key;
  initialReceipt: Receipt | null;
  revision: number;
  query: string;
  onChanged: (key?: Key, receipt?: Receipt) => void;
  onBack: () => void;
  onDiscuss: (detail: Detail, related?: Source[]) => Promise<void>;
}) {
  const [detail, setDetail] = useState<Detail | null>(null),
    [error, setError] = useState(""),
    [loading, setLoading] = useState(true),
    [exportNotice, setExportNotice] = useState("");
  const [tab, setTab] = useState<"current" | "sources" | "history">("current"),
    [editing, setEditing] = useState(false),
    [version, setVersion] = useState<Version | null>(null);
  const [confirmation, setConfirmation] = useState<
      "trash" | "purge" | "restore_version" | null
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
  const actionId = useRef(uid()),
    undoId = useRef(uid()),
    actionLock = useRef(false);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError("");
    void call<Detail>("library_detail", { key: record })
      .then((d) => {
        if (alive) setDetail(d);
      })
      .catch((e) => {
        if (alive) {
          setDetail(null);
          setError(errorText(e));
        }
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [record.id, record.kind, revision]);
  async function action(
    kind: "trash" | "purge" | "restore" | "restore_version" | "undo",
  ) {
    if (!detail || actionLock.current) return;
    actionLock.current = true;
    setBusy(true);
    setError("");
    try {
      const payload =
        kind === "restore_version"
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
        onChanged(record);
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
        <div className="toolbar-actions">
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
            ) : (
              <>
                <button
                  disabled={busy || loading || editing}
                  onClick={() => {
                    setError("");
                    setBusy(true);
                    void onDiscuss(detail)
                      .catch((e) => setError(errorText(e)))
                      .finally(() => setBusy(false));
                  }}
                >
                  <Icon name="chat" size={14} />
                  接着想
                </button>
                <button
                  disabled={busy || loading}
                  onClick={() => {
                    setTab("current");
                    setEditing(true);
                  }}
                >
                  编辑
                </button>
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
      </div>
      <ErrorNotice text={error} />
      {exportNotice && (
        <p className="record-export-notice" role="status">
          {exportNotice}
        </p>
      )}
      {detail && !trashed && <OrganizationReceipt record={record} revision={revision} onOpen={key => onChanged(key)} onRefresh={() => onChanged(record)} />}
      {notice && (
        <div className="mutation-receipt" role="status">
          <Icon name="check" size={16} />
          <span>{notice}</span>
          {receipt && (
            <button disabled={busy} onClick={() => void action("undo")}>
              撤销这次修改
            </button>
          )}
        </div>
      )}
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
                  ? `${detail.current.actor === "user" ? "我编辑的" : "AI 整理"} · ${date(detail.current.created_at)}`
                  : "原话已保存在本机"}
              </span>
              <h1>
                <Highlight text={detail.title} query={query} />
              </h1>
              <p>
                {detail.current
                  ? `${detail.current.capture_ids.length} 段原话作为来源`
                  : "尚未整理；你可以直接阅读、搜索或编辑成记忆。"}
              </p>
            </header>
            {detail.reviewed_conclusion && !editing && <div className="workspace-warning">
              <p>目标记忆在确认保存前发生了变化。你的审核稿已保存在本机，尚未写入目标记忆。</p>
              <details><summary>查看保留的完整审核稿</summary><h3>{detail.reviewed_conclusion.title}</h3><p className="readable-text">{detail.reviewed_conclusion.body}</p></details>
              <button onClick={() => { setTab("current"); setEditing(true); }}>审核保留稿并另存</button>
            </div>}
            <div className="detail-tabs" role="tablist" aria-label="记忆内容">
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
                原话与来源 <span>{detail.sources.length}</span>
              </button>
              <button
                role="tab"
                aria-selected={tab === "history"}
                onClick={() => setTab("history")}
              >
                版本历史 <span>{detail.history.length}</span>
              </button>
            </div>
            <div role="tabpanel">
              {tab === "current" &&
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
                    <div className="readable-text"><Highlight text={detail.body} query={query} /></div>
                    {!trashed && detail.current && <RelatedMemories
                      key={detail.current.id} memoryId={detail.key.id} versionId={detail.current.id}
                      revision={revision} onOpen={onChanged} onDiscuss={sources => onDiscuss(detail, sources)} />}
                  </>
                ))}
              {tab === "sources" && (
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
                            {v.actor === "user" ? "我" : "AI"}
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
                        <div className="readable-text">{version.body}</div>
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
                : "恢复所选版本？"
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
