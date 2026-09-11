import { Icon } from "../ui";
import type { useVoice } from "./useVoice";
import "./voice.css";
type Voice = ReturnType<typeof useVoice>;
export function VoiceButton({voice,disabled}:{voice:Voice;disabled:boolean}) {
  const recording = voice.active && !!(voice.session?.recording || voice.session?.starting);
  const processing = voice.active && !recording;
  const label = processing ? "正在转写" : recording ? "停止录音并转写" : "语音输入";
  return <button type="button" className={`voice-button ${recording?"is-recording":""} ${processing?"is-processing":""}`} disabled={disabled||voice.busy||processing}
    onClick={()=>void voice.toggle()} aria-label={label} title={voice.active?label:"语音输入 · 本机识别"} aria-pressed={recording}>
    {recording ? <span className="voice-stop" aria-hidden="true"/> : processing ? <span className="voice-spinner" aria-hidden="true"/> : <Icon name="mic" size={17}/>}
  </button>;
}
export function VoiceFeedback({voice}:{voice:Voice}) {
  const v=voice.session;
  return <>
    {v&&<div className={`voice-feedback ${voice.active?"is-active":""}`} role="status">
      <div className="voice-wave" aria-hidden="true">{[.45,.8,1,.65,.95,.55,.8].map((n,i)=><i key={i} style={{height:`${4+Math.min(1,v.level*18)*19*n}px`}}/>)}</div>
      <span className="voice-state">{v.error?"录音已保留":v.starting?"正在打开麦克风…":v.recording?"正在听":v.processing?"正在转写…":"转写完成，可编辑"}</span>
      <span className="voice-time">{Math.floor(v.seconds/60)}:{String(Math.floor(v.seconds%60)).padStart(2,"0")}</span>
      {v.recording&&voice.status?.state==="loading"&&<small>模型加载中，声音已接收</small>}
      {!voice.active&&<button type="button" className="voice-text-button" onClick={()=>void voice.clear()} title="移除暂存录音，保留输入框文字">移除录音</button>}
      {v.error&&!voice.active&&<button type="button" className="voice-text-button" onClick={()=>void voice.retry()}>重试转写</button>}
    </div>}
    {voice.error.includes("草稿已在其他位置修改")&&v?.complete&&<div className="voice-recovery"><p>{v.text}</p><button className="outline-button" onClick={voice.recover}>追加到当前草稿</button></div>}
    {(voice.error||v?.error)&&<div className="voice-error" role="alert">{voice.error||v?.error}</div>}
  </>;
}
