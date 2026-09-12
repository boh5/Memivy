import { createInstance, type ParseKeys } from 'i18next';
import { initReactI18next } from 'react-i18next';
import { supportedLanguages, type Language } from './languages';

// Vite eagerly embeds all catalogs. No network, language downloads or detector cache.
const catalogs = import.meta.glob<Record<string, unknown>>('../../locales/*/*.json', {
  eager: true, import: 'default',
});
const resources: Record<string, Record<string, Record<string, unknown>>> = {};
for (const [path, catalog] of Object.entries(catalogs)) {
  const match = path.match(/locales\/([^/]+)\/([^/]+)\.json$/);
  if (match) (resources[match[1]] ??= {})[match[2]] = catalog;
}

// Modules share this instance inside one WebView, including independent editor roots.
const i18n = createInstance();
export default i18n;
type Namespace = 'common' | 'workspace' | 'settings' | 'editor' | 'errors' | 'native';
const namespaces: readonly Namespace[] = ['common', 'workspace', 'settings', 'editor', 'errors', 'native'];
/** The one runtime boundary for persisted codes and semantic message descriptors. */
export function translateCatalog(key: string, options: Record<string, unknown> = {}): string {
  const ns = namespaces.find(namespace => namespace === options.ns) ?? 'common';
  if (!i18n.exists(key, { ...options, ns })) return i18n.t('operation_failed', { ns: 'errors' });
  return String(i18n.t(key as ParseKeys<Namespace>, { ...options, ns }));
}
let initialization: Promise<unknown> | undefined;
export async function initializeI18n(language: Language): Promise<void> {
  initialization ??= i18n.use(initReactI18next).init({
    resources, lng: language, supportedLngs: [...supportedLanguages], fallbackLng: 'en',
    load: 'currentOnly', defaultNS: 'common', returnEmptyString: false, saveMissing: false,
    interpolation: { escapeValue: false }, react: { useSuspense: false },
  });
  await initialization;
  if (i18n.language !== language) await i18n.changeLanguage(language);
  document.documentElement.lang = language;
}
