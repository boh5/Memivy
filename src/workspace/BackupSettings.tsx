import { useEffect, useRef, useState } from "react";
import { call, errorText } from "./api";
import { ErrorNotice } from "./components";
import {useTranslation} from "react-i18next";
import {message} from "../i18n/messages";
import {useNotice} from "../i18n/react";

type Prepared = { id: string; memories: number; captures: number };
export default function BackupSettings({ disabled, onBusyChange, onRestore }: {
  disabled: boolean; onBusyChange: (value: boolean) => void; onRestore: (id: string) => void;
}) {
  const {t}=useTranslation('settings');
  const [busy, setBusy] = useState(false), [error, setError] = useNotice(), [notice, setNotice] = useNotice();
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
    <h3>{t('backup.title')}</h3>
    <p>{t('backup.description')}</p>
    <ErrorNotice text={error} />
    {notice && <p className="workspace-notice" role="status">{notice}</p>}
    <div className="setting-line"><div><strong>{t('backup.createTitle')}</strong></div>
      <button className="outline-button" disabled={disabled || busy || !!prepared} onClick={() => void run(async () => {
        const path = await call<string | null>("backup_create");
        if (path) setNotice(message('settings','backup.saved',{path}));
      })}>{t('backup.createAction')}</button></div>
    <div className="setting-line"><div><strong>{t('backup.restoreTitle')}</strong><p>{t('backup.restoreDescription')}</p></div>
      <button className="outline-button" disabled={disabled || busy || !!prepared} onClick={() => void run(async () => {
        const result = await call<Prepared | null>("backup_prepare");
        staged.current = result?.id || null; setPrepared(result);
      })}>{t('backup.chooseAction')}</button></div>
    {busy && <p role="status">{t('backup.processing')}</p>}
    {prepared && <div className="workspace-warning">
      <p>{t('backup.verifiedSummary',{memories:prepared.memories,captures:prepared.captures})}</p>
      <p>{t('backup.settingsRetained')}</p>
      <div className="action-row">
        <button className="outline-button" disabled={busy || disabled} onClick={() => void run(async () => {
          await call("backup_discard", { id: prepared.id }); staged.current = null; setPrepared(null);
        })}>{t('actions.cancel')}</button>
        <button className="send-button" disabled={busy || disabled} onClick={() => {
          if (lock.current) return;
          lock.current = true; restoring.current = true; onRestore(prepared.id);
        }}>{t('backup.confirmRestore')}</button>
      </div>
    </div>}
  </section>;
}
