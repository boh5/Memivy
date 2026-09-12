export const supportedLanguages = ['en', 'zh-CN'] as const;
export type Language = typeof supportedLanguages[number];
export type LanguagePreference = 'system' | Language;

/** Ordered system preferences: an unsupported script must not hide a later match. */
export function resolveLanguage(preferences: readonly string[]): Language {
  for (const preference of preferences) {
    const parts = preference.toLowerCase().replaceAll('_', '-').split('-');
    if (parts[0] === 'en') return 'en';
    if (parts[0] !== 'zh') continue;
    if (parts.includes('hant')) continue;
    if (parts.includes('hans')) return 'zh-CN';
    if (parts.includes('tw') || parts.includes('hk') || parts.includes('mo')) continue;
    if (parts.length === 1 || parts.includes('cn') || parts.includes('sg')) return 'zh-CN';
  }
  return 'en';
}
