/**
 * Argentum's own translations. Ours.
 *
 * WHY A SEPARATE NAMESPACE
 *
 * i18next only reads the files it is given, and the inherited interface gives
 * it thirteen — one per language. Argentum's strings get their own namespace,
 * `ag`, registered here at startup, so a new string is one line in
 * `locales/en.json` next door rather than thirteen edited files. Use it as
 * `t('ag:wbMenuTooltip')`.
 *
 * WHAT STAYS IN THE INHERITED FILES
 *
 * The inherited strings: the interface's own sentences, with "RapidRAW"
 * reading "Argentum" and `.rrdata` reading `.agdata`.
 *
 * TRANSLATIONS
 *
 * English only for now. i18next falls back to English for any language without
 * a file, so a missing translation shows English rather than a blank label or a
 * raw key. Add `<lang>.json` beside this file and list it below.
 */

import i18n from 'i18next';
import { useTranslation } from 'react-i18next';

import en from './en.json';
import { applyRenames } from './renames';

export const AG_NS = 'ag';

/** One of Argentum's own strings. */
export type AgKey = keyof typeof en;

/**
 * Add our bundle, whenever i18next is ready for it.
 *
 * Order matters and cannot be assumed. i18next only grows `addResourceBundle`
 * once `init` has run, and module import order decides whether that has
 * happened — registering at import time threw
 * `i18n.addResourceBundle is not a function` and took the whole UI down with
 * it, because a module-level throw kills the render, not just the feature.
 *
 * So: register now if it is ready, and otherwise once it says it is. Called
 * more than once is harmless; i18next replaces the bundle.
 */
export function registerArgentumTranslations() {
  const add = () => {
    if (typeof i18n.addResourceBundle === 'function') {
      i18n.addResourceBundle('en', AG_NS, en, true, true);
    }
    // Some of their words, said our way. See renames.ts.
    applyRenames();
  };

  if (i18n.isInitialized) {
    add();
  } else {
    i18n.on('initialized', add);
  }
}

/**
 * Translations for Argentum's own strings.
 *
 * `useTranslation()` from react-i18next is typed against *their* key list, so
 * one of our keys fails to compile there. This is the same lookup, typed
 * against `en.json` next door instead — which means a typo in one of our keys
 * is a compile error rather than a label that silently renders the key.
 */
export function useAgTranslation() {
  // The namespace is ours, so their generated union does not contain it.
  const { t } = useTranslation(AG_NS as never);
  return (key: AgKey): string => t(key as never) as string;
}

/** The same lookup outside a component, for code that runs on an event. */
export const agText = (key: AgKey): string => i18n.t(key as never, { ns: AG_NS }) as string;
