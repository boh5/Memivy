import { useEffect, useRef, useState } from "react";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";
import { shortcutLabel, useDesktop } from "./desktopApi";

const loginLabels: Record<string, string> = { enabled: "已开启", disabled: "未开启", requires_approval: "等待系统允许", not_found: "当前应用包不可用" };
export default function DesktopSettings() {
  const desktop = useDesktop(), state = desktop.state;
  const [recording, setRecording] = useState(false), [candidate, setCandidate] = useState(""), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [loginStatus, setLoginStatus] = useState<string | null>(native ? null : "disabled"), [loginError, setLoginError] = useState("");
  const loginRequest = useRef(0), changingLogin = useRef(false);
  useEffect(() => {
    if (!native) return;
    const refreshLogin = () => {
      if (changingLogin.current) return;
      const request = ++loginRequest.current;
      void call<string>("desktop_login_status").then(status => {
        if (request === loginRequest.current) { setLoginStatus(status); setLoginError(""); }
      }).catch(e => {
        if (request === loginRequest.current) { setLoginStatus(null); setLoginError(errorText(e)); }
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
  return <section className="settings-section desktop-settings">
    <h3>macOS 快捷入口</h3>
    <p>随时记一下，或接着上次的话题。关闭主窗口后，菜单栏仍可使用；⌘Q 退出应用。</p>
    {state && <>
      <div className="setting-line"><div><strong>全局快捷键</strong><p>按一下打开，再按一下收起。首次设置后，请切到其他应用试按一次。</p></div>
        <button className={`shortcut-recorder outline-button ${recording ? "recording" : ""}`} disabled={busy || !native} onClick={() => setRecording(true)}
          onBlur={() => setRecording(false)} onKeyDown={e => {
            if (!recording) return;
            e.preventDefault(); e.stopPropagation();
            if (e.key === "Escape") { setRecording(false); return; }
            if (e.repeat || ["Meta", "Shift", "Control", "Alt"].includes(e.key) || e.nativeEvent.isComposing) return;
            const text = [...(e.ctrlKey ? ["Control"] : []), ...(e.altKey ? ["Alt"] : []), ...(e.shiftKey ? ["Shift"] : []), ...(e.metaKey ? ["Super"] : []), e.code].join("+");
            setCandidate(text); setRecording(false);
          }}>{recording ? "请按快捷键…" : shortcutLabel(candidate || state.shortcut)}</button>
      </div>
      {candidate && <div className="action-row"><button className="send-button" disabled={busy} onClick={() => void run(async () => { await desktop.update({ shortcut: candidate }); setCandidate(""); })}>保存快捷键</button><button className="outline-button" onClick={() => setCandidate("")}>取消修改</button></div>}
      <label className="checkbox-label"><input type="checkbox" checked={state.visible} disabled={busy || !native} onChange={e => void run(() => desktop.update({ visible: e.target.checked }))} />显示桌面助手 <small>可以拖到顺手的位置</small></label>
      <label className="checkbox-label"><input type="checkbox" checked={state.paused} disabled={busy || !native} onChange={e => void run(() => desktop.update({ paused: e.target.checked }))} />暂停快捷入口 <small>暂时停用快捷键和桌面助手</small></label>
      <div className="setting-line"><div><strong>登录时启动</strong><p>安静启动，保留菜单栏入口。当前：{loginStatus ? loginLabels[loginStatus] || "状态未知" : loginError ? "状态未知" : "读取中…"}。</p></div>
        <button className="outline-button" disabled={busy || !native || !loginStatus} onClick={() => void run(() => updateLogin(!(loginStatus === "enabled" || loginStatus === "requires_approval")))}>{loginStatus === "enabled" || loginStatus === "requires_approval" ? "关闭" : "开启"}</button>
      </div>
      {loginStatus === "requires_approval" && <button className="connect-model-link" onClick={() => void run(() => updateLogin(null))}>在系统登录项中允许 Memivy</button>}
      {loginStatus === "not_found" && <p className="field-help">请将正式的 Memivy 应用放入“应用程序”目录，再开启登录启动。</p>}
      <p className="field-help">只在唤起时读取前台应用名称。网页链接或文件路径由你主动附带；原话保存不依赖模型。</p>
      <ErrorNotice text={error || loginError || desktop.error || state.error || ""} />
    </>}
  </section>;
}
