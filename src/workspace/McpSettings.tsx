import { useEffect, useRef, useState } from "react";
import { call, errorText, native } from "./api";
import { ErrorNotice } from "./components";

type McpState = { enabled: boolean; executable_available: boolean; configuration: string | null };
type Diagnostic = { server_version: string; protocol_version: string; tools: string[]; enabled: boolean; scope: "local_stdio_only" };
export default function McpSettings({ onBusyChange }: { onBusyChange?: (busy: boolean) => void } = {}) {
  const [state, setState] = useState<McpState | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const [diagnostic, setDiagnostic] = useState<Diagnostic | null>(null);
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
    else setState({ enabled: false, executable_available: false, configuration: null });
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
    setNotice(current.enabled ? "已允许 MCP 读写，请继续在外部 Agent 配置连接。" : "MCP 已关闭，新的工具调用会被拒绝。");
  }
  return <section className="settings-section desktop-settings">
    <h3>外部 Agent · MCP</h3>
    <p>允许外部 Agent 保存你明确要求记住的原话、检索少量已保存的记忆。返回的内容可能由该 Agent 发送给它使用的模型。</p>
    <label className="checkbox-label">
      <input type="checkbox" checked={state?.enabled ?? false} disabled={!native || !state || busy}
        onChange={event => { const enabled = event.target.checked; void run(() => updateEnabled(enabled)); }} />
      <span>允许 MCP 访问记忆 <small>{state ? (state.enabled ? "已开启" : "已关闭") : error ? "状态无法确认" : "读取中…"}</small></span>
    </label>
    {!native && <p>浏览器仅供预览；请在 macOS 应用中设置 MCP。</p>}
    {native && state && !state.executable_available && <p>未找到 MCP 程序，请重新安装完整应用包；开发环境先运行 npm run build:mcp。</p>}
    <div className="setting-line">
      <button className="outline-button" disabled={busy || !state?.configuration} onClick={() => void run(async () => {
        await navigator.clipboard.writeText(state!.configuration!);
        setNotice("配置已复制。按外部 Agent 的 MCP 配置说明填入 command、args 和 env。");
      })}>复制 MCP 配置</button>
      <button className="outline-button" disabled={busy || !native || !state?.executable_available} onClick={() => void run(async () => {
        setDiagnostic(null);
        const report = await call<Diagnostic>("mcp_diagnose");
        setDiagnostic(report);
      })}>{busy ? "处理中…" : "检查本地 MCP"}</button>
    </div>
    {state?.configuration && <details className="mcp-details"><summary>查看配置与连接说明</summary>
      <p>先将 Memivy 移入固定安装位置，再复制配置。外部 Agent 需要支持本机 stdio；按其说明添加服务并重新连接。应用移动后需重新复制配置。</p>
      <textarea className="mcp-configuration" aria-label="MCP 配置" readOnly value={state.configuration} rows={10} />
      <p>退出 Memivy 后仍可保存和检索；自动整理会在下次打开应用且模型可用时继续。关闭这里的开关即可停止新的 MCP 读写。</p>
      <p>连接后可尝试：“请记住：这是我的 MCP 测试记录”，再检索这句话。只有实际调用成功，才能确认外部 Agent 已接通。</p>
    </details>}
    {diagnostic && <div className="settings-result" role="status">
      本地 stdio 检查通过 · v{diagnostic.server_version} · 协议 {diagnostic.protocol_version} · 2 个工具。
      <p>这只验证本机程序与工具清单，不代表外部 Agent 已连接。{diagnostic.enabled ? "当前已允许读写。" : "当前开关关闭，数据调用仍会被拒绝。"}</p>
    </div>}
    {notice && <p className="settings-result" role="status">{notice}</p>}
    <ErrorNotice text={error} />
  </section>;
}
