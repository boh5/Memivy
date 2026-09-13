import Select from "./Select";
import { useNotice } from "../i18n/react";
import { useTranslation } from "react-i18next";
import MarkdownEditor from "./MarkdownEditor";
import { useEffect, useRef, useState } from "react";
import { call, errorText, type Detail, type Message, type Page, type Topic, type ConclusionDestination } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useDraft } from "./useDraft";
import DraftConflict from "./DraftConflict";

export default function SaveText({ message, topic, onClose, onSaved }: { message: Message; topic: Topic; onClose: () => void; onSaved: () => void }) {
  const { t } = useTranslation("workspace");
  const draft = useDraft(`save:${message.id}`, { title: topic.title, body: message.text, expected_version: null, destination: { kind: "new" } });
  const destination: ConclusionDestination = draft.value.destination || { kind: "new" };
  const [query, setQuery] = useState(""), [rows, setRows] = useState<Page["items"]>([]), [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const lock = useRef(false);
  const [unresolved, setUnresolved] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const timer = setTimeout(() => { void call<Page>("library_query", { query: { query, trash: false, limit: 30 } }).then(page => { if (active) setRows(page.items.filter(row => row.key.kind === "memory")); }).catch(e => { if (active) setError(errorText(e)); }); }, 120);
    return () => { active = false; clearTimeout(timer); };
  }, [query]);
  async function select(id: string) {
    if (lock.current) return;
    if (!id) { setUnresolved(null); draft.update({ destination: { kind: "new" } }); return; }
    setUnresolved(id);
    lock.current = true; setBusy(true); setError("");
    try {
      const detail = await call<Detail>("library_detail", { key: { kind: "memory", id } });
      if (detail.state !== "active" || !detail.current) throw { code: "unavailable" };
      draft.update({ destination: { kind: "existing", memory_id: id, expected_version: detail.current.id } }); setUnresolved(null);
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  async function save() {
    if (lock.current || !draft.ready || unresolved) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const value = await draft.flush(true);
      await call("discussion_save_text", { id: value.request_id, inputId: message.turn_id, text: value.body, title: value.title, destination: value.destination });
      await draft.clear(value.request_id); onSaved();
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  return <Modal title={t("input.saveText")} onClose={() => { if (!busy) onClose(); }}>
    <MarkdownEditor label={t("input.textToSave")} value={draft.value.body} disabled={busy || !draft.ready} onChange={body => draft.update({ body })} />
    <label className="discussion-field">{t("saveConclusion.titleLabel")}<input value={draft.value.title} disabled={busy || !draft.ready} onChange={e => draft.update({ title: e.target.value })} /></label>
    <label className="discussion-field">{t("saveConclusion.searchLabel")}<input value={query} disabled={busy} onChange={e => setQuery(e.target.value)} /></label>
    <label className="discussion-field">{t("saveConclusion.destinationLabel")}<Select aria-label={t("saveConclusion.destinationAria")} value={unresolved ?? (destination.kind === "existing" ? destination.memory_id : "")} disabled={busy || !draft.ready} onChange={e => void select(e.target.value)}>
      <option value="">{t("saveConclusion.newMemory")}</option>
      {destination.kind === "existing" && !rows.some(row => row.key.id === destination.memory_id) && <option value={destination.memory_id}>{t("saveConclusion.unresolvedDestination")}</option>}
      {rows.map(row => <option key={row.key.id} value={row.key.id}>{row.title}</option>)}
    </Select></label>
    {destination.kind === "existing" && <p className="field-help">{t("input.appendHelp")}</p>}
    <ErrorNotice text={error || draft.error} /><DraftConflict draft={draft} />
    <div className="action-row"><button className="send-button" disabled={busy || !draft.ready || !!unresolved || !draft.value.title.trim() || !draft.value.body.trim()} onClick={() => void save()}>{busy ? t("saveConclusion.saving") : t("input.saveText")}</button><button className="outline-button" disabled={busy} onClick={onClose}>{t("saveConclusion.cancel")}</button></div>
  </Modal>;
}
