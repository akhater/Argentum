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
 * What it drives is `mods/sharpen.rs`: RawTherapee's capture sharpening, then
 * darktable's sharpen, with RawTherapee's contrast mask deciding where either
 * lands. Every value is in full-resolution pixels and means the same at any
 * zoom, which is why the card says to judge it at 100%.
 */

import { useEffect, useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import clsx from 'clsx';
import Slider from '../components/ui/Slider';
import Switch from '../components/ui/Switch';
import Text from '../components/ui/Text';
import { TextVariants } from '../types/typography';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { ag } from './ag';
import { useAgTranslation } from './locales';
import { AgSharpen, readSharpen, SHARPEN_DEFAULTS, useSharpenMask } from './sharpenSettings';
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

function AutoChip({ on, label, title, onClick }: { on: boolean; label: string; title: string; onClick(): void }) {
  return (
    <button
      type="button"
      className={clsx(
        'px-1.5 py-0.5 rounded text-[11px] leading-none transition-colors',
        on ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-bg-primary',
      )}
      data-tooltip={title}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

function Group({ title, children, right }: { title: string; children: React.ReactNode; right?: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center justify-between pt-1">
        <Text variant={TextVariants.small} className="uppercase tracking-wide text-text-secondary">
          {title}
        </Text>
        {right}
      </div>
      {children}
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

  const set = (patch: Partial<AgSharpen>) =>
    setAdjustments((prev: any) => ({ ...prev, agSharpen: { ...(prev?.agSharpen ?? {}), ...patch } }));
  const num = (e: any) => parseFloat(String(e.target.value));
  const onDrag = (dragging: boolean) => {
    setEditor({ isSliderDragging: dragging });
    mask.onDragStateChange(dragging);
  };

  const legacy = Number(adjustments?.sharpness) || 0;
  const radiusShown = s.autoRadius ? (auto.radius ?? SHARPEN_DEFAULTS.radius) : s.radius;
  const contrastShown = s.autoContrast ? (auto.contrast ?? s.contrast) : s.contrast;
  const fmt = (v: number) => (Math.round(v * 100) / 100).toString();

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <Text variant={TextVariants.small} className="text-text-secondary">
          {t('sharpenZoomHint')}
        </Text>
        <button
          type="button"
          className={clsx(
            'shrink-0 ml-2 p-1 rounded transition-colors',
            mask.pinned ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-bg-primary',
          )}
          data-tooltip={t('sharpenShowMaskHelp')}
          aria-label={t('sharpenShowMask')}
          aria-pressed={mask.pinned}
          onClick={() => mask.setPinned(!mask.pinned)}
        >
          {mask.pinned ? <Eye size={14} /> : <EyeOff size={14} />}
        </button>
      </div>

      <Group
        title={t('sharpenCapture')}
        right={
          isRaw ? (
            <Switch
              checked={s.capture}
              label=""
              tooltip={t('sharpenCaptureHelp')}
              onChange={(on: boolean) => set({ capture: on })}
            />
          ) : null
        }
      >
        {!isRaw ? (
          <Text variant={TextVariants.small} className="text-text-secondary">
            {t('sharpenCaptureRawOnly')}
          </Text>
        ) : (
          s.capture && (
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
                onDragStateChange={onDrag}
              />
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
                    onDragStateChange={onDrag}
                  />
                </div>
                <AutoChip
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
                onDragStateChange={onDrag}
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
                onDragStateChange={onDrag}
              />
            </>
          )
        )}
      </Group>

      <Group title={t('sharpenUsm')}>
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
          onDragStateChange={onDrag}
        />
        {s.amount > 0 && (
          <>
            <Slider
              label={t('sharpenRadius')}
              min={0.1}
              max={8}
              step={0.05}
              value={s.usmRadius}
              defaultValue={SHARPEN_DEFAULTS.usmRadius}
              fillOrigin="min"
              onChange={(e) => set({ usmRadius: num(e) })}
              onDragStateChange={onDrag}
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
              onDragStateChange={onDrag}
            />
          </>
        )}
      </Group>

      <Group title={t('sharpenMasking')}>
        <div className="flex items-end gap-1">
          <div className="grow">
            <Slider
              label={t('sharpenContrast')}
              min={0}
              max={200}
              step={1}
              value={Math.round(contrastShown)}
              defaultValue={SHARPEN_DEFAULTS.contrast}
              fillOrigin="min"
              onChange={(e) => set({ autoContrast: false, contrast: num(e) })}
              onDragStateChange={onDrag}
            />
          </div>
          <AutoChip
            on={s.autoContrast}
            label={t('sharpenAuto')}
            title={t('sharpenAutoContrastHelp')}
            onClick={() => set({ autoContrast: !s.autoContrast, contrast: Math.round(contrastShown) })}
          />
        </div>
      </Group>

      {s.autoRadius && auto.radius !== null && isRaw && s.capture && (
        <Text variant={TextVariants.small} className="text-text-secondary">
          {t('sharpenAutoRadiusRead').replace('{r}', fmt(auto.radius))}
        </Text>
      )}

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
