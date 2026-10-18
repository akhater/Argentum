// The shell's strings: its own i18next instance and its own locale files, so
// nothing is added to the app's. Tool and setting labels are i18n keys from the
// descriptors; a key with no translation is shown as words made from the key,
// so a new tool is readable before anyone translates it.
//
// Keys are flat ("tools.x.mode": ...). The contract names a choice
// `tools.<id>.<key>.<value>` under its setting's own `tools.<id>.<key>`, and
// nested JSON cannot hold a string and an object at the same path.

import { createInstance } from 'i18next';

import en from './locales/en.json';

const i18n = createInstance();
void i18n.init({
  resources: { en: { translation: en } },
  lng: typeof navigator === 'undefined' ? 'en' : navigator.language,
  fallbackLng: 'en',
  keySeparator: false,
  initAsync: false,
  interpolation: { escapeValue: false },
});

/** `tools.rm_grain.grainSize` -> "Grain size"; `tools.rm_grain.name` -> "Grain". */
export function humanize(key: string): string {
  const parts = key.split('.');
  let word = parts[parts.length - 1];
  if ((word === 'name' || word === 'label') && parts.length > 1) word = parts[parts.length - 2];
  word = word.replace(/^(dt|rt|rr|rm|ag)_/, '');
  const spaced = word
    .replace(/_/g, ' ')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .toLowerCase();
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/** The text for a key, with `{{name}}` values filled in. */
export function text(key: string, values?: Record<string, string | number>): string {
  // The app's i18next types know only the app's keys; these are the shell's.
  return i18n.exists(key) ? String(i18n.t(key as never, values as never)) : humanize(key);
}
