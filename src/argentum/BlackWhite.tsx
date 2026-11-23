/**
 * B&W: Lightroom's Treatment switch, Color or B&W, at the top of Edit > Basic,
 * and its B&W Mix, which takes the Color Mixer's place in Edit > Color while
 * the photo is black and white. Ours.
 *
 * The edit's `agBlackWhite` holds both: `enabled`, and each band's -100..100,
 * how light that colour turns in the grey. The renderer turns the photo grey
 * after Saturation and before Color Grading, with RAWmakase's measured mix, and
 * leaves the Color Mixer, Hue, Saturation and Vibrance out while B&W is on
 * (mods/black_white.rs); they are kept for when the photo goes back to Color.
 * V switches, as in Lightroom and RAWmakase.
 *
 * Auto: a photo going B&W whose mix is all zero starts at its Auto mix, as
 * Lightroom's "Apply auto mix when first converting to black and white" does,
 * and the B&W Mix's Auto button sets it again; it is greyed while the mix is
 * Auto's. A mix set by hand or by a look is kept. The mix is RAWmakase's
 * measured copy of Lightroom's Auto, for the open photo with the edit's white
 * balance and camera profile (mods/black_white.rs `auto`).
 */

import { useEffect, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import clsx from 'clsx';
import { useTranslation } from 'react-i18next';
import Slider from '../components/ui/Slider';
import type { Adjustments } from '../utils/adjustments';
import { useEditorStore } from '../store/useEditorStore';
import { ag as agCommand } from './ag';
import { useAgTranslation } from './locales';
import { typingInto } from './typingInto';
import './blackWhite.css';

export const BLACK_WHITE_KEY = 'agBlackWhite';

/**
 * The mix's bands, in Lightroom's order, named as the Color Mixer names them.
 * Each slider's track runs from black through the band's colour to white
 * (blackWhite.css).
 */
const BANDS = ['reds', 'oranges', 'yellows', 'greens', 'aquas', 'blues', 'purples', 'magentas'] as const;

type Band = (typeof BANDS)[number];
type Mix = Partial<Record<Band, number>>;
type BlackWhite = { enabled: boolean } & Mix;
type Edit = Partial<Adjustments> & { [BLACK_WHITE_KEY]?: BlackWhite };

const sameMix = (a: Mix | undefined, b: Mix | undefined) =>
  BANDS.every((band) => (a?.[band] ?? 0) === (b?.[band] ?? 0));

/** A mix never set, or reset: Auto may take its place. */
const isMixZero = (edit: Edit | null | undefined) => sameMix(edit?.[BLACK_WHITE_KEY], {});

/**
 * The open photo's Auto mix for `edit`, and the photo it is for. Null when
 * the photo is still decoding or cannot be measured; `stale` when another
 * photo was opened before the answer came.
 */
async function measureAuto(edit: unknown): Promise<{ mix: Mix | null; stale: boolean }> {
  const photo = useEditorStore.getState().selectedImage;
  if (!photo?.isReady) {
    return { mix: null, stale: false };
  }
  let mix: Mix | null = null;
  try {
    // A piece of the edit, whose band names are ours: `unknown` from the Rust side.
    mix = (await agCommand('black_white_auto', { path: photo.path, jsAdjustments: edit })) as Mix;
  } catch (err) {
    console.error('Auto B&W mix failed:', err);
  }
  return { mix, stale: useEditorStore.getState().selectedImage?.path !== photo.path };
}

/** A control greyed out while B&W is on, as Lightroom greys them: it has no effect. */
export const NO_EFFECT_IN_BLACK_WHITE = 'opacity-40 grayscale pointer-events-none';

export const isBlackWhite = (adjustments: unknown): boolean =>
  (adjustments as Edit | null)?.[BLACK_WHITE_KEY]?.enabled === true;

function withBlackWhite(prev: Edit, change: Partial<BlackWhite>): Edit {
  const current = prev?.[BLACK_WHITE_KEY] ?? { enabled: false };
  return { ...prev, [BLACK_WHITE_KEY]: { ...current, ...change } };
}

/** As the panels pass them: the setter also takes an update of the previous edit. */
interface Props {
  adjustments: Adjustments;
  setAdjustments(adjustments: Partial<Adjustments>): unknown;
}

/** Color | B&W, at the top of Basic. */
export function TreatmentSwitch({ adjustments, setAdjustments }: Props) {
  const ag = useAgTranslation();
  const blackWhite = isBlackWhite(adjustments);
  const latest = useRef(adjustments);
  latest.current = adjustments;
  // Each press is numbered, so an Auto answer that arrives after a later press is dropped.
  const presses = useRef(0);

  /** Color or B&W; the first conversion of a photo whose mix is all zero brings its Auto mix, in the same step. */
  const choose = async (enabled: boolean) => {
    const press = ++presses.current;
    const first = (edit: Edit) => enabled && !isBlackWhite(edit) && isMixZero(edit);
    let auto: Mix | null = null;
    if (first(latest.current)) {
      const measured = await measureAuto(latest.current);
      if (measured.stale || press !== presses.current) {
        return;
      }
      auto = measured.mix;
    }
    setAdjustments((prev: Edit) => withBlackWhite(prev, auto && first(prev) ? { enabled, ...auto } : { enabled }));
  };
  const chooseRef = useRef(choose);
  chooseRef.current = choose;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() !== 'v' || e.ctrlKey || e.metaKey || e.altKey || e.repeat || typingInto(e.target)) {
        return;
      }
      e.preventDefault();
      chooseRef.current(!isBlackWhite(latest.current));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const option = (enabled: boolean, label: string) => (
    <button
      onClick={() => choose(enabled)}
      className={clsx(
        'relative flex-1 flex items-center justify-center px-3 p-1.5 text-sm font-medium rounded-md transition-colors',
        blackWhite === enabled ? 'text-button-text' : 'text-text-primary hover:bg-surface/50',
      )}
      style={{ WebkitTapHighlightColor: 'transparent' }}
    >
      <span className="relative z-10">{label}</span>
    </button>
  );

  return (
    <div className="w-full p-1 bg-card-active rounded-md mb-3" data-tooltip={ag('bwSwitchTip')}>
      <div className="relative flex w-full">
        <motion.div
          className="absolute top-0 bottom-0 z-0 bg-accent"
          style={{ borderRadius: 6, width: '50%' }}
          animate={{ x: blackWhite ? '100%' : '0%' }}
          transition={{ type: 'spring', bounce: 0.2, duration: 0.6 }}
        />
        {option(false, ag('bwColor'))}
        {option(true, ag('bwBlackWhite'))}
      </div>
    </div>
  );
}

interface MixProps extends Props {
  onDragStateChange?(isDragging: boolean): void;
}

/** What the Auto mix depends on: the photo, its white balance and its camera profile. */
const autoKey = (path: string | null, edit: Record<string, unknown>) =>
  JSON.stringify([
    path,
    edit.temperature,
    edit.tint,
    edit.whiteBalance,
    edit.cameraProfile,
    (edit.sectionVisibility as Record<string, unknown> | undefined)?.basic,
  ]);

const HEADING_BUTTON =
  'text-xs text-text-secondary hover:text-text-primary px-1 disabled:opacity-40 disabled:pointer-events-none';

/**
 * B&W Mix's heading: Auto, which sets the photo's Auto mix and is greyed while
 * the mix is already that, and Reset, every band back to 0. B&W stays on.
 */
export function BlackWhiteMixActions({ adjustments, setAdjustments }: Props) {
  const ag = useAgTranslation();
  const path = useEditorStore((s) => (s.selectedImage?.isReady ? s.selectedImage.path : null));
  const key = autoKey(path, adjustments as unknown as Record<string, unknown>);
  const [auto, setAuto] = useState<{ key: string; mix: Mix } | null>(null);
  const latest = useRef(adjustments);
  latest.current = adjustments;

  // Measured again, after a pause, when the photo, its white balance or its profile changes.
  useEffect(() => {
    if (!path) {
      return;
    }
    let live = true;
    const timer = setTimeout(async () => {
      const { mix, stale } = await measureAuto(latest.current);
      if (live && mix && !stale) {
        setAuto({ key, mix });
      }
    }, 250);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [key, path]);

  const isAuto = auto?.key === key && sameMix((adjustments as Edit)[BLACK_WHITE_KEY], auto.mix);
  const applyAuto = async () => {
    const { mix, stale } = await measureAuto(latest.current);
    if (mix && !stale) {
      setAdjustments((prev: Edit) => withBlackWhite(prev, mix));
    }
  };
  const reset = () =>
    setAdjustments((prev: Edit) => withBlackWhite(prev, Object.fromEntries(BANDS.map((band) => [band, 0]))));

  return (
    <div className="flex items-center gap-1">
      <button className={HEADING_BUTTON} onClick={applyAuto} disabled={isAuto} data-tooltip={ag('bwMixAutoTip')}>
        {ag('bwMixAuto')}
      </button>
      <button className={HEADING_BUTTON} onClick={reset}>
        {ag('bwMixReset')}
      </button>
    </div>
  );
}

/** B&W Mix: how light each colour turns, -100..100. */
export function BlackWhiteMix({ adjustments, setAdjustments, onDragStateChange }: MixProps) {
  const { t } = useTranslation();
  const mix: BlackWhite = (adjustments as Edit)[BLACK_WHITE_KEY] ?? { enabled: true };

  return (
    <div>
      {BANDS.map((band) => (
        <Slider
          key={band}
          label={t(`adjustments.color.mixerColors.${band}`)}
          min={-100}
          max={100}
          step={1}
          value={mix[band] ?? 0}
          onChange={(e) => {
            const value = parseFloat(String(e.target.value));
            setAdjustments((prev: Edit) => withBlackWhite(prev, { [band]: value }));
          }}
          trackClassName={`ag-bw-track-${band}`}
          onDragStateChange={onDragStateChange}
        />
      ))}
    </div>
  );
}
