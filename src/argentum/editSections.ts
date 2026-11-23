/**
 * What each section of the Edit panel holds, as the panel arranges them. Ours.
 *
 * Their sections (utils/adjustments.ts `ADJUSTMENT_SECTIONS`) keep white
 * balance, Vibrance and Saturation in Color, and Clarity, Dehaze, Structure and
 * Centre in Details. The panel shows them in Basic instead, the corrections
 * most photos need, in the order they are made, so a section's reset, copy and
 * paste follow them there; its eye does too, in the renderer's section checks
 * (image_processing.rs). Profile holds how the RAW is read, before any slider:
 * the camera profile, the RAW rendering and the tone mapper.
 *
 * Derived from their lists, not written out again, so a key they add to a
 * section still arrives in it.
 */

import { ADJUSTMENT_SECTIONS, INITIAL_ADJUSTMENTS, type Adjustments } from '../utils/adjustments';
import AG_DEFAULT_ADJUSTMENTS from './defaultAdjustments.json';

/**
 * Which sections' eyes are open: `sectionVisibility` in an edit, read by the
 * renderer's section checks. RapidRAW 1.6.6 replaced it with an eye per tool
 * (`activeTools`), which Argentum does not use, so the type lives here.
 */
export interface SectionVisibility {
  [index: string]: boolean;
  basic: boolean;
  curves: boolean;
  color: boolean;
  details: boolean;
  effects: boolean;
}

/**
 * Argentum's starting values on top of RapidRAW's (`INITIAL_ADJUSTMENTS`, from
 * their defaultAdjustments.json): the camera profile, the RAW rendering and the
 * section eyes. One file, also read by the renderer (image_processing.rs
 * `is_default_adjustments`). An edit that lacks one of these keys means its
 * value here: the renderer reads a missing rendering as DNG Standard
 * (mods/raw_tone.rs `mode_of`) and a missing eye as open.
 */
export { AG_DEFAULT_ADJUSTMENTS };

/** Every section eye open, for an edit or a mask that does not say. */
export const SECTION_EYES_OPEN: SectionVisibility = AG_DEFAULT_ADJUSTMENTS.sectionVisibility;

/** The RAW rendering an edit asks for, DNG Standard when it does not say. */
export const rawToneRenderingOf = (adjustments: Record<string, any> | null | undefined): string =>
  adjustments?.rawToneRendering ?? AG_DEFAULT_ADJUSTMENTS.rawToneRendering;

/** Moved into Basic from Color and Details. `whiteBalanceAuto` is ours (WhiteBalanceMenu.tsx). */
const INTO_BASIC = [
  'temperature',
  'tint',
  'whiteBalance',
  'whiteBalanceAuto',
  'vibrance',
  'saturation',
  'clarity',
  'dehaze',
  'structure',
  'centré',
  'centréX',
  'centréY',
];

/** The Profile section, ahead of the others. It has no eye: none of it can be turned off. */
export const PROFILE_KEYS = ['cameraProfile', 'rawToneRendering', 'rawToneCurve', 'rawToneCurveMode', 'toneMapper'];

/** The manual fringe sliders, in Crop > Lens now (CropPanel.tsx): reset there with the lens. */
const FRINGE_KEYS = ['chromaticAberrationRedCyan', 'chromaticAberrationBlueYellow'];

/** The LUT, a look now (argentum/looks): no section's reset, copy or eye touches it. */
const LUT_KEYS = [
  'lutData',
  'lutIntensity',
  'lutName',
  'lutPath',
  'lutSize',
  'lutIsSceneReferred',
  'agLutDeclaredInput',
  'agProviderLook',
];

/**
 * Maps worked out from one photo's pixels (depth for Lens blur and Fog, surface
 * directions for Relight): another photo needs its own, so no section copies
 * them, as Copy settings never carries `rawToneCurve`.
 */
export const PER_PHOTO_MAPS = ['lensBlurDepthMap', 'relightNormalMap', 'fogDepthMap'];

const without = (keys: string[], gone: string[]) => keys.filter((key) => !gone.includes(key));

export const EDIT_SECTION_KEYS: Record<string, string[]> = {
  basic: [...without(ADJUSTMENT_SECTIONS.basic, PROFILE_KEYS), ...INTO_BASIC],
  curves: ADJUSTMENT_SECTIONS.curves,
  // agBlackWhite is ours (BlackWhite.tsx): the B&W switch and its mix, which takes the Color Mixer's place.
  color: [...without(ADJUSTMENT_SECTIONS.color, INTO_BASIC), 'agBlackWhite'],
  // agSharpen is ours (Sharpening.tsx): Detail's reset, copy and paste take it with their sharpening.
  details: [...without(ADJUSTMENT_SECTIONS.details, [...INTO_BASIC, ...FRINGE_KEYS]), 'agSharpen'],
  effects: without(ADJUSTMENT_SECTIONS.effects, [...LUT_KEYS, ...PER_PHOTO_MAPS]),
};

/** Tools that left their section, for Basic, Crop or the Looks tab; the Customise menu does not list them. */
export const MOVED_TOOLS = ['whiteBalance', 'colorPresence', 'presence', 'lut', 'chromaticAberration'];

/** What `keys` start as on a new photo; a key with no starting value of theirs is cleared. */
export function initialValues(keys: string[]): Partial<Adjustments> {
  const values: Record<string, unknown> = {};
  for (const key of keys) {
    const initial =
      key in AG_DEFAULT_ADJUSTMENTS
        ? AG_DEFAULT_ADJUSTMENTS[key as keyof typeof AG_DEFAULT_ADJUSTMENTS]
        : INITIAL_ADJUSTMENTS[key as keyof Adjustments];
    values[key] = initial === undefined ? undefined : structuredClone(initial);
  }
  return values as Partial<Adjustments>;
}
