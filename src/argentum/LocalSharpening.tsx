/**
 * A mask's own Sharpen, in the Details of the Masks panel. Ours.
 *
 * Mounted into the same marker as the main Sharpening card, in the copy of
 * their Details panel the Masks panel renders for the active mask (flagged
 * `data-mask`). Their local Sharpness slider beside it is hidden by
 * `sharpening.css`, as the global one is.
 *
 * One slider, as Lightroom's local Sharpness: right sharpens with the same
 * unsharp mask as Details > Sharpening > Sharpen - its radius and its mask -
 * left softens towards that blur. It runs on our engine (`mods/sharpen.rs`,
 * `local_amounts`) and is stored in the mask's own adjustments as
 * `agSharpen.amount`, written the way their panel writes the mask:
 * `adjustments.masks[]` by `activeMaskContainerId`.
 */

import Slider from '../components/ui/Slider';
import Text from '../components/ui/Text';
import { TextVariants } from '../types/typography';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { useAgTranslation } from './locales';
import { readSharpen } from './sharpenSettings';

export default function LocalSharpening() {
  const t = useAgTranslation();
  const adjustments = useEditorStore((s: any) => s.adjustments);
  const activeId: string | null = useEditorStore((s: any) => s.activeMaskContainerId);
  const setEditor = useEditorStore((s: any) => s.setEditor);
  const { setAdjustments } = useEditorActions();

  const mask = adjustments?.masks?.find((m: any) => m.id === activeId);
  if (!mask) {
    return null;
  }
  const amount = Number(mask.adjustments?.agSharpen?.amount) || 0;
  const legacy = Number(mask.adjustments?.sharpness) || 0;
  const globalOff = !readSharpen(adjustments).enabled;

  const update = (patch: Record<string, unknown>) =>
    setAdjustments((prev: any) => ({
      ...prev,
      masks: prev.masks.map((m: any) => (m.id === activeId ? { ...m, adjustments: { ...m.adjustments, ...patch } } : m)),
    }));

  return (
    <div className="flex flex-col gap-1">
      <Slider
        label={t('sharpenLocal')}
        min={-100}
        max={100}
        step={1}
        value={amount}
        defaultValue={0}
        onChange={(e: any) =>
          update({ agSharpen: { ...(mask.adjustments?.agSharpen ?? {}), amount: parseFloat(String(e.target.value)) } })
        }
        onDragStateChange={(dragging: boolean) => setEditor({ isSliderDragging: dragging })}
      />
      <Text variant={TextVariants.small} className="text-text-secondary">
        {t(globalOff ? 'sharpenLocalOff' : 'sharpenLocalHint')}
      </Text>
      {legacy !== 0 && (
        <div className="flex items-center justify-between gap-2 rounded bg-bg-primary px-2 py-1">
          <Text variant={TextVariants.small} className="text-text-secondary">
            {t('sharpenLegacy').replace('{n}', String(legacy))}
          </Text>
          <button
            type="button"
            className="shrink-0 text-[11px] text-accent hover:underline"
            onClick={() => update({ sharpness: 0 })}
          >
            {t('sharpenLegacyRemove')}
          </button>
        </div>
      )}
    </div>
  );
}
