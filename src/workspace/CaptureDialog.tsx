import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { finishVoiceInputs } from "./useVoice";
import CaptureForm from "./CaptureForm";
import { ErrorNotice, Modal } from "./components";
import { errorText, type Key } from "./api";
import { flushDraft } from "./useDraft";
import { useNotice } from "../i18n/react";

export default function CaptureDialog({ quick, sourceApp, focus, onReady, onSaved, onClose }: {
  quick: boolean;
  sourceApp?: string;
  focus: number;
  onReady: () => void;
  onSaved: (key: Key) => void;
  onClose: () => void;
}) {
  const busy = useRef(false), closing = useRef(false);
  const { t } = useTranslation("workspace");
  const [error, setError] = useNotice();
  async function close() {
    if (busy.current || closing.current) return;
    closing.current = true;
    try {
      // Closing this input must not depend on an unrelated editor's draft.
      await finishVoiceInputs();
      await flushDraft(quick ? "quick_capture" : "capture");
      onClose();
    } catch (e) { setError(errorText(e)); }
    finally { closing.current = false; }
  }
  return <Modal title={t("capture.title")} className="capture-dialog" onClose={() => void close()}>
    <p className="capture-dialog-intro">{t("capture.intro")}</p>
    <CaptureForm quick={quick} sourceApp={sourceApp} focus={focus} onReady={onReady}
      mode="capture" presentation="capture" onMode={() => {}} onAsk={async () => {}}
      onEdit={() => setError("")} onBusy={value => { busy.current = value; }} onSaved={onSaved} />
    <ErrorNotice text={error} />
  </Modal>;
}
