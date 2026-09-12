import { useEffect, useRef, useState, type ReactNode } from "react";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";
import { shortcutLabel, useDesktop } from "./desktopApi";
import {useTranslation} from "react-i18next";
import {useNotice} from "../i18n/react";
import {translateCatalog} from "../i18n";

type LoginLabelKey = "desktop.login.enabled" | "desktop.login.disabled" | "desktop.login.requiresApproval" | "desktop.login.notFound";
const loginLabels = {
  enabled: "desktop.login.enabled",
  disabled: "desktop.login.disabled",
  requires_approval: "desktop.login.requiresApproval",
  not_found: "desktop.login.notFound",
} as const satisfies Record<string, LoginLabelKey>;
export default function DesktopSettings({focusEntry=false,children}:{focusEntry?:boolean;children?:ReactNode}={}) {
  const {t}=useTranslation('settings');
  const entry = useRef<HTMLElement | null>(null);
  useEffect(() => { if (focusEntry) entry.current?.scrollIntoView({block:'start'}); }, [focusEntry]);
  const desktop = useDesktop(), state = desktop.state;
  const [recording, setRecording] = useState(false), [candidate, setCandidate] = useState(""), [busy, setBusy] = useState(false), [error, setError] = useNotice();
  const [loginStatus, setLoginStatus] = useState<string | null>(native ? null : "disabled"), [loginError, setLoginError] = useNotice();
  const loginRequest = useRef(0), changingLogin = useRef(false);
  useEffect(() => {
    if (!native) return;
    const refreshLogin = () => {
      if (changingLogin.current) return;
      const request = ++loginRequest.current;
      void call<string>("desktop_login_status").then(status => {
        if (request === loginRequest.current) { setLoginStatus(status); setLoginError(""); }
      }).catch(e => {
        if (request === loginRequest.current) { setLoginError(errorText(e)); }
      });
    };
    refreshLogin();
    window.addEventListener("focus", refreshLogin);
    return () => { loginRequest.current++; window.removeEventListener("focus", refreshLogin); };
  }, []);
  async function updateLogin(enabled: boolean | null) {
    changingLogin.current = true;
    const request = ++loginRequest.current;
    try {
      const status = await call<string>("desktop_login", { enabled });
      if (request === loginRequest.current) { setLoginStatus(status); setLoginError(""); }
    } finally { changingLogin.current = false; }
  }
  async function run(work: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true); setError("");
    try { await work(); await desktop.refresh(); }
    catch (e) { setError(errorText(e)); }
    finally { setBusy(false); }
  }
  const loginLabel=(status:string|null)=>status?t((loginLabels as Record<string,LoginLabelKey>)[status]||'desktop.login.unknown'):loginError?t('desktop.login.unknown'):t('desktop.login.reading');
  const translatedDesktopStatusError = desktop.error || (state?.error ? translateCatalog(state.error, {ns: "errors"}) : "");
  return <>
    <section className="settings-section startup-settings">
      <h3>{t('desktop.startupTitle')}</h3>
      <div className="setting-line"><div><strong>{t('desktop.login.title')}</strong><p>{t('desktop.login.description',{status:loginLabel(loginStatus)})}</p></div>
        <button className="outline-button" disabled={busy || !native || !loginStatus} onClick={() => void run(() => updateLogin(!(loginStatus === "enabled" || loginStatus === "requires_approval")))}>{loginStatus === "enabled" || loginStatus === "requires_approval" ? t('actions.disable') : t('actions.enable')}</button>
      </div>
      {loginStatus === "requires_approval" && <button className="connect-model-link" onClick={() => void run(() => updateLogin(null))}>{t('desktop.login.allowInSystem')}</button>}
      {loginStatus === "not_found" && <p className="field-help">{t('desktop.login.notFoundHelp')}</p>}
      <ErrorNotice text={loginError}/>
    </section>
    <section ref={entry} className="settings-section desktop-settings">
    <h3>{t('desktop.title')}</h3>
    <p>{t('desktop.description')}</p>
    {state && <>
      <div className="setting-line"><div><strong>{t('desktop.shortcutTitle')}</strong><p>{t('desktop.shortcutDescription')}</p></div>
        <button className={`shortcut-recorder outline-button ${recording ? "recording" : ""}`} disabled={busy || !native} onClick={() => setRecording(true)}
          onBlur={() => setRecording(false)} onKeyDown={e => {
            if (!recording) return;
            e.preventDefault(); e.stopPropagation();
            if (e.key === "Escape") { setRecording(false); return; }
            if (e.repeat || ["Meta", "Shift", "Control", "Alt"].includes(e.key) || e.nativeEvent.isComposing) return;
            const text = [...(e.ctrlKey ? ["Control"] : []), ...(e.altKey ? ["Alt"] : []), ...(e.shiftKey ? ["Shift"] : []), ...(e.metaKey ? ["Super"] : []), e.code].join("+");
            setCandidate(text); setRecording(false);
          }}>{recording ? t('desktop.pressShortcut') : shortcutLabel(candidate || state.shortcut)}</button>
      </div>
      {candidate && <div className="action-row"><button className="send-button" disabled={busy} onClick={() => void run(async () => { await desktop.update({ shortcut: candidate }); setCandidate(""); })}>{t('desktop.saveShortcut')}</button><button className="outline-button" onClick={() => setCandidate("")}>{t('actions.cancelEdit')}</button></div>}
      <label className="checkbox-label"><input type="checkbox" checked={state.visible} disabled={busy || !native} onChange={e => void run(() => desktop.update({ visible: e.target.checked }))} />{t('desktop.showAssistant')} <small>{t('desktop.showAssistantHint')}</small></label>
      <label className="checkbox-label"><input type="checkbox" checked={state.paused} disabled={busy || !native} onChange={e => void run(() => desktop.update({ paused: e.target.checked }))} />{t('desktop.pauseEntry')} <small>{t('desktop.pauseEntryHint')}</small></label>
      <p className="field-help">{t('desktop.foregroundPrivacy')}</p>
      <ErrorNotice text={error || translatedDesktopStatusError} />
    </>}
    {children}
  </section></>;
}
