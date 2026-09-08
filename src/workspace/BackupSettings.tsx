import { useEffect, useRef, useState } from "react";
import { call, errorText } from "./api";
import { ErrorNotice } from "./components";

type Prepared = { id: string; memories: number; captures: number };
export default function BackupSettings({ disabled, onBusyChange, onRestore }: {
  disabled: boolean; onBusyChange: (value: boolean) => void; onRestore: (id: string) => void;
}) {
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const [prepared, setPrepared] = useState<Prepared | null>(null);
  const staged = useRef<string | null>(null), restoring = useRef(false), lock = useRef(false);
  useEffect(() => () => {
    if (staged.current && !restoring.current) void call("backup_discard", { id: staged.current }).catch(() => {});
  }, []);
  async function run(work: () => Promise<void>) {
    if (lock.current || disabled) return;
    lock.current = true; setBusy(true); onBusyChange(true); setError(""); setNotice("");
    try { await work(); } catch (e) { setError(errorText(e)); }
    finally { lock.current = false; setBusy(false); onBusyChange(false); }
  }
  return <section className="settings-section">
    <h3>备份与恢复</h3>
    <p>备份包含记忆、原话、版本、讨论、草稿和回收站，不包含模型密钥。</p>
    <ErrorNotice text={error} />
    {notice && <p className="workspace-notice" role="status">{notice}</p>}
    <div className="setting-line"><div><strong>创建备份</strong><p>将本地内容保存成一份备份文件。</p></div>
      <button className="outline-button" disabled={disabled || busy || !!prepared} onClick={() => void run(async () => {
        const path = await call<string | null>("backup_create");
        if (path) setNotice(`备份已保存：${path}`);
      })}>创建备份</button></div>
    <div className="setting-line"><div><strong>从备份恢复</strong><p>恢复整个记忆库，并自动保留恢复前的副本。</p></div>
      <button className="outline-button" disabled={disabled || busy || !!prepared} onClick={() => void run(async () => {
        const result = await call<Prepared | null>("backup_prepare");
        staged.current = result?.id || null; setPrepared(result);
      })}>选择备份</button></div>
    {busy && <p role="status">正在处理备份，请稍候…</p>}
    {prepared && <div className="workspace-warning">
      <p>备份已校验，包含 {prepared.memories} 条记忆和 {prepared.captures} 条原话。确认后将重启并整库恢复，当前内容会先保存为一份可恢复的副本。</p>
      <p>模型配置和 MCP 开关保留当前设置。</p>
      <div className="action-row">
        <button className="outline-button" disabled={busy || disabled} onClick={() => void run(async () => {
          await call("backup_discard", { id: prepared.id }); staged.current = null; setPrepared(null);
        })}>取消</button>
        <button className="send-button" disabled={busy || disabled} onClick={() => {
          if (lock.current) return;
          lock.current = true; restoring.current = true; onRestore(prepared.id);
        }}>确认恢复并重启</button>
      </div>
    </div>}
  </section>;
}
