import { describe, expect, it } from 'vitest';
import {
  COPYABLE_ADJUSTMENT_KEYS,
  ADJUSTMENT_GROUPS,
  INITIAL_ADJUSTMENTS,
  type Adjustments,
} from '../../utils/adjustments';
import { EDIT_SECTION_KEYS } from '../editSections';
import { presetAt } from './presetAmount';
import {
  hoverEdit,
  lookKeys,
  lookOf,
  lookOnto,
  madeForWhiteBalance,
  offeredOptions,
  optionIsOn,
  PROVIDER_LOOK_PATH,
  providerLookOff,
  sameOptions,
  sameProviderLook,
  whiteBalanceKeys,
  withOption,
  withOwnWb,
} from './providerLook';

const curve = [
  { x: 0, y: 0 },
  { x: 255, y: 255 },
];

/** A photo with a look on, which replaced a fitted Base Curve rendering. */
const withLook = (camera = 'cam1', look = 'abc'): Partial<Adjustments> => ({
  lutPath: PROVIDER_LOOK_PATH,
  lutName: 'Invented Stock',
  lutIntensity: 80,
  rawToneRendering: 'linear',
  rawToneCurve: null,
  rawToneCurveMode: null,
  agProviderLook: {
    camera,
    look,
    before: { rawToneRendering: 'baseCurve', rawToneCurve: curve, rawToneCurveMode: 'baseCurve' },
  },
});

const realLut = { lutPath: '/luts/film.cube', lutName: 'film.cube', lutIntensity: 100 };

describe('taking a provider look off', () => {
  it('restores the rendering it replaced and clears the look', () => {
    expect(providerLookOff(withLook())).toEqual({
      rawToneRendering: 'baseCurve',
      rawToneCurve: curve,
      rawToneCurveMode: 'baseCurve',
      agProviderLook: null,
    });
  });

  it('changes nothing on a photo with none', () => {
    expect(providerLookOff({ lutPath: '/luts/film.cube', rawToneRendering: 'linear' })).toEqual({});
    expect(providerLookOff({})).toEqual({});
  });

  it('leaves a look a real lutPath already replaced', () => {
    expect(providerLookOff({ ...withLook(), lutPath: '/luts/film.cube' })).toEqual({});
  });
});

describe('sameProviderLook', () => {
  it('compares camera and look', () => {
    expect(sameProviderLook(withLook('cam1', 'abc'), withLook('cam1', 'abc'))).toBe(true);
    expect(sameProviderLook(withLook('cam1', 'abc'), withLook('cam2', 'abc'))).toBe(false);
    expect(sameProviderLook(withLook('cam1', 'abc'), withLook('cam1', 'xyz'))).toBe(false);
  });

  it('counts a look only while the marker is on', () => {
    expect(sameProviderLook({ ...withLook(), lutPath: '/luts/film.cube' }, {})).toBe(true);
    expect(sameProviderLook(withLook(), {})).toBe(false);
  });
});

describe('an option on a look', () => {
  const OFF_FIRST = { id: 'optionA', label: 'Invented choice', default: false, currentWb: false };
  const ON_FIRST = { id: 'optionB', label: 'Another choice', default: true, currentWb: false };

  const lookWith = (options: Record<string, boolean>): Partial<Adjustments> => ({
    ...withLook(),
    agProviderLook: { camera: 'cam1', look: 'abc', options, before: withLook().agProviderLook!.before },
  });

  it('with no stored value is at its default', () => {
    expect(optionIsOn(OFF_FIRST, {})).toBe(false);
    expect(optionIsOn(ON_FIRST, {})).toBe(true);
  });

  it('with a stored value is at that value', () => {
    expect(optionIsOn(OFF_FIRST, { optionA: true })).toBe(true);
    expect(optionIsOn(ON_FIRST, { optionB: false })).toBe(false);
  });

  it('is stored only while it is away from its default', () => {
    expect(withOption({}, OFF_FIRST, true)).toEqual({ optionA: true });
    expect(withOption({}, ON_FIRST, false)).toEqual({ optionB: false });
    expect(withOption({ optionA: true }, OFF_FIRST, false)).toEqual({});
    expect(withOption({}, OFF_FIRST, false)).toEqual({});
    expect(withOption({ optionA: true }, ON_FIRST, false)).toEqual({ optionA: true, optionB: false });
  });

  it('keeps only what is still offered', () => {
    expect(offeredOptions([OFF_FIRST], { optionA: true, gone: true })).toEqual({ optionA: true });
    expect(offeredOptions([], { optionA: true })).toEqual({});
    expect(offeredOptions([OFF_FIRST], undefined)).toEqual({});
  });

  it('compares by value, with none stored the same as an empty set', () => {
    expect(sameOptions({ optionA: true }, { optionA: true })).toBe(true);
    expect(sameOptions(undefined, {})).toBe(true);
    expect(sameOptions({ optionA: true }, {})).toBe(false);
    expect(sameOptions({ optionA: true }, { optionA: false })).toBe(false);
    expect(sameOptions({ optionA: true, optionB: false }, { optionB: false, optionA: true })).toBe(true);
  });

  it('goes into the edit with its look', () => {
    expect(lookKeys('cam1', 'abc', 'Invented Stock', { optionA: true }).agProviderLook?.options).toEqual({
      optionA: true,
    });
  });

  it('leaves a look with none as it was before options existed', () => {
    expect(Object.keys(lookKeys('cam1', 'abc', 'Invented Stock').agProviderLook ?? {})).toEqual([
      'camera',
      'look',
      'before',
    ]);
    expect(Object.keys(lookKeys('cam1', 'abc', 'Invented Stock', {}).agProviderLook ?? {})).toEqual([
      'camera',
      'look',
      'before',
    ]);
  });

  it('makes it another look to compare', () => {
    expect(sameProviderLook(lookWith({ optionA: true }), lookWith({ optionA: true }))).toBe(true);
    expect(sameProviderLook(lookWith({ optionA: true }), lookWith({}))).toBe(false);
    expect(sameProviderLook(lookWith({}), withLook())).toBe(true);
  });

  it('travels with a preset, onto the target’s rendering', () => {
    const put = lookOnto({ rawToneRendering: 'default' }, lookWith({ optionA: true }));
    expect(put.agProviderLook?.options).toEqual({ optionA: true });
    expect(put.agProviderLook?.before.rawToneRendering).toBe('default');
  });

  it('fades in with the Amount, as another look does', () => {
    const before = { ...lookWith({}), lutIntensity: 80 } as Adjustments;
    const preset = { ...lookWith({ optionA: true }), lutIntensity: 100 };
    expect(presetAt(before, preset, 50).lutIntensity).toBe(50);
  });

  it('goes off with its look', () => {
    expect(providerLookOff(lookWith({ optionA: true })).agProviderLook).toBeNull();
  });
});

describe('a preset applied to a photo', () => {
  const unedited: Partial<Adjustments> = {
    rawToneRendering: 'dngStandard',
    rawToneCurve: null,
    rawToneCurveMode: null,
  };

  it('with a provider look gets the target photo’s rendering as its before', () => {
    const saved = withLook();
    const put = lookOnto({ ...unedited, rawToneRendering: 'default' }, saved);
    expect(put.agProviderLook).toEqual({
      camera: 'cam1',
      look: 'abc',
      before: { rawToneRendering: 'default', rawToneCurve: null, rawToneCurveMode: null },
    });
    expect(saved.agProviderLook?.before.rawToneRendering).toBe('baseCurve');
  });

  it('with a provider look keeps the first before on a photo that has one', () => {
    const put = lookOnto(withLook('cam1', 'abc'), withLook('cam2', 'xyz'));
    expect(put.agProviderLook).toEqual({
      camera: 'cam2',
      look: 'xyz',
      before: { rawToneRendering: 'baseCurve', rawToneCurve: curve, rawToneCurveMode: 'baseCurve' },
    });
  });

  it('with a provider look goes on in Linear, whatever carried it', () => {
    const fitted = { rawToneRendering: 'baseCurve', rawToneCurve: curve, rawToneCurveMode: null };
    const { lutPath, agProviderLook } = withLook();
    const put = lookOnto(fitted, { lutPath, agProviderLook });
    expect([put.rawToneRendering, put.rawToneCurve, put.rawToneCurveMode]).toEqual(['linear', null, null]);
  });

  it('with a real LUT turns a provider look off, rendering included', () => {
    expect(lookOnto(withLook(), realLut)).toEqual({
      rawToneRendering: 'baseCurve',
      rawToneCurve: curve,
      rawToneCurveMode: 'baseCurve',
      agProviderLook: null,
      ...realLut,
    });
  });

  it('saved with no LUT turns a provider look off, rendering included', () => {
    expect(lookOnto(withLook(), { lutPath: null }).rawToneRendering).toBe('baseCurve');
  });

  it('with a real LUT and its own rendering keeps the preset’s rendering', () => {
    const preset = { ...realLut, rawToneRendering: 'default' };
    expect(lookOnto(withLook(), preset).rawToneRendering).toBe('default');
  });

  it('without a LUT leaves a provider look on', () => {
    expect(lookOnto(withLook(), { exposure: 1 })).toEqual({ exposure: 1 });
  });

  it('fades a different provider look in with its Amount', () => {
    const before = { ...withLook('cam1', 'abc'), lutIntensity: 80 } as Adjustments;
    const preset = { ...withLook('cam1', 'xyz'), lutIntensity: 100 };
    expect(presetAt(before, preset, 50).lutIntensity).toBe(50);
  });

  it('keeps a same provider look’s strength in the Amount', () => {
    const before = { ...withLook('cam1', 'abc'), lutIntensity: 80 } as Adjustments;
    const preset = { ...withLook('cam1', 'abc'), lutIntensity: 100 };
    expect(presetAt(before, preset, 50).lutIntensity).toBe(90);
  });

  it('switches the look whole: never a mix of two looks at any Amount', () => {
    const before = { ...withLook('cam1', 'abc') } as Adjustments;
    const preset = withLook('cam2', 'xyz');
    expect(presetAt(before, preset, 0).agProviderLook).toEqual(before.agProviderLook);
    expect(presetAt(before, preset, 40).agProviderLook).toEqual(preset.agProviderLook);
  });
});

describe('agProviderLook in the edit’s key lists', () => {
  it('travels in the LUT copy group', () => {
    const group = Object.values(ADJUSTMENT_GROUPS)
      .flat()
      .find((g) => g.label === 'modals.copyPaste.groups.lut');
    expect(group?.keys).toContain('agProviderLook');
    expect(COPYABLE_ADJUSTMENT_KEYS).toContain('agProviderLook');
  });

  it('is in no section’s reset, copy or eye', () => {
    expect(Object.values(EDIT_SECTION_KEYS).flat()).not.toContain('agProviderLook');
  });
});

describe('a look on hover', () => {
  const photo: Adjustments = { ...INITIAL_ADJUSTMENTS, exposure: 1, rawToneRendering: 'dngStandard' };
  const keys = lookKeys('cam1', 'abc', 'Invented Stock');

  it('from the file beside the photo, is the look itself', () => {
    const shown = hoverEdit(photo, keys);
    expect(shown.lutPath).toBe(PROVIDER_LOOK_PATH);
    expect(lookOf(shown)?.look).toBe('abc');
    expect(shown.exposure).toBe(1);
  });

  it('from a preview, names that file for the render and goes on as the look would', () => {
    const shown = hoverEdit(photo, keys, 'C:/cache/.a.cam1.abc.lut.png');
    expect(shown.lutPath).toBe('C:/cache/.a.cam1.abc.lut.png');
    expect(shown.rawToneRendering).toBe('linear');
    expect(shown.lutIntensity).toBe(100);
    expect(shown.agLutDeclaredInput).toBe(true);
  });

  it('from a preview, does not read as a look on the photo', () => {
    expect(lookOf(hoverEdit(photo, keys, 'C:/cache/.a.cam1.abc.lut.png'))).toBeNull();
  });
});

describe('a white balance of its own', () => {
  const ownWb = { option: 'optionW', wb: { temperature: 4300, tint: -12 } };
  const keys = lookKeys('cam1', 'look1', 'Invented Stock', { optionW: true }, ownWb);
  const photo = { rawToneRendering: 'dngStandard' } as const;

  it('is held by the look that was made for it', () => {
    expect(lookOf(lookOnto(photo, keys, true))?.ownWb).toEqual(ownWb);
    expect(lookOf(hoverEdit(photo as never, keys))?.ownWb).toEqual(ownWb);
  });

  it('stays behind when the look comes from elsewhere, like its `before`', () => {
    expect(lookOf(lookOnto(photo, keys))?.ownWb).toEqual({ option: 'optionW', wb: null });
  });

  it('is nowhere in a look with no such option', () => {
    const plain = lookKeys('cam1', 'look1', 'Invented Stock');
    expect(lookOf(plain)).not.toHaveProperty('ownWb');
    expect(lookOf(lookOnto(photo, plain, true))).not.toHaveProperty('ownWb');
  });

  it('changes nothing else of the look on the photo', () => {
    const on = { ...lookOnto(photo, keys, true), lutIntensity: 60 };
    const cooler = { option: 'optionW', wb: { temperature: 6500, tint: 0 } };
    const next = withOwnWb(on, 'cam1', 'look1', cooler);
    expect(next?.lutIntensity).toBe(60);
    expect(lookOf(next ?? {})).toEqual({ ...lookOf(on), ownWb: cooler });
  });

  it('does not land on another look', () => {
    const on = lookOnto(photo, keys, true);
    expect(withOwnWb(on, 'cam1', 'look2', ownWb)).toBeNull();
    expect(withOwnWb(on, 'cam2', 'look1', ownWb)).toBeNull();
    expect(withOwnWb(photo, 'cam1', 'look1', ownWb)).toBeNull();
  });

  it('asks no change of a look that has that file already', () => {
    expect(withOwnWb(lookOnto(photo, keys, true), 'cam1', 'look1', ownWb)).toBeNull();
  });

  it('reads only what decides the white', () => {
    const edit = { temperature: 4, tint: -2, whiteBalance: null, sectionVisibility: { basic: true }, exposure: 1 };
    expect(whiteBalanceKeys(edit as never)).toEqual({
      temperature: 4,
      tint: -2,
      whiteBalance: null,
      sectionVisibility: { basic: true },
    });
    expect(whiteBalanceKeys({ exposure: 1 })).toEqual({});
  });

  it('makes a file for what it was made for, past any slider', () => {
    expect(madeForWhiteBalance(ownWb)).toEqual({
      temperature: 0,
      tint: 0,
      whiteBalance: ownWb.wb,
      sectionVisibility: null,
    });
    expect(madeForWhiteBalance(null).whiteBalance).toBeNull();
  });
});
