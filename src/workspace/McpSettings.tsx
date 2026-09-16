import { useEffect, useRef, useState } from "react";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";
import {useTranslation} from "react-i18next";
import {message} from "../i18n/messages";
import {useNotice} from "../i18n/react";
import {buildMcpSetupPrompt} from "./mcpSetupPrompt";

type McpState = { enabled: boolean; executable_available: boolean; configuration: string | null };
type Diagnostic = { server_version: string; protocol_version: string; tools: string[]; enabled: boolean; scope: "local_stdio_only" };
export default function McpSettings({ onBusyChange }: { onBusyChange?: (busy: boolean) => void } = {}) {
  const {t}=useTranslation('settings');
  const [state, setState] = useState<McpState | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useNotice(), [notice, setNotice] = useNotice();
  const [diagnostic, setDiagnostic] = useState<Diagnostic | null>(null);
  const setupPrompt = state?.configuration ? buildMcpSetupPrompt(state.configuration) : "";
  const locked = useRef(false), reads = useRef(0);
  useEffect(() => {
    let active = true;
    const refresh = () => {
      if (locked.current) return;
      const request = ++reads.current;
      void call<McpState>("mcp_settings").then(value => {
        if (active && request === reads.current) { setState(value); setError(""); }
      }).catch(e => {
        if (active && request === reads.current) { setError(errorText(e)); }
      });
    };
    if (native) { refresh(); window.addEventListener("focus", refresh); }
    else setState({ enabled: true, executable_available: false, configuration: null });
    return () => { active = false; reads.current++; window.removeEventListener("focus", refresh); };
  }, []);
  async function run(work: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; reads.current++;
    setBusy(true); onBusyChange?.(true); setError(""); setNotice("");
    try { await work(); }
    catch (e) { setError(errorText(e)); }
    finally { locked.current = false; setBusy(false); onBusyChange?.(false); }
  }
  async function updateEnabled(enabled: boolean) {
    setDiagnostic(null);
    let failure: unknown;
    try { await call("mcp_set_enabled", { enabled }); } catch (e) { failure = e; }
    // A write may reach disk before a later fsync/IPC error. Always reconcile
    // with the real switch, including when the mutation reports failure.
    const current = await call<McpState>("mcp_settings").catch(e => { setState(null); throw e; });
    setState(current);
    if (failure !== undefined) throw failure;
    setNotice(message("settings", current.enabled ? "mcp.enabledNotice" : "mcp.disabledNotice"));
  }
  return <section className="settings-section desktop-settings">
    <h3>{t('mcp.title')}</h3>
    <p>{t('mcp.description')}</p>
    <p className="field-help">{t('mcp.privacyDescription')}</p>
    <div className="setting-line">
      <div><strong>{t('mcp.allowAccess')}</strong>{!state&&<p>{error?t('mcp.statusUnknown'):t('status.reading')}</p>}</div>
      <input type="checkbox" role="switch" className="settings-switch" aria-label={t('mcp.allowAccess')} checked={state?.enabled??false} disabled={!native||!state||busy}
        onChange={event=>{const enabled=event.target.checked;void run(()=>updateEnabled(enabled));}}/>
    </div>
    {!native && <p>{t('preview.mcpReadOnly')}</p>}
    {native && state && !state.executable_available && <p>{t('mcp.executableMissing')}</p>}
    <p id="mcp-setup-instructions">{t('mcp.setupInstructions')}</p>
    {setupPrompt && <textarea className="mcp-installation-prompt" aria-label={t('mcp.setupPromptLabel')} aria-describedby="mcp-setup-instructions" readOnly value={setupPrompt} rows={9} />}
    <div className="action-row mcp-actions">
      <button className="outline-button" disabled={busy || !state?.enabled || !setupPrompt} onClick={() => void run(async () => {
        await navigator.clipboard.writeText(setupPrompt);
        setNotice(message("settings", "mcp.setupPromptCopied"));
      })}>{t('mcp.copySetupPrompt')}</button>
      <button className="outline-button" disabled={busy || !native || !state?.executable_available} onClick={() => void run(async () => {
        setDiagnostic(null);
        const report = await call<Diagnostic>("mcp_diagnose");
        setDiagnostic(report);
      })}>{busy ? t('status.processing') : t('mcp.checkLocal')}</button>
    </div>
    {state?.configuration && <details className="mcp-details"><summary>{t('mcp.connectionSummary')}</summary>
      <p>{t('mcp.connectionInstructions')}</p>
      <textarea className="mcp-configuration" aria-label={t('mcp.configurationLabel')} readOnly value={state.configuration} rows={10} />
      <p>{t('mcp.afterExit')}</p>
      <p>{t('mcp.tryConnection')}</p>
    </details>}
    {diagnostic && <div className="settings-result" role="status">
      {t('mcp.diagnosticPassed',{count:diagnostic.tools.length})}
      <p>{t('mcp.diagnosticScope')}{' '}{diagnostic.enabled ? t('mcp.diagnosticEnabled') : t('mcp.diagnosticDisabled')}</p>
    </div>}
    {notice && <p className="settings-result" role="status">{notice}</p>}
    <ErrorNotice text={error} />
  </section>;
}
