/**
 * The top bar's Focus points button: an icon, its words in the tooltip. Ours.
 *
 * Sits after RGB with the other view tools of the photo. The tooltip says
 * what a click does, and on a photo whose file records no point, why nothing
 * is drawn (Shoebox's viewer words, src/components/Viewer.tsx at 580c5a6d).
 */

import clsx from 'clsx';
import { useEditorStore } from '../store/useEditorStore';
import type { Focus } from './ag.generated';
import { FocusIcon, hasFocusPoints } from './FocusPoints';
import { usePhotoFocus, useFocusPoints } from './focusSwitch';
import { hiddenByTransform } from './focusPlacement';
import { useAgTranslation, type AgKey } from './locales';

/** The tooltip, by what a click would change and what the photo holds. */
export function focusTip(on: boolean, focus: Focus | null | undefined, transformed: boolean): AgKey {
  if (!on) return 'focusPointsTip';
  if (focus !== undefined && !hasFocusPoints(focus)) {
    return focus?.mode === 'Manual focus' ? 'focusPointsManualTip' : 'focusPointsUnrecordedTip';
  }
  return transformed ? 'focusPointsTransformTip' : 'focusPointsHideTip';
}

export default function FocusPointsButton() {
  const t = useAgTranslation();
  const on = useFocusPoints((s) => s.on);
  const toggle = useFocusPoints((s) => s.toggle);
  const transformed = useEditorStore((s) => hiddenByTransform(s.adjustments));
  // undefined: not read yet, so the tooltip makes no claim about the photo.
  const focus = usePhotoFocus(on);

  return (
    <button
      className={clsx('agm-btn', on && 'agm-on')}
      onClick={toggle}
      aria-pressed={on}
      aria-label={t('focusPointsTip')}
      data-tooltip={t(focusTip(on, focus, transformed))}
    >
      <FocusIcon on={on} />
    </button>
  );
}
