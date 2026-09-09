/**
 * Run lens auto-detection when a photo loads. Ours.
 *
 * WHY
 *
 * `handleAutoDetectLens` in their `CropPanel.tsx` is called from exactly one
 * place: `handleModeChange`, when the Auto button is *clicked*. Nothing runs it
 * when a photo opens.
 *
 * So detection only ever happened if you toggled the mode for that particular
 * image. Open a photo already set to Auto and the panel sits on "Waiting for
 * Auto-Detect..." forever. It looked random — some photos worked, some did not —
 * which sent the search after the metadata rather than the trigger, well after
 * the lens name itself was verified present in all 53 files of a folder.
 *
 * Conditions, deliberately narrow: only in auto mode, only when no lens is
 * already chosen, and only once the EXIF has actually arrived. Detection reads
 * `selectedImage.exif` and gives up with "not found" if it is missing, with no
 * retry — so firing early would produce exactly the failure it is meant to fix.
 */

import { useEffect, useRef } from 'react';

export function useAutoDetectOnLoad(
  selectedImage: any,
  adjustments: any,
  detect: () => void,
) {
  // Detection is idempotent but not free, and it writes to the adjustments —
  // which would re-run this effect. One attempt per image.
  const attempted = useRef<string | null>(null);

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
      console.log('[autolens] skip: no exif yet for', path);
      return;
    }

    if (adjustments?.lensCorrectionMode !== 'auto') {
      console.log('[autolens] skip: mode is', adjustments?.lensCorrectionMode, 'for', path);
      return;
    }

    if (adjustments?.lensModel) {
      console.log('[autolens] skip: lens already set to', adjustments.lensModel);
      return;
    }

    // TEMP tracing while the first-few-photos failure is diagnosed.
    console.log('[autolens] firing for', path);
    attempted.current = path;
    detect();
  }, [
    selectedImage?.path,
    selectedImage?.exif,
    adjustments?.lensCorrectionMode,
    adjustments?.lensModel,
    detect,
  ]);
}
