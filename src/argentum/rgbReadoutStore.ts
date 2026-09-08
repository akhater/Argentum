/**
 * Shared on/off for the RGB readout. Ours.
 *
 * The toggle button belongs in the Color panel, next to the white balance
 * picker; the readout itself has to live over the image. Two different places
 * in the tree, so the state sits outside both rather than being threaded
 * through their components as props.
 */

import { create } from 'zustand';

interface RgbReadoutState {
  on: boolean;
  toggle: () => void;
}

export const useRgbReadout = create<RgbReadoutState>((set) => ({
  on: false,
  toggle: () => set((s) => ({ on: !s.on })),
}));
