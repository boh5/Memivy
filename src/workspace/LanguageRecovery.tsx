import { useState, useSyncExternalStore } from 'react';
import { useTranslation } from 'react-i18next';
import { getLanguageSnapshot, subscribeLanguage, refreshLanguage } from '../i18n/preferences';

export default function LanguageRecovery() {
  const { t } = useTranslation('common');
  const snapshot = useSyncExternalStore(subscribeLanguage, getLanguageSnapshot);
  const [busy, setBusy] = useState(false);
  if (!snapshot.error) return null;
  return <div className="preview-banner" role="alert">
    {t(snapshot.error === 'native_menu_unavailable' ? 'languageMenuError' : 'languageReadError')}{' '}
    <button className="quiet" disabled={busy} onClick={() => {
      setBusy(true); void refreshLanguage().finally(() => setBusy(false));
    }}>{t('retry')}</button>
  </div>;
}
