import { useEffect, useRef, useState, type ReactNode } from "react";
import ShortcutSetting from "./ShortcutSetting";
import {listen} from "@tauri-apps/api/event";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";
import { useDesktop } from "./desktopApi";
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
export default function DesktopSettings({focusEntry=false,children,onBusyChange}:{focusEntry?:boolean;children?:ReactNode;onBusyChange?:(busy:boolean)=>void}={}) {
  const {t}=useTranslation('settings');
  const entry = useRef<HTMLElement | null>(null);
  useEffect(() => { if (focusEntry) entry.current?.scrollIntoView({block:'start'}); }, [focusEntry]);
  const desktop = useDesktop(), state = desktop.state;
  const [busy, setBusy] = useState(false), [error, setError] = useNotice();
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
    let alive = true;
    const subscription = listen<string>("desktop-login-changed", event => {
      if (!alive) return;
      loginRequest.current++;
      setLoginStatus(event.payload); setLoginError("");
    });
    void subscription.then(() => { if (alive) refreshLogin(); }).catch(e => { if (alive) setLoginError(errorText(e)); });
    window.addEventListener("focus", refreshLogin);
    return () => { alive = false; loginRequest.current++; window.removeEventListener("focus", refreshLogin); void subscription.then(stop => stop()).catch(() => {}); };
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
    setBusy(true); onBusyChange?.(true); setError("");
    try { await work(); await desktop.refresh(); }
    catch (e) { setError(errorText(e)); }
    finally { setBusy(false); onBusyChange?.(false); }
  }
  const loginLabel=(status:string|null)=>status?t((loginLabels as Record<string,LoginLabelKey>)[status]||'desktop.login.unknown'):loginError?t('desktop.login.unknown'):t('desktop.login.reading');
  const translatedDesktopStatusError = desktop.error || (state?.error ? translateCatalog(state.error, {ns: "errors"}) : "");
  return <>
    <section className="settings-section startup-settings">
      <h3>{t('desktop.startupTitle')}</h3>
      <div className="setting-line"><div><strong>{t('desktop.login.title')}</strong>{loginStatus !== 'enabled' && loginStatus !== 'disabled' && <p>{loginLabel(loginStatus)}</p>}</div>
        <button className="model-switch" role="switch" aria-label={t("desktop.login.title")} aria-checked={loginStatus === "enabled" || loginStatus === "requires_approval"} disabled={busy || !native || !loginStatus} onClick={() => void run(() => updateLogin(!(loginStatus === "enabled" || loginStatus === "requires_approval")))}><i /></button>
      </div>
      {loginStatus === "requires_approval" && <button className="connect-model-link" onClick={() => void run(() => updateLogin(null))}>{t('desktop.login.allowInSystem')}</button>}
      {loginStatus === "not_found" && <p className="field-help">{t('desktop.login.notFoundHelp')}</p>}
      <ErrorNotice text={loginError}/>
    </section>
    <section ref={entry} className="settings-section desktop-settings">
    <h3>{t('desktop.title')}</h3>
    {state && <>
      <div className="setting-line"><strong>{t('desktop.showIcon')}</strong><input type="checkbox" role="switch" className="settings-switch" aria-label={t('desktop.showIcon')} checked={state.visible} disabled={busy || !native} onChange={e => void run(() => desktop.update({visible:e.target.checked}))}/></div>
      <ShortcutSetting label={t('desktop.shortcutTitle')} value={state.shortcut} disabled={busy||!native} onChange={shortcut=>void run(()=>desktop.update({shortcut}))}/>
      <ErrorNotice text={error || translatedDesktopStatusError} />
    </>}
    {children}
    <details className="settings-help"><summary>{t('desktop.usage')}</summary>
      <p>{t('desktop.shortcutDescription')}</p>
      <p>{t('voice.shortcutDescription')}</p>
      <p>{t('desktop.foregroundPrivacy')}</p>
    </details>
  </section></>;
}
