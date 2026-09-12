import i18n from './index';

const dates = new Map<string, Intl.DateTimeFormat>();
const numbers = new Map<string, Intl.NumberFormat>();
export function formatNumber(value: number, maximumFractionDigits = 0) {
  const locale = i18n.resolvedLanguage || 'en';
  const key = `${locale}:${maximumFractionDigits}`;
  let formatter = numbers.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale, { maximumFractionDigits });
    numbers.set(key, formatter);
  }
  return formatter.format(value);
}
function dateFormatter(full: boolean) {
  const locale = i18n.resolvedLanguage || 'en';
  const key = `${locale}:${full}`;
  let formatter = dates.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, full
      ? { year: 'numeric', month: 'numeric', day: 'numeric', hour: 'numeric', minute: 'numeric', second: 'numeric', hour12: false }
      : { month: 'long', day: 'numeric' });
    dates.set(key, formatter);
  }
  return formatter;
}
export const formatDate = (timestamp: number) => dateFormatter(false).format(timestamp);
export const formatFullDate = (timestamp: number) => dateFormatter(true).format(timestamp);
