/**
 * The name of each step of a photo's edit history, for the History tab.
 *
 * Moved here from RapidRAW's src/components/panel/editor/EditorToolbar.tsx
 * (RapidRAW 1.6.6 @ 52138689), where it named the steps of the menu under the
 * toolbar's undo and redo buttons. The naming is RapidRAW's, unchanged
 * (stepName).
 *
 * Ours: a name is kept with the state it names, not its place in the list, so
 * a History put back from a sidecar (history/historyKeeper.ts) keeps the names
 * it was saved with rather than showing the last photo's.
 */

import { useMemo } from 'react';
import type { Adjustments } from '../../utils/adjustments';

const formatKey = (k: string) => {
  const special: Record<string, string> = {
    aiPatches: 'AI Patches',
    aspectRatio: 'Aspect Ratio',
    flipHorizontal: 'Flip Horizontal',
    flipVertical: 'Flip Vertical',
    orientationSteps: 'Rotation',
    lutPath: 'LUT',
    lutIntensity: 'LUT Intensity',
    lutData: 'LUT Data',
    lutName: 'LUT Name',
    lutSize: 'LUT Size',
    chromaticAberrationBlueYellow: 'Chromatic Aberration Blue/Yellow',
    chromaticAberrationRedCyan: 'Chromatic Aberration Red/Cyan',
    centré: 'Centré',
    lumaNoiseReduction: 'Luma Noise Reduction',
    colorNoiseReduction: 'Color Noise Reduction',
    lensMaker: 'Lens Maker',
    lensModel: 'Lens Model',
    lensDistortionAmount: 'Lens Distortion',
    lensVignetteAmount: 'Lens Vignette',
    lensTcaAmount: 'Lens TCA',
    lensDistortionEnabled: 'Enable Lens Distortion',
    lensTcaEnabled: 'Enable Lens TCA',
    lensVignetteEnabled: 'Enable Lens Vignette',
    transformDistortion: 'Transform Distortion',
    transformVertical: 'Transform Vertical',
    transformHorizontal: 'Transform Horizontal',
    transformRotate: 'Transform Rotate',
    transformAspect: 'Transform Aspect',
    transformScale: 'Transform Scale',
    transformXOffset: 'Transform X Offset',
    transformYOffset: 'Transform Y Offset',
    colorGrading: 'Color Grading',
    colorCalibration: 'Color Calibration',
    toneMapper: 'Tone Mapper',
    showClipping: 'Show Clipping',
    sectionVisibility: 'Section Visibility',
    flareAmount: 'Flare Amount',
    glowAmount: 'Glow Amount',
    halationAmount: 'Halation Amount',
    grainAmount: 'Grain Amount',
    grainRoughness: 'Grain Roughness',
    grainSize: 'Grain Size',
    vignetteAmount: 'Post-Crop Vignette Amount',
    vignetteFeather: 'Post-Crop Vignette Feather',
    vignetteMidpoint: 'Post-Crop Vignette Midpoint',
    vignetteRoundness: 'Post-Crop Vignette Roundness',
    dehaze: 'Dehaze',
    exposure: 'Exposure',
    blacks: 'Blacks',
    whites: 'Whites',
    shadows: 'Shadows',
    highlights: 'Highlights',
    contrast: 'Contrast',
    brightness: 'Brightness',
    clarity: 'Clarity',
    structure: 'Structure',
    sharpness: 'Sharpness',
    saturation: 'Saturation',
    temperature: 'Temperature',
    tint: 'Tint',
    vibrance: 'Vibrance',
    hsl: 'HSL',
    curves: 'Curves',
    crop: 'Crop',
    masks: 'Masks',
    rating: 'Rating',
  };
  if (special[k]) return special[k];
  return k.replace(/([A-Z])/g, ' $1').replace(/^./, (str) => str.toUpperCase());
};

/** What the step from `prev` to `curr` changed, as their toolbar named it. */
export function stepName(prev: Adjustments, curr: Adjustments): string {
  const changed: string[] = [];

  for (const key of Object.keys(curr)) {
    if (prev[key] === curr[key]) continue;

    if (key === 'masks') {
      const prevMasks = prev.masks || [];
      const currMasks = curr.masks || [];

      if (currMasks.length > prevMasks.length) changed.push('Added Mask');
      else if (currMasks.length < prevMasks.length) changed.push('Deleted Mask');
      else {
        currMasks.forEach((cMask: any) => {
          const pMask = prevMasks.find((m: any) => m.id === cMask.id);
          if (pMask) {
            if (pMask.opacity !== cMask.opacity) changed.push('Mask Opacity');
            if (pMask.invert !== cMask.invert) changed.push('Mask Invert');
            if (pMask.visible !== cMask.visible) changed.push('Mask Visibility');
            if (pMask.subMasks !== cMask.subMasks) changed.push('Mask Area / Brush');

            if (pMask.adjustments !== cMask.adjustments) {
              for (const adjKey of Object.keys(cMask.adjustments || {})) {
                if (pMask.adjustments[adjKey] !== cMask.adjustments[adjKey]) {
                  changed.push(`Mask ${formatKey(adjKey)}`);
                }
              }
            }
          }
        });
      }
    } else if (key === 'aiPatches') {
      const prevPatches = prev.aiPatches || [];
      const currPatches = curr.aiPatches || [];

      if (currPatches.length > prevPatches.length) changed.push('Added AI Patch');
      else if (currPatches.length < prevPatches.length) changed.push('Deleted AI Patch');
      else {
        currPatches.forEach((cPatch: any) => {
          const pPatch = prevPatches.find((p: any) => p.id === cPatch.id);
          if (pPatch) {
            if (pPatch.visible !== cPatch.visible) changed.push('AI Patch Visibility');
            if (pPatch.subMasks !== cPatch.subMasks) changed.push('AI Patch Area');
            if (pPatch.patchData !== cPatch.patchData || pPatch.prompt !== cPatch.prompt) {
              changed.push('AI Generation');
            }
          }
        });
      }
    } else {
      changed.push(formatKey(key));
    }
  }

  const uniqueChanged = Array.from(new Set(changed));

  if (uniqueChanged.length === 0) return 'Adjustment';
  if (uniqueChanged.length > 2) return `${uniqueChanged.slice(0, 2).join(', ')}...`;
  return uniqueChanged.join(', ');
}

/** Each state's name, once known. */
const names = new WeakMap<Adjustments, string>();

/** Name these states as they were named when saved. */
export function keepNames(states: Adjustments[], stateNames: string[]) {
  states.forEach((state, i) => names.set(state, stateNames[i]));
}

/** The name of each step of `history`. */
export function historyNames(history: Adjustments[]): string[] {
  return history.map((curr, i) => {
    if (i === 0) return 'Initial State';
    let name = names.get(curr);
    if (name === undefined) {
      name = stepName(history[i - 1], curr);
      names.set(curr, name);
    }
    return name;
  });
}

export function useHistoryNames(adjustmentsHistory: Adjustments[]): string[] {
  return useMemo(() => historyNames(adjustmentsHistory ?? []), [adjustmentsHistory]);
}
