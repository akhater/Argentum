/**
 * The white balance menu: Lightroom's list of presets, with Auto among them.
 *
 * Ours. Upstream RapidRAW has never seen this file, so it can never conflict.
 * Auto moved into the menu, as in Lightroom, and the auto white balance wand
 * went.
 *
 * WHERE IT GOES
 *
 * A row of its own at the top of the White Balance section, above
 * Temperature: "Preset" on the left, the menu on the right, where Lightroom
 * has its WB row. It started in their header beside K and the picker, and on
 * a narrow panel that pushed "White Balance" onto two lines even for Flash,
 * the shortest name.
 *
 * Their `Color.tsx` gives Argentum one slot in that section, the
 * `data-argentum="color-tools"` marker in the header. The row is placed from
 * it, the way AiModelsPlacement places its card: from the marker up to their
 * header row, across to the folding body after it (AdjustmentSubSection's
 * markup), and a container of our own is put first in that body. If the body
 * is ever not where that expects, the menu sits in the slot itself, in the
 * header, rather than nowhere.
 *
 * WHAT IT OFFERS
 *
 * As Shot, Auto, Daylight, Cloudy, Shade, Tungsten, Fluorescent, Flash, and
 * Custom, which is not a choice but what the menu says once a slider has moved
 * the white balance off all of them. The label is worked out from the numbers
 * on every render, never stored as a name, so undo, paste and a slider drag all
 * leave it telling the truth.
 *
 * The presets are Adobe Camera Raw's numbers, and they carry over unchanged:
 * RapidRAW 1.6.5's kelvin and tint are the DNG SDK's, the same Robertson
 * isotherms and the same tint scale (`TINT_SCALE` in `white_balance.rs`), read
 * from the camera's own matrices, with positive tint towards magenta.
 *
 * A preset is written as an absolute kelvin whichever slider mode is on. That
 * is what a preset means - Daylight is 5500 K on every photo, pasted or not -
 * and their engine keeps the `whiteBalance` field for exactly that. In relative
 * mode their sliders show it as an offset from the camera's as-shot setting,
 * as they already do for any absolute white balance.
 *
 * AUTO
 *
 * Detection is harvested from darktable - see src-tauri/src/mods/auto_wb.rs.
 * The answer comes back in RapidRAW 1.6.5's units, an absolute kelvin and tint,
 * and is written exactly the way their area picker writes its own: in their
 * Kelvin mode as kelvin, otherwise as the relative slider values that reach
 * it from the camera's as-shot white balance.
 *
 * Auto is the one entry the numbers alone cannot name, because what it finds
 * is different on every photo. So its answer is kept in the edit, as
 * `whiteBalanceAuto`, and the menu says Auto while the white balance still
 * matches it. Their `normalizeLoadedAdjustments` spreads the saved edit over
 * their defaults, so a key it does not know survives a restart without a line
 * of theirs. Every other choice removes it.
 *
 * JPEGs get the same menu. Their as-shot white balance for anything but a RAW
 * is D65, so a preset on a JPEG corrects colours that were already balanced.
 * Left that way for now, on purpose.
 */

import { ag } from './ag';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { AnimatePresence, motion } from 'framer-motion';
import { Check, ChevronDown } from 'lucide-react';
import clsx from 'clsx';
import { useAgTranslation } from './locales';
import { useCompactSliders } from './compactSliders';
import { useEditorStore } from '../store/useEditorStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { Adjustments } from '../utils/adjustments';
import {
  getWhiteBalanceMode,
  resolveWhiteBalance,
  toRelativeWhiteBalance,
  WhiteBalance,
  WhiteBalanceMode,
  withKelvinWhiteBalance,
  withRelativeWhiteBalance,
} from '../utils/whiteBalance';

/** Which neutrality assumption to use when detecting the illuminant. */
export type AutoWbMode = 'surfaces' | 'edges';

interface AutoWhiteBalanceResult {
  x: number;
  y: number;
  temperatureK: number;
  whiteBalance: WhiteBalance;
}

/** Their adjustments, plus the white balance Auto last found. */
type WithAuto = Adjustments & { whiteBalanceAuto?: WhiteBalance };

type Choice = 'asShot' | 'auto' | 'daylight' | 'cloudy' | 'shade' | 'tungsten' | 'fluorescent' | 'flash' | 'custom';

/** Adobe Camera Raw's white balance presets, in its kelvin and tint. */
const PRESETS: Array<{ id: Choice; whiteBalance: WhiteBalance }> = [
  { id: 'daylight', whiteBalance: { temperature: 5500, tint: 10 } },
  { id: 'cloudy', whiteBalance: { temperature: 6500, tint: 10 } },
  { id: 'shade', whiteBalance: { temperature: 7500, tint: 10 } },
  { id: 'tungsten', whiteBalance: { temperature: 2850, tint: 0 } },
  { id: 'fluorescent', whiteBalance: { temperature: 3800, tint: 21 } },
  { id: 'flash', whiteBalance: { temperature: 5500, tint: 0 } },
];

type AgKey = Parameters<ReturnType<typeof useAgTranslation>>[0];

const LABELS: Record<Choice, AgKey> = {
  asShot: 'wbAsShot',
  auto: 'wbAuto',
  daylight: 'wbDaylight',
  cloudy: 'wbCloudy',
  shade: 'wbShade',
  tungsten: 'wbTungsten',
  fluorescent: 'wbFluorescent',
  flash: 'wbFlash',
  custom: 'wbCustom',
};

/** Space between the button and the menu, and the least the menu keeps from the window's edge. */
const GAP = 4;
const MARGIN = 8;

/** The same white balance, give or take what a slider leaves after the decimal point. */
const same = (a: WhiteBalance, b: WhiteBalance) =>
  Math.abs(a.temperature - b.temperature) < 0.5 && Math.abs(a.tint - b.tint) < 0.05;

/** Which entry describes the white balance the photo has now. */
const currentChoice = (asShot: WhiteBalance, adjustments: WithAuto): Choice => {
  const current = resolveWhiteBalance(asShot, adjustments);
  if (same(current, asShot)) {
    return 'asShot';
  }
  if (adjustments.whiteBalanceAuto && same(current, adjustments.whiteBalanceAuto)) {
    return 'auto';
  }
  return PRESETS.find((preset) => same(current, preset.whiteBalance))?.id ?? 'custom';
};

const withoutAuto = (adjustments: WithAuto): WithAuto => {
  const next = { ...adjustments };
  delete next.whiteBalanceAuto;
  return next;
};

const kelvin = (whiteBalance: WhiteBalance) => `${Math.round(whiteBalance.temperature)} K`;

interface MenuPosition {
  top: number;
  right: number;
  above: boolean;
}

/**
 * A container of ours, first in the folding body of the section `slot` is in,
 * or null when the body is not where AdjustmentSubSection puts it: the element
 * after their header row, whose first child holds the sliders.
 */
function useSectionRow(slot: HTMLElement): HTMLElement | null {
  const [row, setRow] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const own = document.createElement('div');
    const header = slot.closest('.cursor-pointer');

    const place = () => {
      const body = header?.nextElementSibling?.firstElementChild;
      if (body?.querySelector('input[type="range"]')) {
        // React only ever positions its own nodes, so ours stays first.
        if (body.firstElementChild !== own) {
          body.prepend(own);
        }
        setRow(own);
      } else {
        own.remove();
        setRow(null);
      }
    };

    place();
    // Their section re-renders its sliders when the K mode switches.
    const observer = new MutationObserver(place);
    if (header?.parentElement) {
      observer.observe(header.parentElement, { childList: true, subtree: true });
    }
    return () => {
      observer.disconnect();
      own.remove();
    };
  }, [slot]);

  return row;
}

export default function WhiteBalanceMenu({ slot }: { slot: HTMLElement }) {
  const row = useSectionRow(slot);
  const isCompact = useCompactSliders((s) => s.on);
  // From the stores rather than props: this is mounted through a portal, so
  // there is no parent to pass anything down. See Argentum.tsx.
  const adjustments = useEditorStore((s) => s.adjustments) as WithAuto;
  const asShot = useEditorStore((s) => s.selectedImage?.asShotWhiteBalance);
  const appSettings = useSettingsStore((s) => s.appSettings);
  const { setAdjustments } = useEditorActions();
  const t = useAgTranslation();
  const [isOpen, setIsOpen] = useState(false);
  const [isDetecting, setIsDetecting] = useState(false);
  const [position, setPosition] = useState<MenuPosition | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const choice = asShot ? currentChoice(asShot, adjustments) : 'asShot';
  const current = asShot ? resolveWhiteBalance(asShot, adjustments) : undefined;

  // Under the button, right edges lined up, or above it when the panel is too
  // close to the bottom of the window for it to fit.
  useLayoutEffect(() => {
    const trigger = triggerRef.current?.getBoundingClientRect();
    const menu = menuRef.current;
    if (!isOpen || !trigger || !menu) {
      return;
    }
    const height = menu.offsetHeight;
    const fitsBelow = trigger.bottom + GAP + height <= window.innerHeight - MARGIN;
    setPosition({
      top: fitsBelow ? trigger.bottom + GAP : Math.max(MARGIN, trigger.top - GAP - height),
      right: Math.max(MARGIN, window.innerWidth - trigger.right),
      above: !fitsBelow,
    });
    const selected = menu.querySelector<HTMLButtonElement>('[aria-checked="true"]:not(:disabled)');
    (selected ?? menu.querySelector<HTMLButtonElement>('button:not(:disabled)'))?.focus({ preventScroll: true });
  }, [isOpen]);

  // Keys go to the menu while it is open. Captured on the window, because the
  // editor's own shortcuts listen there too and the arrows would otherwise
  // change photo underneath it.
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    const close = () => setIsOpen(false);
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!menuRef.current?.contains(target) && !triggerRef.current?.contains(target)) {
        close();
      }
    };
    const onScroll = (e: Event) => {
      if (!menuRef.current?.contains(e.target as Node)) {
        close();
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []);
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const focus = (i: number) => items[(i + items.length) % items.length]?.focus();
      switch (e.key) {
        case 'Escape':
          close();
          triggerRef.current?.focus();
          break;
        case 'ArrowDown':
          focus(index + 1);
          break;
        case 'ArrowUp':
          focus(index < 0 ? -1 : index - 1);
          break;
        case 'Home':
          focus(0);
          break;
        case 'End':
          focus(-1);
          break;
        case 'Tab':
          close();
          return;
        case 'Enter':
        case ' ':
          // The focused entry still clicks; nothing else hears the key.
          e.stopPropagation();
          return;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    document.addEventListener('mousedown', onMouseDown);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', close);
    };
  }, [isOpen]);

  const choosePreset = (whiteBalance: WhiteBalance) =>
    setAdjustments((prev: WithAuto) => withoutAuto(withKelvinWhiteBalance(prev, whiteBalance)));

  const chooseAsShot = () =>
    setAdjustments((prev: WithAuto) => withoutAuto(withRelativeWhiteBalance(prev, { temperature: 0, tint: 0 })));

  /**
   * Only 'surfaces' is offered. 'edges' exists in the backend but returns
   * implausible illuminants in this pipeline - see the note on DetectMode::Edges
   * in mods/auto_wb.rs. Better to offer one mode that works than two where one
   * quietly ruins the picture.
   */
  const chooseAuto = async () => {
    if (isDetecting || !asShot) {
      return;
    }
    const path = useEditorStore.getState().selectedImage?.path;
    setIsDetecting(true);
    try {
      const result: AutoWhiteBalanceResult = await ag('detect_auto_white_balance', {
        jsAdjustments: adjustments,
        mode: 'surfaces' satisfies AutoWbMode,
      });
      // Another photo may be open by the time the answer arrives.
      if (useEditorStore.getState().selectedImage?.path !== path) {
        return;
      }
      const picked = result.whiteBalance;
      const isKelvin = getWhiteBalanceMode(appSettings) === WhiteBalanceMode.Kelvin;
      setAdjustments((prev: WithAuto) => {
        const next = isKelvin
          ? withKelvinWhiteBalance(prev, picked)
          : withRelativeWhiteBalance(prev, toRelativeWhiteBalance(asShot, picked));
        return { ...next, whiteBalanceAuto: resolveWhiteBalance(asShot, next) };
      });
    } catch (err) {
      console.error('Auto white balance failed:', err);
    } finally {
      setIsDetecting(false);
    }
  };

  const entry = (id: Choice, onChoose: (() => void) | null, detail?: string) => {
    const isChosen = choice === id;
    return (
      <button
        key={id}
        type="button"
        role="menuitemradio"
        aria-checked={isChosen}
        disabled={!onChoose}
        onClick={() => {
          setIsOpen(false);
          onChoose?.();
        }}
        className={clsx(
          'w-full text-left pl-2 pr-3 py-1.5 text-sm rounded-md flex items-center gap-2 whitespace-nowrap',
          'transition-colors duration-150 outline-hidden',
          isChosen && 'bg-bg-primary font-semibold',
          isChosen || onChoose ? 'text-text-primary' : 'text-text-secondary',
          onChoose ? 'hover:bg-bg-primary focus-visible:bg-bg-primary' : 'cursor-default',
        )}
      >
        <Check size={14} className={clsx('shrink-0', !isChosen && 'invisible')} />
        <span className="grow">{t(LABELS[id])}</span>
        {detail && <span className="pl-4 text-xs font-normal text-text-secondary tabular-nums">{detail}</span>}
      </button>
    );
  };

  const separator = (key: string) => <div key={key} className="h-px bg-text-secondary/20 my-1 mx-2" />;

  // In the row it reads like a slider's value, the same size and colour; in
  // the header, where it only lands if the row cannot be placed, like K.
  const small = !row || isCompact;
  const trigger = (
    <button
      ref={triggerRef}
      type="button"
      onClick={() => setIsOpen((open) => !open)}
      disabled={!asShot}
      aria-haspopup="menu"
      aria-expanded={isOpen}
      className={clsx(
        'pl-1.5 pr-1 flex items-center gap-0.5 rounded-md whitespace-nowrap transition-colors',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        small ? 'text-xs' : 'text-sm',
        row ? '-mr-1' : '',
        row && isCompact ? 'h-5' : 'h-6',
        isOpen
          ? 'bg-bg-secondary text-text-primary'
          : clsx('hover:bg-bg-secondary', row ? 'text-text-primary' : 'text-text-secondary'),
      )}
      data-tooltip={isOpen || row ? undefined : t('wbMenuTooltip')}
    >
      <span className={isDetecting ? 'animate-pulse' : undefined}>{t(LABELS[isDetecting ? 'auto' : choice])}</span>
      <ChevronDown size={12} className={clsx('transition-transform duration-200', isOpen && 'rotate-180')} />
    </button>
  );

  return (
    <>
      {row
        ? createPortal(
            // Spaced like the sliders under it: their gap, or Row spacing in compact panels.
            <div
              className={clsx('flex items-center justify-between gap-2', !isCompact && 'mb-2')}
              style={isCompact ? { marginBottom: 'var(--ag-slider-gap, 4px)' } : undefined}
            >
              <span className={clsx('font-medium text-text-secondary select-none', small ? 'text-xs' : 'text-sm')}>
                {t('wbPresetLabel')}
              </span>
              {trigger}
            </div>,
            row,
          )
        : createPortal(trigger, slot)}
      {createPortal(
        <AnimatePresence>
          {isOpen && (
            <motion.div
              ref={menuRef}
              animate={{ opacity: 1, scale: 1 }}
              className="fixed z-50"
              exit={{ opacity: 0, scale: 0.95 }}
              initial={{ opacity: 0, scale: 0.95 }}
              role="menu"
              style={{
                top: position?.top ?? 0,
                right: position?.right ?? 0,
                transformOrigin: position?.above ? 'bottom right' : 'top right',
                visibility: position ? 'visible' : 'hidden',
              }}
              transition={{ duration: 0.1, ease: 'easeOut' }}
            >
              <div className="bg-surface/95 backdrop-blur-md rounded-lg shadow-xl p-2 min-w-48">
                {entry('asShot', chooseAsShot, asShot && kelvin(asShot))}
                {entry('auto', chooseAuto, choice === 'auto' && current ? kelvin(current) : undefined)}
                {separator('presets')}
                {PRESETS.map((preset) =>
                  entry(preset.id, () => choosePreset(preset.whiteBalance), kelvin(preset.whiteBalance)),
                )}
                {separator('custom')}
                {entry('custom', null, choice === 'custom' && current ? kelvin(current) : undefined)}
              </div>
            </motion.div>
          )}
        </AnimatePresence>,
        document.body,
      )}
    </>
  );
}
