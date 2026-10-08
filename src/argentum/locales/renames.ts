/**
 * Their strings, worded differently. Ours.
 *
 * WHY NOT EDIT THEIR LOCALE FILES
 *
 * Because there are fifteen of them and upstream edits them every release. A
 * changed word there is a line in each, in the files most likely to conflict,
 * and a rename is exactly the change that should cost nothing. So the word is
 * replaced in i18next at startup: their key, our value. Their files keep
 * saying what they said, and every place that reads the key shows ours.
 *
 * If a key here is ever renamed upstream, the override lands on a key nobody
 * reads and their wording comes back. That is the right failure: an old label
 * beats a merge conflict.
 *
 * SUBJECT IS OBJECT
 *
 * Their Subject mask selects whatever you draw a box round. Lightroom calls
 * that Select Object; its Select Subject is one click and finds the main thing
 * by itself, which is what our Foreground does. A photographer coming from
 * Lightroom reads "Subject" as the wrong tool, so it says Object, in each
 * language in Lightroom's own word for it.
 *
 * Only the label changes. The type stays `ai-subject`, so every saved mask
 * still loads, and a mask already named "Subject" keeps that name: their code
 * stores the label as the mask's name when it is created, and that is the
 * user's to change.
 */

import i18n from 'i18next';

/** Their key for the Subject mask's label. */
const SUBJECT = 'masks.types.subject';

/** Their Built-in AI card lists the masks by name, Subject among them. */
const AI_FEATURES = 'settings.processing.ai.cpu.feature1';

/** Lightroom's word for Select Object, per language they ship. */
const OBJECT: Record<string, string> = {
  en: 'Object',
  ca: 'Objecte',
  cs: 'Objekt',
  de: 'Objekt',
  es: 'Objeto',
  fr: 'Objet',
  it: 'Oggetto',
  ja: 'オブジェクト',
  ko: '개체',
  nl: 'Object',
  pl: 'Obiekt',
  pt: 'Objeto',
  ru: 'Объект',
  'zh-CN': '对象',
  'zh-TW': '物件',
};

/**
 * Swap their word for ours inside a sentence of theirs, keeping its case.
 *
 * The card's list is "Subject, Sky, Foreground" in English and
 * "onderwerp, lucht, voorgrond" in Dutch, so the match ignores case and the
 * replacement follows whatever the sentence did. A sentence that does not
 * contain their word is left exactly as it is.
 */
function swapWord(sentence: string, from: string, to: string): string {
  const at = sentence.toLocaleLowerCase().indexOf(from.toLocaleLowerCase());
  if (at < 0) return sentence;
  const found = sentence.slice(at, at + from.length);
  const lower = found === found.toLocaleLowerCase() && found !== found.toLocaleUpperCase();
  return sentence.slice(0, at) + (lower ? to.toLocaleLowerCase() : to) + sentence.slice(at + from.length);
}

/** The string at a dotted key in one of their bundles, if there is one. */
function lookup(bundle: unknown, key: string): string | undefined {
  let node = bundle;
  for (const part of key.split('.')) {
    if (typeof node !== 'object' || node === null) return undefined;
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === 'string' ? node : undefined;
}

/** Put our wording over theirs. Called once i18next has their bundles. */
export function applyRenames() {
  if (typeof i18n.addResource !== 'function') return;

  // Running this twice is harmless: the second pass finds our word where
  // theirs was and swaps it for itself.
  for (const [lng, word] of Object.entries(OBJECT)) {
    const bundle: unknown = i18n.getResourceBundle(lng, 'translation');
    const theirs = lookup(bundle, SUBJECT);
    if (theirs === undefined) continue;

    i18n.addResource(lng, 'translation', SUBJECT, word);

    const features = lookup(bundle, AI_FEATURES);
    if (features !== undefined) {
      i18n.addResource(lng, 'translation', AI_FEATURES, swapWord(features, theirs, word));
    }
  }
}
