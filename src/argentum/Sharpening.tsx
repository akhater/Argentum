/**
 * The Sharpening card in Details. Ours.
 *
 * Mounted inside RapidRAW's own Sharpening section, through the one marker
 * their Details panel carries for it, so it sits where sharpening has always
 * been, folds and reorders with their Sections menu, and their title stays.
 * Their two sliders are hidden by `sharpening.css` while this is mounted -
 * only in the global panel: the marker in a mask's Details is skipped, and a
 * mask keeps their local Sharpness slider.
 *
 * Three things on screen, the rest folded away, because AK's first look at
 * every knob at once was "confusing AF":
 *
 *   the switch   all of it on or off - the before and after
 *   Capture      RawTherapee's, RAW only, automatic: undoes the softness
 *                every RAW has off the sensor
 *   Sharpen      darktable's, by hand: extra crunch, off until raised
 *
 * Each has a Mask (RawTherapee's contrast mask: what is protected) and an
 * eye that shows it - white is sharpened, black is left alone. Sharpen's
 * mask is capture's until it is given its own.
 *
 * What it drives is `mods/sharpen.rs`. Every value is in full-resolution
 * pixels and means the same at any zoom, which is why it says to judge it at
 * 100%.
 */

import { useEffect, useState } from 'react';
import { ChevronDown, Eye, EyeOff } from 'lucide-react';
import clsx from 'clsx';
import Slider from '../components/ui/Slider';
import Switch from '../components/ui/Switch';
import Text from '../components/ui/Text';
import { TextVariants } from '../types/typography';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { ag } from './ag';
import { useAgTranslation } from './locales';
import {
  AgSharpen,
  CAPTURE_MASK,
  MaskMode,
  readSharpen,
  SHARPEN_DEFAULTS,
  SHARPEN_MASK,
  useSharpenMask,
} from './sharpenSettings';
import './sharpening.css';

interface AutoValues {
  radius: number | null;
  contrast: number | null;
}

/** What auto picked for this photo, asked again whenever the picture changes. */
function useAutoValues(path: string | undefined, adjustments: any): AutoValues {
  const [values, setValues] = useState<AutoValues>({ radius: null, contrast: null });
  useEffect(() => {
    setValues({ radius: null, contrast: null });
  }, [path]);
  useEffect(() => {
    if (!path) {
      return;
    }
    // After the render the change sets off, which is what measures.
    const timer = window.setTimeout(() => {
      ag<AutoValues>('sharpen_auto', { path })
        .then(setValues)
        .catch(() => {});
    }, 600);
    return () => window.clearTimeout(timer);
  }, [path, adjustments]);
  return values;
}

function Chip({ on, label, title, onClick }: { on: boolean; label: string; title?: string; onClick(): void }) {
  return (
    <button
      type="button"
      className={clsx(
        'shrink-0 px-1.5 py-0.5 rounded text-[11px] leading-none transition-colors',
        on ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-bg-primary',
      )}
      data-tooltip={title}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function EyeButton({ on, title, onClick }: { on: boolean; title: string; onClick(): void }) {
  return (
    <button
      type="button"
      className={clsx(
        'shrink-0 p-1 rounded transition-colors',
        on ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-bg-primary',
      )}
      data-tooltip={title}
      aria-label={title}
      aria-pressed={on}
      onClick={onClick}
    >
      {on ? <Eye size={14} /> : <EyeOff size={14} />}
    </button>
  );
}

function Group({ title, right, children }: { title: string; right?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between gap-2 pt-1">
        <Text variant={TextVariants.small} className="uppercase tracking-wide text-text-secondary">
          {title}
        </Text>
        <div className="flex items-center gap-1">{right}</div>
      </div>
      {children}
    </div>
  );
}

function More({ children }: { children: React.ReactNode }) {
  const t = useAgTranslation();
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-col gap-1">
      <button
        type="button"
        className="flex items-center gap-1 self-start text-[11px] text-text-secondary hover:text-text-primary"
        onClick={() => setOpen(!open)}
      >
        <ChevronDown size={12} className={clsx('transition-transform', open && 'rotate-180')} />
        {t(open ? 'sharpenLess' : 'sharpenMore')}
      </button>
      {open && children}
    </div>
  );
}

export default function Sharpening() {
  const t = useAgTranslation();
  const adjustments = useEditorStore((s: any) => s.adjustments);
  const selectedImage = useEditorStore((s: any) => s.selectedImage);
  const setEditor = useEditorStore((s: any) => s.setEditor);
  const { setAdjustments } = useEditorActions();
  const path: string | undefined = selectedImage?.path;
  const isRaw = selectedImage?.isRaw === true;
  const s = readSharpen(adjustments);
  const auto = useAutoValues(path, adjustments);
  const mask = useSharpenMask();

  // Nothing is sharpened, so there is no mask to look at: switching
  // sharpening off - or capture, while its mask is up - goes back to the photo.
  const { clear } = mask;
  useEffect(() => {
    if (mask.pinned !== null && (!s.enabled || (mask.pinned === CAPTURE_MASK && !s.capture))) {
      clear();
    }
  }, [s.enabled, s.capture, mask.pinned, clear]);

  const set = (patch: Partial<AgSharpen>) =>
    setAdjustments((prev: any) => ({ ...prev, agSharpen: { ...(prev?.agSharpen ?? {}), ...patch } }));
  const num = (e: any) => parseFloat(String(e.target.value));
  const onDrag = (mode: MaskMode) => {
    const held = mask.dragOf(mode);
    return (dragging: boolean) => {
      setEditor({ isSliderDragging: dragging });
      held(dragging);
    };
  };

  // On a JPEG the manual sharpen is all there is, and its mask is its own.
  const usmOwn = !isRaw || s.usmOwnMask;
  const radiusShown = s.autoRadius ? (auto.radius ?? SHARPEN_DEFAULTS.radius) : s.radius;
  const contrastShown = s.autoContrast ? (auto.contrast ?? s.contrast) : s.contrast;
  const usmContrastShown = s.usmAutoContrast ? (auto.contrast ?? s.usmContrast) : s.usmContrast;
  const legacy = Number(adjustments?.sharpness) || 0;

  return (
    <div className="flex flex-col gap-2">
      <Switch
        checked={s.enabled}
        label={t('sharpenEnabled')}
        tooltip={t('sharpenEnabledHelp')}
        onChange={(on: boolean) => set({ enabled: on })}
      />

      <div className={clsx('flex flex-col gap-2', !s.enabled && 'opacity-40 pointer-events-none')}>
        {isRaw && (
          <Group
            title={t('sharpenCapture')}
            right={
              <>
                <EyeButton
                  on={mask.pinned === CAPTURE_MASK}
                  title={t('sharpenShowMaskHelp')}
                  onClick={() => mask.toggle(CAPTURE_MASK)}
                />
                <Switch
                  checked={s.capture}
                  label=""
                  tooltip={t('sharpenCaptureHelp')}
                  onChange={(on: boolean) => set({ capture: on })}
                />
              </>
            }
          >
            {s.capture && (
              <>
                <Slider
                  label={t('sharpenAmount')}
                  min={0}
                  max={100}
                  step={1}
                  value={s.captureAmount}
                  defaultValue={SHARPEN_DEFAULTS.captureAmount}
                  fillOrigin="min"
                  suffix="%"
                  onChange={(e) => set({ captureAmount: num(e) })}
                  onDragStateChange={onDrag(CAPTURE_MASK)}
                />
                <div className="flex items-end gap-1">
                  <div className="grow">
                    <Slider
                      label={t('sharpenMask')}
                      min={0}
                      max={200}
                      step={1}
                      value={Math.round(contrastShown)}
                      defaultValue={SHARPEN_DEFAULTS.contrast}
                      fillOrigin="min"
                      onChange={(e) => set({ autoContrast: false, contrast: num(e) })}
                      onDragStateChange={onDrag(CAPTURE_MASK)}
                    />
                  </div>
                  <Chip
                    on={s.autoContrast}
                    label={t('sharpenAuto')}
                    title={t('sharpenAutoContrastHelp')}
                    onClick={() => set({ autoContrast: !s.autoContrast, contrast: Math.round(contrastShown) })}
                  />
                </div>
                <More>
                  <div className="flex items-end gap-1">
                    <div className="grow">
                      <Slider
                        label={t('sharpenRadius')}
                        min={0.4}
                        max={2}
                        step={0.01}
                        value={radiusShown}
                        defaultValue={SHARPEN_DEFAULTS.radius}
                        fillOrigin="min"
                        onChange={(e) => set({ autoRadius: false, radius: num(e) })}
                        onDragStateChange={onDrag(CAPTURE_MASK)}
                      />
                    </div>
                    <Chip
                      on={s.autoRadius}
                      label={t('sharpenAuto')}
                      title={t('sharpenAutoRadiusHelp')}
                      onClick={() => set({ autoRadius: !s.autoRadius, radius: radiusShown })}
                    />
                  </div>
                  <Slider
                    label={t('sharpenCorner')}
                    min={-0.5}
                    max={0.5}
                    step={0.01}
                    value={s.cornerBoost}
                    defaultValue={SHARPEN_DEFAULTS.cornerBoost}
                    onChange={(e) => set({ cornerBoost: num(e) })}
                    onDragStateChange={onDrag(CAPTURE_MASK)}
                  />
                  <Slider
                    label={t('sharpenIterations')}
                    min={1}
                    max={100}
                    step={1}
                    value={s.iterations}
                    defaultValue={SHARPEN_DEFAULTS.iterations}
                    fillOrigin="min"
                    onChange={(e) => set({ iterations: Math.round(num(e)) })}
                    onDragStateChange={onDrag(CAPTURE_MASK)}
                  />
                </More>
              </>
            )}
          </Group>
        )}

        <Group
          title={t('sharpenUsm')}
          right={
            <EyeButton
              on={mask.pinned === SHARPEN_MASK}
              title={t('sharpenShowMaskHelp')}
              onClick={() => mask.toggle(SHARPEN_MASK)}
            />
          }
        >
          <Slider
            label={t('sharpenAmount')}
            min={0}
            max={200}
            step={1}
            value={s.amount}
            defaultValue={SHARPEN_DEFAULTS.amount}
            fillOrigin="min"
            suffix="%"
            onChange={(e) => set({ amount: num(e) })}
            onDragStateChange={onDrag(SHARPEN_MASK)}
          />
          {s.amount > 0 && (
            <>
              {isRaw && (
                <div className="flex items-center justify-between">
                  <Text variant={TextVariants.small} className="text-text-secondary">
                    {t('sharpenMask')}
                  </Text>
                  <div className="flex gap-1">
                    <Chip
                      on={!s.usmOwnMask}
                      label={t('sharpenMaskSame')}
                      title={t('sharpenMaskSameHelp')}
                      onClick={() => set({ usmOwnMask: false })}
                    />
                    <Chip
                      on={s.usmOwnMask}
                      label={t('sharpenMaskOwn')}
                      title={t('sharpenMaskOwnHelp')}
                      onClick={() => set({ usmOwnMask: true })}
                    />
                  </div>
                </div>
              )}
              {usmOwn && (
                <div className="flex items-end gap-1">
                  <div className="grow">
                    <Slider
                      label={t('sharpenMask')}
                      min={0}
                      max={200}
                      step={1}
                      value={Math.round(usmContrastShown)}
                      defaultValue={SHARPEN_DEFAULTS.usmContrast}
                      fillOrigin="min"
                      onChange={(e) => set({ usmAutoContrast: false, usmContrast: num(e) })}
                      onDragStateChange={onDrag(SHARPEN_MASK)}
                    />
                  </div>
                  <Chip
                    on={s.usmAutoContrast}
                    label={t('sharpenAuto')}
                    title={t('sharpenAutoContrastHelp')}
                    onClick={() =>
                      set({ usmAutoContrast: !s.usmAutoContrast, usmContrast: Math.round(usmContrastShown) })
                    }
                  />
                </div>
              )}
              <More>
                <Slider
                  label={t('sharpenRadius')}
                  min={0.1}
                  max={8}
                  step={0.05}
                  value={s.usmRadius}
                  defaultValue={SHARPEN_DEFAULTS.usmRadius}
                  fillOrigin="min"
                  onChange={(e) => set({ usmRadius: num(e) })}
                  onDragStateChange={onDrag(SHARPEN_MASK)}
                />
                <Slider
                  label={t('sharpenThreshold')}
                  min={0}
                  max={10}
                  step={0.1}
                  value={s.threshold}
                  defaultValue={SHARPEN_DEFAULTS.threshold}
                  fillOrigin="min"
                  onChange={(e) => set({ threshold: num(e) })}
                  onDragStateChange={onDrag(SHARPEN_MASK)}
                />
              </More>
            </>
          )}
        </Group>

        <Text variant={TextVariants.small} className="text-text-secondary">
          {t('sharpenZoomHint')}
        </Text>
      </div>

      {legacy !== 0 && (
        <div className="flex items-center justify-between gap-2 rounded bg-bg-primary px-2 py-1">
          <Text variant={TextVariants.small} className="text-text-secondary">
            {t('sharpenLegacy').replace('{n}', String(legacy))}
          </Text>
          <button
            type="button"
            className="shrink-0 text-[11px] text-accent hover:underline"
            onClick={() => setAdjustments((prev: any) => ({ ...prev, sharpness: 0 }))}
          >
            {t('sharpenLegacyRemove')}
          </button>
        </div>
      )}
    </div>
  );
}
