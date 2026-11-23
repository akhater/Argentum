/**
 * The inherited strings, worded differently. Ours.
 *
 * WHY NOT EDIT THE LOCALE FILES
 *
 * There are fifteen of them, so a changed word is a line in each. The word is
 * replaced in i18next as each language loads (English at start, the others
 * when picked, languages.ts): the inherited key, our value. Every place that
 * reads the key shows ours.
 *
 * If a key here is ever renamed in the locale files, the override lands on a
 * key nobody reads and the original wording comes back. That is the right
 * failure: an old label beats a broken screen.
 *
 * SUBJECT IS OBJECT
 *
 * The inherited Subject mask selects whatever you draw a box round. Lightroom calls
 * that Select Object; its Select Subject is one click and finds the main thing
 * by itself, which is what our Foreground does. A photographer coming from
 * Lightroom reads "Subject" as the wrong tool, so it says Object, in each
 * language in Lightroom's own word for it.
 *
 * Only the label changes. The type stays `ai-subject`, so every saved mask
 * still loads, and a mask already named "Subject" keeps that name: the editor
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
 * RELIGHT AND FOG, SAID OUR WAY
 *
 * RapidRAW 1.6.6's Relight and Fog in the words of the 2026-10-20 placement
 * note (Edit > Effects): a light is a Lamp, a Spot or a Sun, the light already
 * in the photo is "Existing light", and their "Spatial" group, which only the
 * Customise menu still shows, says what it holds. English only: in the other
 * languages their translation of their word stays.
 */
const RELIGHT_FOG_EN: Record<string, string> = {
  'adjustments.effects.relightTypes.point': 'Lamp',
  'adjustments.effects.relightTypes.directional': 'Sun',
  'adjustments.effects.relightAmbient': 'Existing light',
  'adjustments.effects.relightDepth': 'Distance',
  'adjustments.effects.relightFalloff': 'Reach',
  'adjustments.effects.relightAngle': 'Direction',
  'adjustments.effects.relightElevation': 'Height',
  'adjustments.effects.relightCone': 'Beam width',
  'adjustments.effects.relightShadows': 'Cast shadows',
  'adjustments.effects.relightShadowSoftness': 'Shadow blur',
  'adjustments.effects.relightType': 'Type',
  'adjustments.effects.relightColor': 'Colour',
  'adjustments.effects.relightAddLight': 'Add light',
  'adjustments.effects.relightDuplicateLight': 'Duplicate light',
  'adjustments.effects.relightRemoveLight': 'Remove light',
  'adjustments.effects.fogStart': 'Starts at',
  'adjustments.effects.fogHeight': 'Ground fog',
  'adjustments.effects.spatial': 'Lens blur, Relight and Fog',
  'editor.ai.generatingNormalMap': 'Working out which way each surface faces…',
};

/**
 * THE MODERN WINDOW'S NAMES
 *
 * Their shortcuts name the panels of the window they had: D was the
 * Adjustments panel, K the AI panel, P Presets. In the Modern window the same
 * keys open the right panel's tabs (modern/RightPanel.tsx), so Settings >
 * Controls says the tab, and Reset Layout says what it still resets there
 * (useUIStore's resetWorkspaceLayout: the column widths, and the columns and
 * filmstrip shown). English only, as above.
 */
const MODERN_WINDOW_EN: Record<string, string> = {
  'settings.keybinds.actions.toggle_adjustments': 'Edit tab',
  'settings.keybinds.actions.toggle_crop_panel': 'Crop tab',
  'settings.keybinds.actions.toggle_presets': 'Looks tab',
  'settings.keybinds.actions.toggle_masks': 'Masks tab',
  'settings.keybinds.actions.toggle_ai': 'Retouch tab',
  'settings.keybinds.actions.toggle_export': 'Export tab',
  'settings.keybinds.actions.toggle_metadata': 'Photo info',
  'settings.data.resetLayoutDesc':
    'Put the left and right columns back to their default widths, and show the columns and filmstrip again if they were hidden.',
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

/** Put our wording over theirs, in every language loaded so far. Called again after each load. */
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

  for (const [key, text] of Object.entries({ ...RELIGHT_FOG_EN, ...MODERN_WINDOW_EN })) {
    i18n.addResource('en', 'translation', key, text);
  }
}
