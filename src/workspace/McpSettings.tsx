import { useEffect, useRef, useState } from "react";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";
import { Icon } from "../ui";
import {useTranslation} from "react-i18next";
import {message} from "../i18n/messages";
import {useNotice} from "../i18n/react";

type McpState = { enabled: boolean; executable_available: boolean; configuration: string | null; setup_prompt: string | null };
type Diagnostic = { server_version: string; protocol_version: string; tools: string[]; enabled: boolean; scope: "local_stdio_only" };
type CopyKind = "prompt" | "configuration";
type Work = CopyKind | "switch" | "diagnose";

export default function McpSettings({ onBusyChange }: { onBusyChange?: (busy: boolean) => void } = {}) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<McpState | null>(null);
  const [busy, setBusy] = useState<Work | null>(null);
  const [readError, setReadError] = useNotice();
  const [switchError, setSwitchError] = useNotice();
  const [diagnosticError, setDiagnosticError] = useNotice();
  const [notice, setNotice] = useNotice();
  const [diagnostic, setDiagnostic] = useState<Diagnostic | null>(null);
  const [copyResult, setCopyResult] = useState<{ kind: CopyKind; status: "copied" | "failed" } | null>(null);
  const promptPreview = useRef<HTMLDetailsElement>(null);
  const promptText = useRef<HTMLTextAreaElement>(null);
  const configurationText = useRef<HTMLTextAreaElement>(null);
  const locked = useRef(false), reads = useRef(0);

  useEffect(() => { setCopyResult(null); }, [state?.enabled, state?.setup_prompt, state?.configuration]);

  useEffect(() => {
    let active = true;
    const refresh = () => {
      if (locked.current) return;
      const request = ++reads.current;
      void call<McpState>("mcp_settings").then(value => {
        if (active && request === reads.current) { setState(value); setReadError(""); }
      }).catch(e => {
        if (active && request === reads.current) { setReadError(errorText(e)); }
      });
    };
    if (native) { refresh(); window.addEventListener("focus", refresh); }
    return () => { active = false; reads.current++; window.removeEventListener("focus", refresh); };
  }, []);

  async function run(action: Work, work: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; reads.current++;
    setBusy(action); onBusyChange?.(true);
    try { await work(); }
    finally { locked.current = false; setBusy(null); onBusyChange?.(false); }
  }

  async function updateEnabled(enabled: boolean) {
    setSwitchError(""); setNotice("");
    setDiagnostic(null);
    setDiagnosticError("");
    let failure: unknown;
    try { await call("mcp_set_enabled", { enabled }); } catch (e) { failure = e; }
    // A write may reach disk before a later fsync/IPC error. Always reconcile
    // with the real switch, including when the mutation reports failure.
    try {
      const current = await call<McpState>("mcp_settings");
      setState(current); setReadError("");
      if (failure !== undefined) setSwitchError(errorText(failure));
      else setNotice(message("settings", current.enabled ? "mcp.enabledNotice" : "mcp.disabledNotice"));
    } catch (e) {
      setState(null);
      setSwitchError(errorText(e));
    }
  }

  async function diagnose() {
    setDiagnostic(null); setDiagnosticError("");
    try { setDiagnostic(await call<Diagnostic>("mcp_diagnose")); }
    catch (e) { setDiagnosticError(errorText(e)); }
  }

  async function copy(kind: CopyKind) {
    setCopyResult(null);
    try {
      await call("mcp_copy_setup", { kind });
      setCopyResult({ kind, status: "copied" });
    } catch {
      setCopyResult({ kind, status: "failed" });
    }
  }

  function selectText(kind: CopyKind) {
    if (kind === "prompt" && promptPreview.current) promptPreview.current.open = true;
    const textarea = (kind === "prompt" ? promptText : configurationText).current;
    textarea?.focus();
    textarea?.select();
  }

  function copyControls(kind: CopyKind, disabled: boolean) {
    const result = copyResult?.kind === kind ? copyResult.status : null;
    const label = kind === "prompt" ? t("mcp.copySetupPrompt") : t("mcp.copyConfiguration");
    return <>
      <div className="action-row mcp-actions">
        <button className={`mcp-copy-button ${kind === "prompt" ? "send-button" : "outline-button"}`} disabled={!!busy || disabled}
          onClick={() => void run(kind, () => copy(kind))}>
          {result === "copied" && <Icon name="check" size={13} />}
          {busy === kind ? t("mcp.copying") : result === "copied" ? t("mcp.copied") : label}
        </button>
        {result === "failed" && <button className="outline-button" onClick={() => selectText(kind)}>{t("mcp.selectText")}</button>}
      </div>
      <ErrorNotice text={result === "failed" ? t("mcp.copyFailed") : ""} />
      <div role="status">{result === "copied" && <p className="settings-result">
        {t(kind === "prompt" ? "mcp.setupPromptCopied" : "mcp.configurationCopied")}
      </p>}</div>
    </>;
  }

  return <section className="settings-section mcp-settings">
    <h3>{t('mcp.title')}</h3>
    <p>{t('mcp.description')}</p>
    <p className="field-help" id="mcp-access-help">{t('mcp.privacyDescription')}</p>
    <div className="setting-line">
      <div><strong>{t('mcp.allowAccess')}</strong>{native && !state && <p>{readError || switchError ? t('mcp.statusUnknown') : t('status.reading')}</p>}</div>
      <input type="checkbox" role="switch" className="settings-switch" aria-label={t('mcp.allowAccess')}
        aria-describedby="mcp-access-help" checked={state?.enabled ?? false} disabled={!state || !!busy}
        onChange={event => { const enabled = event.target.checked; void run("switch", () => updateEnabled(enabled)); }}/>
    </div>
    <ErrorNotice text={switchError || readError} />
    {notice && <p className="settings-result" role="status">{notice}</p>}
    {!native && <p>{t('preview.mcpReadOnly')}</p>}
    {native && state && !state.executable_available && <p>{t('mcp.executableMissing')}</p>}
    <p id="mcp-setup-instructions">{t(state?.enabled ? 'mcp.setupInstructions' : 'mcp.enableToCopy')}</p>
    {copyControls("prompt", !state?.enabled || !state.setup_prompt)}
    {state?.setup_prompt && <details className="mcp-details" ref={promptPreview}>
      <summary>{t('mcp.viewSetupPrompt')}</summary>
      <textarea ref={promptText} className="mcp-installation-prompt" aria-label={t('mcp.setupPromptLabel')}
        aria-describedby="mcp-setup-instructions" readOnly value={state.setup_prompt} rows={9} />
    </details>}
    {state?.configuration && <details className="mcp-details"><summary>{t('mcp.connectionSummary')}</summary>
      <p>{t('mcp.connectionInstructions')}</p>
      {copyControls("configuration", false)}
      <textarea ref={configurationText} className="mcp-configuration" aria-label={t('mcp.configurationLabel')}
        readOnly value={state.configuration} rows={10} />
    </details>}
    <p className="field-help">{t('mcp.afterExit')}</p>
    <div className="action-row mcp-actions">
      <button className="outline-button" disabled={!!busy || !state?.executable_available} onClick={() => void run("diagnose", diagnose)}>
        {busy === "diagnose" ? t('mcp.checkingLocal') : t('mcp.checkLocal')}
      </button>
    </div>
    {diagnostic && <div className="settings-result" role="status">
      {t('mcp.diagnosticPassed',{count:diagnostic.tools.length})}
      <p>{diagnostic.enabled ? t('mcp.diagnosticEnabled') : t('mcp.diagnosticDisabled')}</p>
    </div>}
    <ErrorNotice text={diagnosticError} />
    <p className="field-help">{t('mcp.diagnosticScope')}</p>
  </section>;
}
