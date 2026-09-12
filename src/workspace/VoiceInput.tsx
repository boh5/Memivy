import { Icon } from "../ui";
import type { useVoice } from "./useVoice";
import { useTranslation } from "react-i18next";
import { translateCatalog } from "../i18n";
import "./voice.css";
type Voice = ReturnType<typeof useVoice>;
type VoiceSource = "local" | "service";
function recognitionLabel(t: (key: "voice.localRecognition") => string, source?: VoiceSource, label?: string) {
  return source === "local" && (!label || label === "voice_local") ? t("voice.localRecognition") : label || t("voice.localRecognition");
}
export function VoiceButton({voice,disabled}:{voice:Voice;disabled:boolean}) {
  const { t } = useTranslation("workspace");
  const recording = voice.active && !!(voice.session?.recording || voice.session?.starting);
  const processing = voice.active && !recording;
  const label = processing ? t("voice.transcribing") : recording ? t("voice.stopAndTranscribe") : t("voice.input");
  const recognition = recognitionLabel(t, voice.status?.source, voice.status?.label);
  return <button type="button" className={`voice-button ${recording?"is-recording":""} ${processing?"is-processing":""}`} disabled={disabled||voice.busy||processing}
    onClick={()=>void voice.toggle()} aria-label={label} title={voice.active?label:t("voice.title", { label: t("voice.input"), recognition })} aria-pressed={recording}>
    {recording ? <span className="voice-stop" aria-hidden="true"/> : processing ? <span className="voice-spinner" aria-hidden="true"/> : <Icon name="mic" size={17}/>}
  </button>;
}
export function VoiceFeedback({voice}:{voice:Voice}) {
  const { t } = useTranslation("workspace");
  const v=voice.session;
  const displayError = (code: string | null | undefined) => code ? translateCatalog(code, { ns: "errors" }) : "";
  return <>
    {v&&<div className={`voice-feedback ${voice.active?"is-active":""}`} role="status">
      <div className="voice-wave" aria-hidden="true">{[.45,.8,1,.65,.95,.55,.8].map((n,i)=><i key={i} style={{height:`${4+Math.min(1,v.level*18)*19*n}px`}}/>)}</div>
      <span className="voice-state">{v.error?t("voice.recordingSaved"):v.starting?t("voice.openingMicrophone"):v.recording?t("voice.listening"):v.processing?t("voice.transcribingStatus"):t("voice.transcribedEditable")}</span>
      <small>{recognitionLabel(t, v.source, v.label)}</small><span className="voice-time">{Math.floor(v.seconds/60)}:{String(Math.floor(v.seconds%60)).padStart(2,"0")}</span>
      {v.recording&&voice.status?.state==="loading"&&<small>{t("voice.modelLoading")}</small>}
      {!voice.active&&<button type="button" className="voice-text-button" onClick={()=>void voice.clear()} title={t("voice.removeTitle")}>{t("voice.remove")}</button>}
      {v.error&&!voice.active&&<button type="button" className="voice-text-button" onClick={()=>void voice.retry()}>{t("voice.retry")}</button>}
    </div>}
    {voice.conflicted&&v?.complete&&<div className="voice-recovery"><p>{v.text}</p><button className="outline-button" onClick={voice.recover}>{t("voice.appendDraft")}</button></div>}
    {(voice.error||v?.error)&&<div className="voice-error" role="alert">{voice.error||displayError(v?.error)}</div>}
  </>;
}
