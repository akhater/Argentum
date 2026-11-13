// How a new photo opens from My Lenses' "Correct new photos" (argentum/newPhotoDefaults.ts).
import { describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { corrected, correctsOnOpen, lensOnOpen } from './newPhotoDefaults';
import { useSettingsStore } from '../store/useSettingsStore';
import type { Found } from './lensBuiltin';
import type { Adjustments } from '../utils/adjustments';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const found = (fresh: boolean): Found => ({ correction: null, defaultOn: false, fresh });
const lens = (correctNew?: boolean) => ({ maker: 'Canon', model: 'Canon RF 24-105mm F4L IS USM', correctNew });

describe('whether a photo opens corrected', () => {
  it('only for a RAW with no saved edit from a lens switched on', () => {
    expect(correctsOnOpen(lens(true), found(true), true)).toBe(true);
    expect(correctsOnOpen(lens(false), found(true), true)).toBe(false);
    expect(correctsOnOpen(lens(), found(true), true)).toBe(false);
    expect(correctsOnOpen(null, found(true), true)).toBe(false);
    expect(correctsOnOpen(lens(true), found(false), true)).toBe(false);
    expect(correctsOnOpen(lens(true), found(true), false)).toBe(false);
  });
});

describe('the lens profile a new photo asks for', () => {
  it('hands over the whole EXIF to both calls, so the app reads it as a save reads it', async () => {
    const exif = { Make: 'Canon', Model: 'Canon EOS R6', LensModel: 'RF 50mm', FNumber: 'f/5.6', FocalLength: '50' };
    const lens: [string, string] = ['Canon', 'Canon RF 50mm'];
    vi.mocked(invoke).mockImplementation(async (command) => (command === 'autodetect_lens' ? lens : {}));
    useSettingsStore.setState({
      appSettings: { myLenses: [{ maker: lens[0], model: lens[1], correctNew: true }] },
    } as never);

    await lensOnOpen('photo.CR3', exif, true, found(true));

    expect(invoke).toHaveBeenCalledWith('autodetect_lens', expect.objectContaining({ exif }));
    expect(invoke).toHaveBeenCalledWith('get_lens_distortion_params', { maker: lens[0], model: lens[1], exif });
  });
});

describe('the edit a corrected photo opens with', () => {
  const edit = { exposure: 0.5, lensCorrectionMode: 'manual' } as unknown as Adjustments;
  const database = { lens: ['Canon', 'Canon RF 24-105mm F4L IS USM'] as [string, string], profile: { k1: -0.02 } };

  it('is Auto with the lens database’s profile, and the camera’s own correction when the file has one', () => {
    expect(corrected(edit, database, true)).toEqual({
      exposure: 0.5,
      lensCorrectionMode: 'auto',
      lensMaker: 'Canon',
      lensModel: 'Canon RF 24-105mm F4L IS USM',
      lensDistortionParams: { k1: -0.02 },
      lensBuiltin: true,
    });
  });

  it('is the camera’s own correction alone when the lens database does not know the lens', () => {
    expect(corrected(edit, null, true)).toEqual({ ...edit, lensBuiltin: true });
  });

  it('leaves the camera switch alone for a file without its tables', () => {
    expect(corrected(edit, database, false)).not.toHaveProperty('lensBuiltin');
  });
});
