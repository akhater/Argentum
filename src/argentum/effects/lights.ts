/**
 * Relight's lights: adding, duplicating, removing and showing one. Ours.
 *
 * The Effects panel's light list and the photo's right-click on a light do the
 * same things, so both call these. Taken from RapidRAW's Effects.tsx (main at
 * 454789b7: handleSelectLight, handleDuplicateLight, handleRemoveLight,
 * handleLightContextMenu; 653f1dcd added Duplicate).
 *
 * "Showing" a light is RapidRAW's relight picker: its handles are on the photo
 * and a click there moves it. Only one tool's handles show at a time, so
 * showing the lights puts away the white balance picker and the centre pins.
 */

import { CopyPlus, Trash2 } from 'lucide-react';
import { useCallback } from 'react';
import { useContextMenu } from '../../context/ContextMenuContext';
import { useEditorStore } from '../../store/useEditorStore';
import { Adjustments, RelightLight, createRelightLight } from '../../utils/adjustments';
import { useTheirText } from './theirText';

type SetAdjustments = (update: (prev: Adjustments) => Adjustments) => void;

/** Shows the lights on the photo, with this one selected. */
export function showLight(id: string) {
  useEditorStore.getState().setEditor({
    activeRelightLightId: id,
    isRelightPickerActive: true,
    isWbPickerActive: false,
    activeCenterMarker: null,
  });
}

/** Puts the lights' handles away; the lights themselves stay. */
export function hideLights() {
  useEditorStore.getState().setEditor({ isRelightPickerActive: false });
}

/** A new Lamp in the middle of the photo, added last and shown. */
export function addLight(setAdjustments: SetAdjustments) {
  const light = createRelightLight(0.5, 0.5);
  setAdjustments((prev) => ({ ...prev, relightLights: [...(prev.relightLights || []), light] }));
  showLight(light.id);
}

/** A copy of a light, right after it in the list, shown. */
export function duplicateLight(setAdjustments: SetAdjustments, id: string) {
  const copyId = crypto.randomUUID();
  setAdjustments((prev) => {
    const lights = prev.relightLights || [];
    const index = lights.findIndex((l: RelightLight) => l.id === id);
    if (index < 0) return prev;
    const copy = { ...lights[index], id: copyId };
    return { ...prev, relightLights: [...lights.slice(0, index + 1), copy, ...lights.slice(index + 1)] };
  });
  showLight(copyId);
}

export function removeLight(setAdjustments: SetAdjustments, id: string) {
  setAdjustments((prev) => ({
    ...prev,
    relightLights: (prev.relightLights || []).filter((l: RelightLight) => l.id !== id),
  }));
  if (useEditorStore.getState().activeRelightLightId === id) {
    useEditorStore.getState().setEditor({ activeRelightLightId: null, isRelightPickerActive: false });
  }
}

/** Right-click on a light, in the list or on the photo: Duplicate light, Remove light. */
export function useLightMenu(setAdjustments: SetAdjustments) {
  const t = useTheirText();
  const { showContextMenu } = useContextMenu();
  return useCallback(
    (x: number, y: number, id: string) =>
      showContextMenu(x, y, [
        {
          label: t('adjustments.effects.relightDuplicateLight'),
          icon: CopyPlus,
          onClick: () => duplicateLight(setAdjustments, id),
        },
        {
          label: t('adjustments.effects.relightRemoveLight'),
          icon: Trash2,
          isDestructive: true,
          onClick: () => removeLight(setAdjustments, id),
        },
      ]),
    [showContextMenu, setAdjustments, t],
  );
}
