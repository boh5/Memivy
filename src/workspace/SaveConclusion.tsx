import MarkdownEditor from "./MarkdownEditor";
import Markdown from "./Markdown";
import { useEffect, useRef, useState } from "react";
import { call, errorText, type Detail, type Message, type Page, type Receipt, type Source, type Topic, type ConclusionDestination } from "./api";
import { ErrorNotice, Modal } from "./components";

import { useDraft } from "./useDraft";
import DraftConflict from "./DraftConflict";
export default function SaveConclusion({ message, topic, context, onClose, onSaved }: {
  message: Message; topic: Topic; context: Source[]; onClose: () => void; onSaved: (receipt: Receipt) => void;
}) {
  const draft = useDraft(`conclusion:${message.id}`, {
    title: topic.title, body: message.answer?.conclusion || message.text,
    expected_version: null, conclusion: { destination: { kind: "new" }, merged_body: null },
  });
  const { title, body: text } = draft.value;
  const destination: ConclusionDestination = draft.value.conclusion?.destination || { kind: "new" };
  const merged = draft.value.conclusion?.merged_body ?? null;
  const [unresolvedSelection, setUnresolvedSelection] = useState<string | null>(null);
  const selection = unresolvedSelection ?? (destination.kind === "existing" ? destination.memory_id : "");
  const setTitle = (value: string) => draft.update({ title: value });
  const setText = (value: string) => draft.update({ body: value });
  const setMerged = (value: string | null) => draft.update({ conclusion: { destination, merged_body: value } });
  const [suggestions, setSuggestions] = useState<[string, string][]>([]);
  const [query, setQuery] = useState(""), [rows, setRows] = useState<Page["items"]>([]);
  const [target, setTarget] = useState<Detail | null>(null);
  const [needsCheck, setNeedsCheck] = useState(false);
  const [busy, setBusy] = useState(false), [previewing, setPreviewing] = useState(false), [error, setError] = useState("");
  const mergedField = useRef<HTMLDivElement | null>(null);
  useEffect(() => { if (merged !== null) mergedField.current?.scrollIntoView({ block: "center" }); }, [merged !== null]);
  const locked = useRef(false), generation = useRef(0), alive = useRef(true);
  useEffect(() => { generation.current++; setPreviewing(false); }, [draft.value.request_id]);
  useEffect(() => { alive.current = true; return () => { alive.current = false; generation.current++; }; }, []);
  useEffect(() => {
    let valid = true;
    const timer = setTimeout(() => { void call<Page>("library_query", { query: { query, trash: false, limit: 30 } }).then(page => { if (valid) setRows(page.items.filter(r => r.key.kind === "memory")); }).catch(e => { if (valid) setError(errorText(e)); }); }, 150);
    return () => { valid = false; clearTimeout(timer); };
  }, [query]);
  useEffect(() => {
    let valid = true;
    void call<[string, string][]>("discussion_targets", { sources: context }).then(value => { if (valid) setSuggestions(value); }).catch(e => { if (valid) setError(errorText(e)); });
    return () => { valid = false; };
  }, [JSON.stringify(context)]);
  function changed() { generation.current++; setMerged(null); setPreviewing(false); }
  // Loading a saved destination never silently rebases a reviewed rewrite.
  useEffect(() => {
    let valid = true;
    setTarget(null);
    if (!draft.ready || destination.kind !== "existing") { setNeedsCheck(false); return; }
    const expected = destination.expected_version;
    void call<Detail>("library_detail", { key: { kind: "memory", id: destination.memory_id } }).then(detail => {
      if (!valid) return;
      if (detail.state !== "active" || detail.current?.id !== expected) {
        setNeedsCheck(true); setError("保存目标已经变化。审核稿已保留，请重新核对去向和正文。");
      } else { setTarget(detail); setNeedsCheck(false); }
    }).catch(e => { if (valid) { setNeedsCheck(true); setError(errorText(e)); } });
    return () => { valid = false; };
  }, [draft.ready, destination.kind === "existing" ? destination.memory_id : "", destination.kind === "existing" ? destination.expected_version : ""]);
  async function select(id: string, recheck = false) {
    generation.current++; setPreviewing(false); setError(""); setTarget(null); setUnresolvedSelection(id);
    if (!id) {
      draft.update({ conclusion: { destination: { kind: "new" }, merged_body: null } });
      setTarget(null); setNeedsCheck(false); setUnresolvedSelection(null); return;
    }
    const run = generation.current;
    setBusy(true);
    try {
      const detail = await call<Detail>("library_detail", { key: { kind: "memory", id } });
      if (detail.state !== "active" || !detail.current) throw new Error("这条记忆已不可用，请选择其他保存去向。");
      if (alive.current && run === generation.current) {
        setTarget(detail); setNeedsCheck(false); setUnresolvedSelection(null);
        draft.update({ title: recheck ? title : detail.title, conclusion: {
          destination: { kind: "existing", memory_id: id, expected_version: detail.current.id },
          merged_body: recheck ? merged : null,
        } });
      }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { if (alive.current) setBusy(false); }
  }
  async function preview() {
    if (!target || locked.current) return;
    const run = ++generation.current;
    setPreviewing(true); setError("");
    try {
      const body = await call<string>("discussion_merge", { destination, text });
      if (alive.current && run === generation.current) { setMerged(body); }
    } catch (e) { if (alive.current && run === generation.current) setError(errorText(e)); }
    finally { if (alive.current && run === generation.current) setPreviewing(false); }
  }
  async function save() {
    if (locked.current || !draft.ready || busy || previewing || needsCheck || (selection && !target)) return;
    locked.current = true; setBusy(true); setError("");
    let persisted = false;
    try {
      // Persist the exact reviewed payload before any durable memory mutation.
      const saved = await draft.flush(true);
      persisted = true;
      const receipt = await call<Receipt>("discussion_save", { request: { request_id: saved.request_id, message_id: message.id, destination: saved.conclusion!.destination, title: saved.title, text: saved.body }, mergedBody: saved.conclusion!.merged_body });
      if (receipt.status === "needs_review") {
        setNeedsCheck(true);
        setError("保存目标已经变化。完整审核稿已保存在本机，请重新核对去向和正文。");
        return;
      }
      // Core already consumed the exact submitted draft atomically. A local
      // cache refresh failure must not turn a successful save into a retry.
      await draft.clear(saved.request_id).catch(() => {});
      onSaved(receipt);
    } catch (e) {
      setError(`${errorText(e)} ${persisted ? "未能确认保存结果，请保持文字不变并重试。" : "这份审核稿尚未保存，请解决草稿冲突或存储问题后重试。"}`);
    }
    finally { locked.current = false; setBusy(false); }
  }
  return <Modal title="留下这段结论" onClose={() => { if (!busy) onClose(); }}>
    <p className="field-help">核对文字和去向后确认保存。讨论本身不会进入记忆库。</p>
    <MarkdownEditor label="要保存的结论" value={text} disabled={busy || !draft.ready} onChange={value => { setText(value); changed(); }} />
    <label className="discussion-field">保存去向<select aria-label="结论保存去向" value={selection} disabled={busy || !draft.ready} onChange={e => void select(e.target.value)}>
      <option value="">新建记忆</option>
      {selection && !target && !rows.some(r => r.key.id === selection) && <option value={selection}>原保存目标 · 待核对</option>}
      {target && !rows.some(r => r.key.id === target.key.id) && <option value={target.key.id}>{target.title}</option>}
      {suggestions.filter(([id]) => !rows.some(r => r.key.id === id) && id !== target?.key.id).map(([id, name]) => <option key={id} value={id}>{name} · 本次讨论</option>)}
      {rows.map(row => <option key={row.key.id} value={row.key.id}>{row.title}{suggestions.some(([id]) => id === row.key.id) ? " · 本次讨论" : ""}</option>)}
    </select></label>
    <label className="discussion-field">查找已有记忆<input value={query} disabled={busy || !draft.ready} placeholder="输入标题或关键词" onChange={e => setQuery(e.target.value)} /></label>
    <label className="discussion-field">{target ? "保存后的标题" : "标题"}<input value={title} disabled={busy || !draft.ready} onChange={e => { setTitle(e.target.value); }} /></label>
    {needsCheck && selection && <button className="outline-button" disabled={busy} onClick={() => void select(selection, true)}>重新读取目标并核对</button>}
    {target && <>
      <p className="field-help">默认把结论补充到「{target.title}」，保留原有正文。</p>
      <details><summary>查看当前正文</summary><Markdown text={target.body} /></details>
      <div className="action-row"><button className="outline-button" disabled={busy || previewing || !text.trim()} onClick={() => void preview()}>{previewing ? "正在生成融合预览…" : "预览融合成文"}</button>
        {(merged !== null || previewing) && <button className="quiet" disabled={busy || !draft.ready} onClick={changed}>使用默认补充</button>}
      </div>
    </>}
    {merged !== null && <div ref={mergedField}><MarkdownEditor label="审核融合后的完整正文" value={merged} disabled={busy || !draft.ready} onChange={value => { generation.current++; setPreviewing(false); setMerged(value); }} /><p className="field-help">确认后按这里的文字保存，不再自动改写。</p></div>}
    <ErrorNotice text={error || draft.error} />
    <DraftConflict draft={draft} />
    <div className="action-row"><button className="send-button" disabled={!draft.ready || needsCheck || busy || previewing || !!(selection && !target) || !title.trim() || !text.trim() || merged?.trim() === ""} onClick={() => void save()}>{busy ? "保存中…" : merged !== null ? "确认融合并保存" : target ? "确认补充到记忆" : "确认保存新记忆"}</button><button className="outline-button" disabled={busy || !draft.ready} onClick={onClose}>取消</button></div>
  </Modal>;
}
