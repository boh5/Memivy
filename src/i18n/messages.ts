/** Presentation state retains semantic messages, never a frozen translation. */
import type { ParseKeys } from 'i18next';
type MessageNamespace = 'common' | 'workspace' | 'settings' | 'editor' | 'errors' | 'native';
export type MessageValues = Readonly<Record<string, string | number | boolean | null | UiMessage>>;
export type UiMessage = string | Readonly<{ ns: string; key: string; values?: MessageValues }>;

export function message<N extends MessageNamespace>(ns: N, key: ParseKeys<N>, values?: MessageValues): UiMessage {
  return { ns, key: String(key), values };
}

export function renderMessage(
  value: UiMessage,
  translate: (key: string, options: Record<string, unknown>) => string,
): string {
  if (typeof value === 'string') return value;
  const values = Object.fromEntries(Object.entries(value.values ?? {}).map(([key, parameter]) => [
    key, parameter !== null && typeof parameter === 'object'
      ? renderMessage(parameter, translate) : parameter,
  ]));
  return translate(value.key, { ...values, ns: value.ns });
}
