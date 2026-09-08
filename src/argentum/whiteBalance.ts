/**
 * White balance picker. Ours.
 *
 * Sends the *coordinates* of the click, not the colour under it.
 *
 * That is the whole fix. The picker used to average pixels off the processed
 * preview and send those — an image that already has the current white balance
 * applied, along with exposure, curves and every other adjustment. Solving from
 * it produced an illuminant, which was assigned to the sliders, which changed
 * the preview, so the next click on the same spot saw different pixels and gave
 * a different answer. It chased its own output and never settled.
 *
 * Rust now samples the geometry-only cache instead: keyed on crop and rotation,
 * untouchable by any colour slider, and the exact image auto-WB analyses. Same
 * point, same answer, and the wand and the picker finally agree with each other.
 *
 * (The earlier version of this file also linearised with a 2.2 gamma, which was
 * the wrong curve for that data anyway. Moot now — Rust decodes what it samples.)
 */

import { invoke } from '@tauri-apps/api/core';
import { useEditorStore } from '../store/useEditorStore';

interface SolvedWhiteBalance {
  x: number;
  y: number;
  temperatureK: number;
  temperature: number;
  tint: number;
}

/**
 * Set the sliders that balance to the point the user clicked.
 *
 * @param x horizontal position within the image, 0..1 from the left
 * @param y vertical position, 0..1 from the top
 */
export async function applyPickedWhiteBalance(
  x: number,
  y: number,
  setAdjustments: (updater: (prev: any) => any) => void,
): Promise<void> {
  try {
    // The cache Rust samples is keyed on geometry, so it needs the adjustments
    // to find the right entry — crop and rotation, not colour.
    const adjustments = useEditorStore.getState().adjustments;

    const result: SolvedWhiteBalance = await invoke('solve_white_balance_at_point', {
      x,
      y,
      jsAdjustments: adjustments,
    });

    setAdjustments((prev: any) => ({
      ...prev,
      temperature: Math.round(result.temperature),
      tint: Math.round(result.tint),
    }));
  } catch (err) {
    // Too dark or too saturated to balance from. Leave the sliders alone
    // rather than throwing the picture somewhere random.
    console.warn('White balance picker:', err);
  }
}
