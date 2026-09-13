import { useState } from "react";
import { useTranslation } from "react-i18next";
import { call, errorText, type Draft } from "./api";
import { ErrorNotice, Modal } from "./components";
import type { useDraft } from "./useDraft";
import { useNotice } from "../i18n/react";

export default function DraftConflict({ draft }: { draft: ReturnType<typeof useDraft> }) {
  const [other, setOther] = useState<{ value: Draft | null } | null>(null);
  const { t } = useTranslation("workspace");
  const [error, setError] = useNotice();
  async function review() {
    try { setOther({ value: await call<Draft | null>("draft_read", { key: draft.value.key }) }); setError(""); }
    catch (e) { setError(errorText(e)); }
  }
  async function resolve(local: boolean) {
    try { await draft.resolve(local, other?.value?.request_id || null); setOther(null); }
    catch (e) { await review(); setError(errorText(e)); }
  }
  return <>
    {draft.conflicted && <button className="outline-button" onClick={() => void review()}>{t("draft.review")}</button>}
    {other && <Modal title={t("draft.title")} onClose={() => setOther(null)}>
      <p>{t("draft.description")}</p>
      {[{ key: "thisWindow", value: draft.value }, { key: "otherWindow", value: other.value }].map(({ key, value }) => {
        const label = key === "thisWindow" ? t("draft.thisWindow") : t("draft.otherWindow");
        return (
          <div key={key}>
            {value?.destination && <p className="field-help">{label} · {value.destination.kind === "new" ? t("draft.newMemory") : t("draft.existingMemory")} · {value.title}</p>}
            <label className="discussion-field">{label}<textarea rows={4} readOnly value={value?.body || ""} /></label>
          </div>
        );
      })}
      <ErrorNotice text={error} />
      <div className="action-row">
        <button className="outline-button" onClick={() => void resolve(false)}>{t("draft.useOther")}</button>
        <button className="send-button" onClick={() => void resolve(true)}>{t("draft.useThis")}</button>
      </div>
    </Modal>}
  </>;
}
