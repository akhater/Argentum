/**
 * A provider look in a photo's edit. Ours.
 *
 * A LUT made for one photo, kept beside it (mods/provider_look.rs). `lutPath`
 * holds a marker rather than a path, and `agProviderLook` the camera, the look,
 * its options and the rendering the look replaced. The marker is the switch: a
 * look counts only while `lutPath` is exactly the marker, so anything that
 * writes a real `lutPath` turns it off.
 *
 * An edit holds only the options away from their default, so a look with all of
 * them at their default names the file it always did, and an edit saved before
 * options existed reads the same as one with every option at its default. What
 * an option means is the provider's: here it is an id, a label and a value.
 *
 * One option can have a look follow the photo's white balance. `ownWb` then names
 * that option and the white balance the look's file was made for, none for the
 * camera's. It says which file is on the photo, not where the sliders are now.
 *
 * A look goes on with Linear (its LUT is made for that), so whatever takes it
 * off also gives the photo its own rendering back, from `before`, which belongs
 * to that photo: it never travels to another.
 */

import type { Adjustments } from '../../utils/adjustments';
import type { OfferedOption, OwnWb, ProviderLook, RenderingBefore } from '../ag.generated';

/** As `MARKER` in mods/provider_look.rs. */
export const PROVIDER_LOOK_PATH = 'provider-look';

/** As `KEY` in mods/provider_look.rs. */
export const PROVIDER_LOOK_KEY = 'agProviderLook';

type Edit = Partial<Adjustments>;

/** A look's options away from their default, by id. */
export type LookOptions = NonNullable<ProviderLook['options']>;

/** The rendering a look goes on with; the renderer fills in the photo's curve (mods/raw_tone.rs). */
const LINEAR: RenderingBefore = { rawToneRendering: 'linear', rawToneCurve: null, rawToneCurveMode: null };

/** The edit's provider look, or null. */
export const lookOf = (edit: Edit): ProviderLook | null =>
  edit.lutPath === PROVIDER_LOOK_PATH ? (edit.agProviderLook ?? null) : null;

/** The steps of a look's file: `GRID` in mods/lut_provider.rs. */
const LOOK_STEPS = 64;

/** Whether `option` is on, with `values` chosen. */
export const optionIsOn = (option: OfferedOption, values: LookOptions): boolean => values[option.id] ?? option.default;

/** `values` with `option` at `on`. */
export function withOption(values: LookOptions, option: OfferedOption, on: boolean): LookOptions {
  const others = Object.entries(values).filter(([id]) => id !== option.id);
  return Object.fromEntries(on === option.default ? others : [...others, [option.id, on]]);
}

/** The ones of `values` still among `offered`. */
export const offeredOptions = (offered: OfferedOption[], values: LookOptions = {}): LookOptions =>
  Object.fromEntries(Object.entries(values).filter(([id]) => offered.some((option) => option.id === id)));

export function sameOptions(a: LookOptions = {}, b: LookOptions = {}): boolean {
  const ids = Object.keys(a);
  return ids.length === Object.keys(b).length && ids.every((id) => a[id] === b[id]);
}

/** What a provider look writes on a photo, through `lookOnto`, which fits it to the photo. */
export const lookKeys = (
  camera: string,
  look: string,
  name: string,
  options: LookOptions = {},
  ownWb?: OwnWb | null,
): Edit => ({
  lutPath: PROVIDER_LOOK_PATH,
  lutName: name,
  lutSize: LOOK_STEPS,
  lutIntensity: 100,
  lutIsSceneReferred: false,
  agLutDeclaredInput: true,
  agProviderLook: {
    camera,
    look,
    ...(Object.keys(options).length > 0 && { options }),
    ...(ownWb && { ownWb }),
    before: LINEAR,
  },
});

/** The keys of an edit that decide the white it renders to: `target_white_balance` in image_processing.rs. */
const WHITE_BALANCE_KEYS = ['temperature', 'tint', 'whiteBalance', 'sectionVisibility'];

export const whiteBalanceKeys = (edit: Edit): Edit =>
  Object.fromEntries(Object.entries(edit).filter(([key]) => WHITE_BALANCE_KEYS.includes(key)));

/** What makes a look's file for the white balance it was made for, wherever the sliders are. */
export const madeForWhiteBalance = (ownWb?: OwnWb | null): Edit => ({
  temperature: 0,
  tint: 0,
  whiteBalance: ownWb?.wb ?? null,
  sectionVisibility: null,
});

/**
 * The edit a render shows for a look on hover. From a preview, `lutPath` is that
 * temporary file: this edit is only ever shown, never saved, so no photo holds such a path.
 */
export const hoverEdit = (photo: Adjustments, keys: Edit, preview?: string): Adjustments => ({
  ...photo,
  ...lookOnto(photo, keys, true),
  ...(preview && { lutPath: preview }),
});

/** Every key a provider look writes on a photo. */
export const LOOK_KEYS = [...Object.keys(lookKeys('', '', '')), ...Object.keys(LINEAR), 'lutData'];

const renderingOf = (edit: Edit): RenderingBefore => ({
  rawToneRendering: edit.rawToneRendering ?? null,
  rawToneCurve: edit.rawToneCurve ?? null,
  rawToneCurveMode: edit.rawToneCurveMode ?? null,
});

/** The keys that take a photo's provider look off; none for a photo without one. */
export const providerLookOff = (photo: Edit): Edit => {
  const on = lookOf(photo);
  return on ? { ...on.before, agProviderLook: null } : {};
};

/** Whether two edits have the same provider look on, or both none. */
export const sameProviderLook = (a: Edit, b: Edit): boolean => {
  const [one, other] = [lookOf(a), lookOf(b)];
  return one?.camera === other?.camera && one?.look === other?.look && sameOptions(one?.options, other?.options);
};

/**
 * What `changes`, made elsewhere (a preset, a LUT), write on `photo`. A provider
 * look they bring keeps this photo's rendering as its `before`, the first one
 * if a look is already on. The white balance it was made for is this photo's only
 * if it was made for this photo (`made`): from elsewhere, like `before`, it stays
 * behind. Any other LUT in them takes a look on the photo off.
 */
export function lookOnto(photo: Edit, changes: Edit, made = false): Edit {
  const incoming = lookOf(changes);
  if (incoming) {
    const before = lookOf(photo)?.before ?? renderingOf(photo);
    const ownWb = incoming.ownWb && { ...incoming.ownWb, wb: made ? incoming.ownWb.wb : null };
    return { ...changes, ...LINEAR, agProviderLook: { ...incoming, ...(ownWb && { ownWb }), before } };
  }
  return changes.lutPath === undefined ? changes : { ...providerLookOff(photo), ...changes };
}

/** `edit` with its look's file now the one for `ownWb`; null if that look is off it, or has that file already. */
export function withOwnWb(edit: Edit, camera: string, look: string, ownWb: OwnWb | null): Edit | null {
  const on = lookOf(edit);
  if (on?.camera !== camera || on.look !== look) return null;
  if (JSON.stringify(on.ownWb ?? null) === JSON.stringify(ownWb)) return null;
  return { ...edit, agProviderLook: { ...on, ...(ownWb && { ownWb }) } };
}
