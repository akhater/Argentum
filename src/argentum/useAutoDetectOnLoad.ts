/**
 * Run lens auto-detection when a photo loads, and remember what it found. Ours.
 *
 * WHY DETECTION HAD TO MOVE
 *
 * `handleAutoDetectLens` in their `CropPanel.tsx` is called from exactly one
 * place: `handleModeChange`, when the Auto button is *clicked*. Nothing runs it
 * when a photo opens.
 *
 * So detection only ever happened if you toggled the mode for that particular
 * image. Open a photo already set to Auto and the panel sits on "Waiting for
 * Auto-Detect..." forever. It looked random — some photos worked, some did not
 * — which sent the search after the metadata rather than the trigger, well
 * after the lens name itself was verified present in all 53 files of a folder.
 *
 * Conditions, deliberately narrow: only in auto mode, only when no lens is
 * already chosen, and only once the EXIF has actually arrived. Detection reads
 * `selectedImage.exif` and gives up with "not found" if it is missing, with no
 * retry — so firing early would produce exactly the failure it is meant to fix.
 *
 * WHY IT ALSO WRITES TO MY LENSES
 *
 * Detection already knows the lens; the user should not have to add it to their
 * own gear list by hand afterwards. Adding it means the manual dropdown offers
 * it next time, including for a photo where detection fails — which is the case
 * that most needs a shortcut.
 *
 * Only lenses that were *detected* are added. A lens picked by hand is already
 * a deliberate choice and the user can add it themselves; recording those too
 * would fill the list with anything ever tried.
 */

import { useEffect, useRef } from 'react';
import { useSettingsStore } from '../store/useSettingsStore';

/** Add a lens to My Lenses if it is not already there. */
function rememberLens(maker?: string | null, model?: string | null) {
  if (!maker || !model) {
    return;
  }

  const { appSettings, handleSettingsChange } = useSettingsStore.getState();
  if (!appSettings) {
    return;
  }

  const known: { maker: string; model: string }[] = (appSettings as any).myLenses || [];
  const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();
  if (known.some((l) => same(l.maker, maker) && same(l.model, model))) {
    return;
  }

  const next = [...known, { maker, model }].sort(
    (a, b) => a.maker.localeCompare(b.maker) || a.model.localeCompare(b.model),
  );
  handleSettingsChange({ ...(appSettings as any), myLenses: next });
}

export function useAutoDetectOnLoad(selectedImage: any, adjustments: any, detect: () => void) {
  // Detection is idempotent but not free, and it writes to the adjustments —
  // which would re-run this effect. One attempt per image.
  const attempted = useRef<string | null>(null);
  // The image detection last ran for, so a lens that appears afterwards is
  // known to have been found rather than chosen by hand.
  const detectedFor = useRef<string | null>(null);

  useEffect(() => {
    const path = selectedImage?.path ?? null;
    if (!path) {
      attempted.current = null;
      return;
    }

    if (attempted.current === path) {
      return;
    }

    // Wait for the metadata. Firing before it lands is the same bug in a
    // different disguise.
    if (!selectedImage?.exif?.Make) {
      return;
    }

    if (adjustments?.lensCorrectionMode !== 'auto') {
      return;
    }

    if (adjustments?.lensModel) {
      return;
    }

    attempted.current = path;
    detectedFor.current = path;
    detect();
  }, [
    selectedImage?.path,
    selectedImage?.exif,
    adjustments?.lensCorrectionMode,
    adjustments?.lensModel,
    detect,
  ]);

  // A lens appearing on the image detection just ran for was found by it.
  useEffect(() => {
    if (detectedFor.current !== (selectedImage?.path ?? null)) {
      return;
    }
    if (!adjustments?.lensModel) {
      return;
    }
    detectedFor.current = null;
    rememberLens(adjustments?.lensMaker, adjustments?.lensModel);
  }, [selectedImage?.path, adjustments?.lensMaker, adjustments?.lensModel]);
}
