import { Icon } from "../ui";
import type { useVoice } from "./useVoice";
import { useTranslation } from "react-i18next";
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
  return <button type="button" className={`voice-button ${recording?"is-recording":""} ${processing?"is-processing":""}`} disabled={disabled||voice.busy||processing||voice.foreign}
    onClick={()=>void voice.toggle()} aria-label={label} title={voice.active?label:t("voice.title", { label: t("voice.input"), recognition })} aria-pressed={recording}>
    {recording ? <span className="voice-stop" aria-hidden="true"/> : processing ? <span className="voice-spinner" aria-hidden="true"/> : <Icon name="mic" size={17}/>}
  </button>;
}
export function VoiceFeedback({voice}:{voice:Voice}) {
  const { t } = useTranslation(["workspace", "errors", "common"]);
  const v=voice.session;
  const noAudio = !voice.active && v?.error === "voice_no_audio";
  const startFailed = !voice.active && v?.error === "microphone_start_timeout";
  const failed = !startFailed && !voice.active && !!v && !!(v.error || voice.error) && !noAudio && !voice.conflicted;
  return <>
    {v && voice.active && <div className="voice-feedback is-active" role="status" aria-label={v.recording ? t("voice.listening") : undefined}>
      {v.recording ? <>
        <div className="voice-wave" aria-hidden="true">{[.45,.8,1,.65,.95,.55,.8].map((n,i)=><i key={i} style={{height:`${4+Math.min(1,v.level*18)*19*n}px`}}/>)}</div>
        <span className="voice-time">{Math.floor(v.seconds/60)}:{String(Math.floor(v.seconds%60)).padStart(2,"0")}</span>
      </> : <span>{t(v.starting ? "voice.openingMicrophone" : "voice.transcribingStatus")}</span>}
    </div>}
    {noAudio && <div className="voice-error" role="alert">{t("voice.noAudioRecorded")}</div>}
    {startFailed && <div className="voice-feedback voice-failed" role="alert">
      <span>{t("errors:microphone_start_timeout")}</span>
      <button type="button" className="voice-text-button" disabled={voice.busy} onClick={()=>void voice.clear()} aria-label={t("common:closeNotice")}><Icon name="close" size={16}/></button>
    </div>}
    {failed && <div className="voice-feedback voice-failed" role="alert">
      <span>{v?.error ? t("voice.transcriptionFailed") : voice.error}</span>
      <button type="button" className="voice-text-button" disabled={voice.busy} onClick={()=>void voice.retry()}>{t("voice.retry")}</button>
      <button type="button" className="voice-text-button" disabled={voice.busy} onClick={()=>void voice.clear()} title={t("voice.removeTitle")}>{t("voice.remove")}</button>
    </div>}
    {voice.conflicted&&v?.complete&&<div className="voice-recovery"><p>{voice.error}</p><p>{v.text}</p><button className="outline-button" disabled={voice.busy} onClick={voice.recover}>{t("voice.appendDraft")}</button><button type="button" className="voice-text-button" disabled={voice.busy} onClick={()=>void voice.clear()} title={t("voice.removeTitle")}>{t("voice.remove")}</button></div>}
    {voice.error && !startFailed && !failed && !noAudio && !voice.conflicted && <div className="voice-error" role="alert">{voice.error}</div>}
  </>;
}
