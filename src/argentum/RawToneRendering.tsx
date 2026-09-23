/**
 * The RAW tone rendering choice. Ours.
 *
 * This is deliberately separate from RapidRAW's AgX/basic tone-mapper switch.
 * It answers a different question: which camera-style rendering should happen
 * at the RAW display-transform stage? Default is the existing Argentum path;
 * Base Curve is a camera-style curve; Auto-Matched is fitted from the JPEG
 * embedded in this RAW file.
 */

import { useCallback, useEffect, useState } from 'react';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { ag } from './ag';
import { useAgTranslation } from './locales';

type RawToneRenderingMode = 'default' | 'baseCurve' | 'autoMatched';

interface ToneCurvePoint {
  x: number;
  y: number;
}

const MODES: RawToneRenderingMode[] = ['default', 'baseCurve', 'autoMatched'];

export default function RawToneRendering() {
  const t = useAgTranslation();
  const selectedImage = useEditorStore((s: any) => s.selectedImage);
  const adjustments = useEditorStore((s: any) => s.adjustments);
  const { setAdjustments } = useEditorActions();
  const path: string | undefined = selectedImage?.path;
  const isRaw = selectedImage?.isRaw === true;
  const current: RawToneRenderingMode = MODES.includes(adjustments?.rawToneRendering)
    ? adjustments.rawToneRendering
    : 'default';
  const [busy, setBusy] = useState(false);

  const ensureCurve = useCallback(
    async (mode: Exclude<RawToneRenderingMode, 'default'>) => {
      if (!path) return;
      setBusy(true);
      try {
        const curve = await ag<ToneCurvePoint[]>('raw_tone_curve', { path, mode });
        if (useEditorStore.getState().selectedImage?.path !== path) return;
        setAdjustments((prev: any) => ({
          ...prev,
          rawToneRendering: mode,
          rawToneCurve: curve,
        }));
      } catch (error) {
        console.warn(`Could not calculate RAW tone curve (${mode})`, error);
      } finally {
        setBusy(false);
      }
    },
    [path, setAdjustments],
  );

  useEffect(() => {
    if (!isRaw || !path || current === 'default' || adjustments?.rawToneCurve?.length >= 2 || busy) {
      return;
    }
    ensureCurve(current);
  }, [adjustments?.rawToneCurve, busy, current, ensureCurve, isRaw, path]);

  if (!isRaw) return null;

  const choose = async (mode: RawToneRenderingMode) => {
    if (busy || mode === current) return;
    if (mode === 'default') {
      setAdjustments((prev: any) => ({
        ...prev,
        rawToneRendering: 'default',
        rawToneCurve: null,
      }));
      return;
    }
    await ensureCurve(mode);
  };

  return (
    <div className="p-2 bg-bg-tertiary rounded-md">
      <div className="flex justify-between items-center mb-2">
        <span className="text-sm font-semibold text-text-primary">{t('rawToneLabel')}</span>
      </div>
      <select
        value={current}
        disabled={busy}
        onChange={(e) => choose(e.target.value as RawToneRenderingMode)}
        className="w-full text-xs bg-bg-primary text-text-primary rounded px-2 py-1.5 disabled:opacity-50"
        data-tooltip={t('rawToneHelp')}
      >
        <option value="default">{t('rawToneDefault')}</option>
        <option value="baseCurve">{t('rawToneBaseCurve')}</option>
        <option value="autoMatched">{t('rawToneAutoMatched')}</option>
      </select>
      {busy && <p className="mt-1 text-xs text-text-secondary">{t('rawToneCalculating')}</p>}
    </div>
  );
}
