import { isTauri, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { initializeI18n } from './index';
import { resolveLanguage, type Language, type LanguagePreference } from './languages';

export type LanguageSnapshot = {
  preference: LanguagePreference;
  language: Language;
  revision: number;
  error?: 'preferences_unavailable' | 'native_menu_unavailable' | null;
};
let current: LanguageSnapshot = { preference: 'system', language: 'en', revision: -1 };
const subscribers = new Set<() => void>();
let started: Promise<void> | undefined;
let subscription: Promise<unknown> | undefined;
let presentation: Promise<void> = Promise.resolve();
export const getLanguageSnapshot = () => current;
export const subscribeLanguage = (subscriber: () => void) => {
  subscribers.add(subscriber);
  return () => { subscribers.delete(subscriber); };
};

async function accept(snapshot: LanguageSnapshot) {
  if (snapshot.revision < current.revision) return;
  current = snapshot;
  // Serialize async i18next changes so the last accepted snapshot always wins.
  presentation = presentation.catch(() => {}).then(async () => {
    await initializeI18n(current.language);
    for (const subscriber of subscribers) subscriber();
  });
  await presentation;
}

export async function refreshLanguage(): Promise<void> {
  if (!isTauri()) return;
  try {
    subscription ??= listen<LanguageSnapshot>('ui-language-changed', event => { void accept(event.payload); })
      .catch(error => { subscription = undefined; throw error; });
    await subscription;
    await accept(await invoke<LanguageSnapshot>('ui_language_snapshot'));
  } catch {
    // No write on read failure; preserve any valid snapshot already seen.
    await accept({ ...current, error: 'preferences_unavailable' });
  }
}

export function startLanguage(): Promise<void> {
  return started ??= (async () => {
    if (!isTauri()) {
      await accept({ preference: 'system', language: resolveLanguage(navigator.languages), revision: 0 });
      return;
    }
    // The subscription must exist before the initial IPC read can race with a change.
    await refreshLanguage();
  })();
}

export async function setLanguagePreference(preference: LanguagePreference): Promise<void> {
  if (isTauri()) {
    await accept(await invoke<LanguageSnapshot>('ui_language_set', { preference }));
  } else {
    await accept({ preference, language: preference === 'system' ? resolveLanguage(navigator.languages) : preference, revision: current.revision + 1 });
  }
}
