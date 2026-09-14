import { useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { getLanguageSnapshot, subscribeLanguage, setLanguagePreference, refreshLanguage } from '../i18n/preferences';
import type { LanguagePreference } from '../i18n/languages';

export default function LanguageSettings({onBusyChange}:{onBusyChange?:(busy:boolean)=>void}={}) {
  const { t } = useTranslation('common');
  const snapshot = useSyncExternalStore(subscribeLanguage, getLanguageSnapshot);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  async function change(preference: LanguagePreference) {
    setBusy(true); onBusyChange?.(true); setFailed(false);
    try { await setLanguagePreference(preference); }
    catch { setFailed(true); }
    finally { setBusy(false); onBusyChange?.(false); }
  }
  return <section className="language-settings">
    <div className="language-setting-row">
    <div className="language-setting-copy"><h3 id="ui-language-label">{t('language')}</h3></div>
    <div className="language-segments" role="radiogroup" aria-labelledby="ui-language-label" aria-busy={busy}>
      {([{value:'system',label:t('system')},{value:'zh-CN',label:t('languageChinese')},{value:'en',label:t('languageEnglish')}] as const).map(option => <label key={option.value} className="language-option">
        <input type="radio" name="ui-language" value={option.value} checked={snapshot.preference === option.value} disabled={busy} onChange={() => void change(option.value)} />
        <span>{option.label}</span>
      </label>)}
    </div>
    </div>
    <div className="language-feedback">
      {(failed || snapshot.error) && <p role="alert">{t(failed ? 'languageSaveError' : snapshot.error === 'native_menu_unavailable' ? 'languageMenuError' : 'languageReadError')}</p>}
      {snapshot.error && <button className="model-text-button" disabled={busy} onClick={() => { setBusy(true); void refreshLanguage().finally(() => setBusy(false)); }}>{t('retry')}</button>}
    </div>
  </section>;
}
