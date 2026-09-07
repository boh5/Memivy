import { useEffect, useRef, useState } from "react";
import { call, errorText, uid, type Detail, type Message, type Page, type Receipt, type Source, type Topic } from "./api";
import { ErrorNotice, Modal } from "./components";

type Destination = { kind: "new" } | { kind: "existing"; memory_id: string; expected_version: string };
export default function SaveConclusion({ message, topic, context, onClose, onSaved }: {
  message: Message; topic: Topic; context: Source[]; onClose: () => void; onSaved: (receipt: Receipt) => void;
}) {
  const [title, setTitle] = useState(topic.title), [text, setText] = useState(message.answer?.conclusion || message.text);
  const [suggestions, setSuggestions] = useState<[string, string][]>([]);
  const [selection, setSelection] = useState("");
  const [query, setQuery] = useState(""), [rows, setRows] = useState<Page["items"]>([]);
  const [target, setTarget] = useState<Detail | null>(null), [merged, setMerged] = useState<string | null>(null);
  const [busy, setBusy] = useState(false), [previewing, setPreviewing] = useState(false), [error, setError] = useState("");
  const mergedField = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => { if (merged !== null) mergedField.current?.scrollIntoView({ block: "center" }); }, [merged !== null]);
  const locked = useRef(false), attempt = useRef(uid()), generation = useRef(0), alive = useRef(true);
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
  function changed() { attempt.current = uid(); generation.current++; setMerged(null); setPreviewing(false); }
  async function select(id: string) {
    changed(); setSelection(id); setError(""); setTarget(null);
    if (!id) return;
    const run = generation.current;
    setBusy(true);
    try {
      const detail = await call<Detail>("library_detail", { key: { kind: "memory", id } });
      if (alive.current && run === generation.current) { setTarget(detail); setTitle(detail.title); }
    } catch (e) { if (alive.current) setError(errorText(e)); }
    finally { if (alive.current) setBusy(false); }
  }
  const destination: Destination = target?.current ? { kind: "existing", memory_id: target.key.id, expected_version: target.current.id } : { kind: "new" };
  async function preview() {
    if (!target || locked.current) return;
    const run = ++generation.current;
    setPreviewing(true); setError("");
    try {
      const body = await call<string>("discussion_merge", { destination, text });
      if (alive.current && run === generation.current) { setMerged(body); attempt.current = uid(); }
    } catch (e) { if (alive.current && run === generation.current) setError(errorText(e)); }
    finally { if (alive.current && run === generation.current) setPreviewing(false); }
  }
  async function save() {
    if (locked.current || busy || previewing || (selection && !target)) return;
    locked.current = true; setBusy(true); setError("");
    try {
      const receipt = await call<Receipt>("discussion_save", { request: { request_id: attempt.current, message_id: message.id, destination, title, text }, mergedBody: merged });
      onSaved(receipt);
    } catch (e) { setError(errorText(e)); }
    finally { locked.current = false; setBusy(false); }
  }
  return <Modal title="留下这段结论" onClose={() => { if (!busy) onClose(); }}>
    <p className="field-help">核对文字和去向后确认保存。讨论本身不会进入记忆库。</p>
    <label className="discussion-field">要保存的结论<textarea rows={5} value={text} disabled={busy} onChange={e => { setText(e.target.value); changed(); }} /></label>
    <label className="discussion-field">保存去向<select aria-label="结论保存去向" value={selection} disabled={busy} onChange={e => void select(e.target.value)}>
      <option value="">新建记忆</option>
      {target && !rows.some(r => r.key.id === target.key.id) && <option value={target.key.id}>{target.title}</option>}
      {suggestions.filter(([id]) => !rows.some(r => r.key.id === id) && id !== target?.key.id).map(([id, name]) => <option key={id} value={id}>{name} · 本次讨论</option>)}
      {rows.map(row => <option key={row.key.id} value={row.key.id}>{row.title}{suggestions.some(([id]) => id === row.key.id) ? " · 本次讨论" : ""}</option>)}
    </select></label>
    <label className="discussion-field">查找已有记忆<input value={query} disabled={busy} placeholder="输入标题或关键词" onChange={e => setQuery(e.target.value)} /></label>
    <label className="discussion-field">{target ? "保存后的标题" : "标题"}<input value={title} disabled={busy} onChange={e => { setTitle(e.target.value); attempt.current = uid(); }} /></label>
    {target && <>
      <p className="field-help">默认把结论补充到「{target.title}」，保留原有正文。</p>
      <details><summary>查看当前正文</summary><p className="readable-text">{target.body}</p></details>
      <div className="action-row"><button className="outline-button" disabled={busy || previewing || !text.trim()} onClick={() => void preview()}>{previewing ? "正在生成融合预览…" : "预览融合成文"}</button>
        {(merged !== null || previewing) && <button className="quiet" disabled={busy} onClick={changed}>使用默认补充</button>}
      </div>
    </>}
    {merged !== null && <label className="discussion-field">审核融合后的完整正文<textarea ref={mergedField} rows={10} value={merged} disabled={busy} onChange={e => { setMerged(e.target.value); attempt.current = uid(); }} /><span className="field-help">确认后按这里的文字保存，不再自动改写。</span></label>}
    <ErrorNotice text={error} />
    <div className="action-row"><button className="send-button" disabled={busy || previewing || !!(selection && !target) || !title.trim() || !text.trim() || merged?.trim() === ""} onClick={() => void save()}>{busy ? "保存中…" : merged !== null ? "确认融合并保存" : target ? "确认补充到记忆" : "确认保存新记忆"}</button><button className="outline-button" disabled={busy} onClick={onClose}>取消</button></div>
  </Modal>;
}
