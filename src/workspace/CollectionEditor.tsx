import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { call, errorText, uid, type Collection } from "./api";
import { ErrorNotice, Modal } from "./components";
import { useNotice } from "../i18n/react";

export default function CollectionEditor({ value, onSaved, onClose }: {
  value?: Collection; onSaved: (id: string) => void; onClose: () => void;
}) {
  const { t } = useTranslation("workspace");
  const [name, setName] = useState(value?.name || "");
  const [description, setDescription] = useState(value?.description || "");
  const [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const id = useRef(value?.id || uid()), lock = useRef(false);
  async function save() {
    if (lock.current || !name.trim()) return;
    lock.current = true; setBusy(true); setError("");
    try {
      await call("navigation_save_collection", { id: id.current, name: name.trim(), description: description.trim(), expected: value?.revision ?? null });
      onSaved(id.current);
    } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); }
  }
  return <Modal title={value ? t("collection.editTitle") : t("collection.newTitle")} onClose={() => { if (!lock.current) onClose(); }} className="collection-dialog">
    <p className="field-help">{t("collection.description")}</p>
    <label>{t("collection.nameLabel")}<input autoFocus aria-label={t("collection.nameAria")} maxLength={80} value={name} disabled={busy} placeholder={t("collection.namePlaceholder")} onChange={e => setName(e.target.value)} /></label>
    <label>{t("collection.focusLabel")}<textarea aria-label={t("collection.focusAria")} maxLength={800} value={description} disabled={busy} placeholder={t("collection.focusPlaceholder")} onChange={e => setDescription(e.target.value)} /></label>
    <ErrorNotice text={error} />
    <div className="action-row"><button className="outline-button" disabled={busy} onClick={onClose}>{t("collection.cancel")}</button><button className="send-button" disabled={busy || !name.trim()} onClick={() => void save()}>{busy ? t("collection.saving") : value ? t("collection.save") : t("collection.create")}</button></div>
  </Modal>;
}
