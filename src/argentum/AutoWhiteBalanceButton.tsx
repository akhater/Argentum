/**
 * Auto white balance button.
 *
 * Ours. Upstream RapidRAW has never seen this file, so it can never conflict.
 * `Color.tsx` (theirs) gets one line: <AutoWhiteBalanceButton ... />
 *
 * Detection is harvested from darktable — see src-tauri/src/mods/auto_wb.rs.
 */

import { ag } from './ag';
import { useState } from 'react';
import { Wand2 } from 'lucide-react';
import { useAgTranslation } from './locales';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { Adjustments } from '../utils/adjustments';

/** Which neutrality assumption to use when detecting the illuminant. */
export type AutoWbMode = 'surfaces' | 'edges';

interface AutoWhiteBalanceResult {
  x: number;
  y: number;
  temperatureK: number;
  temperature: number;
  tint: number;
}

export default function AutoWhiteBalanceButton() {
  // From the store rather than props: this is mounted through a portal, so
  // there is no parent to pass anything down. See Argentum.tsx.
  const adjustments = useEditorStore((s: any) => s.adjustments) as Adjustments;
  const { setAdjustments } = useEditorActions();
  const t = useAgTranslation();
  const [isRunning, setIsRunning] = useState(false);

  /**
   * Only 'surfaces' is offered. 'edges' exists in the backend but returns
   * implausible illuminants in this pipeline — see the note on DetectMode::Edges
   * in mods/auto_wb.rs. It was on shift-click; better to offer one mode that
   * works than two where one quietly ruins the picture.
   */
  const run = async (mode: AutoWbMode) => {
    if (isRunning) {
      return;
    }
    setIsRunning(true);
    try {
      const result: AutoWhiteBalanceResult = await ag('detect_auto_white_balance', {
        jsAdjustments: adjustments,
        mode,
      });
      setAdjustments((prev: Partial<Adjustments>) => ({
        ...prev,
        temperature: Math.round(result.temperature),
        tint: Math.round(result.tint),
      }));
    } catch (err) {
      console.error('Auto white balance failed:', err);
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <button
      // Shift-click for edge mode. Right-click was the obvious choice but the
      // app's own context menu opens over it.
      onClick={() => run('surfaces')}
      disabled={isRunning}
      className={`p-1.5 rounded-md transition-colors ${
        isRunning ? 'text-text-secondary opacity-50' : 'hover:bg-bg-secondary text-text-secondary'
      }`}
      data-tooltip={t('autoWbTooltip')}
    >
      <Wand2 size={16} className={isRunning ? 'animate-pulse' : ''} />
    </button>
  );
}
