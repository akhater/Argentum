/**
 * Auto white balance button.
 *
 * Ours. Upstream RapidRAW has never seen this file, so it can never conflict.
 * `Color.tsx` (theirs) gets one line: a mount point beside their K and picker
 * buttons.
 *
 * Detection is harvested from darktable — see src-tauri/src/mods/auto_wb.rs.
 * The answer comes back in RapidRAW 1.6.5's units, an absolute kelvin and tint,
 * and is written exactly the way their area picker writes its own: in their
 * Kelvin mode as kelvin, otherwise as the relative slider values that reach
 * it from the camera's as-shot white balance.
 */

import { ag } from './ag';
import { useState } from 'react';
import { Wand2 } from 'lucide-react';
import { useAgTranslation } from './locales';
import { useEditorStore } from '../store/useEditorStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { Adjustments } from '../utils/adjustments';
import {
  getWhiteBalanceMode,
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

export default function AutoWhiteBalanceButton() {
  // From the stores rather than props: this is mounted through a portal, so
  // there is no parent to pass anything down. See Argentum.tsx.
  const adjustments = useEditorStore((s: any) => s.adjustments) as Adjustments;
  const asShotWhiteBalance = useEditorStore((s: any) => s.selectedImage?.asShotWhiteBalance) as
    | WhiteBalance
    | undefined;
  const appSettings = useSettingsStore((s) => s.appSettings);
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
    if (isRunning || !asShotWhiteBalance) {
      return;
    }
    setIsRunning(true);
    try {
      const result: AutoWhiteBalanceResult = await ag('detect_auto_white_balance', {
        jsAdjustments: adjustments,
        mode,
      });
      const picked = result.whiteBalance;
      const isKelvin = getWhiteBalanceMode(appSettings) === WhiteBalanceMode.Kelvin;
      setAdjustments((prev: Adjustments) =>
        isKelvin
          ? withKelvinWhiteBalance(prev, picked)
          : withRelativeWhiteBalance(prev, toRelativeWhiteBalance(asShotWhiteBalance, picked)),
      );
    } catch (err) {
      console.error('Auto white balance failed:', err);
    } finally {
      setIsRunning(false);
    }
  };

  return (
    <button
      onClick={() => run('surfaces')}
      disabled={isRunning || !asShotWhiteBalance}
      className={`w-6 h-6 flex items-center justify-center rounded-md transition-colors ${
        isRunning ? 'text-text-secondary opacity-50' : 'hover:bg-bg-secondary text-text-secondary'
      }`}
      data-tooltip={t('autoWbTooltip')}
    >
      <Wand2 size={16} className={isRunning ? 'animate-pulse' : ''} />
    </button>
  );
}
