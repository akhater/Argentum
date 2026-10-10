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
 *
 * WHY IT ASKS THE FILE FOR THE LENS
 *
 * EXIF is cached beside the photo, keyed on the photo rather than the app. A
 * CR3 opened while CR3 lenses went unread has an empty lens saved there, and
 * the backend fix never sees it again. So when the EXIF arrives with no lens,
 * this has the backend read it from the file and write it into that cache
 * (`recover_lens_name` in `mods/lens_name.rs`), puts it into the EXIF the
 * panel and detection read, and only then detects.
 */

import { useEffect, useRef, useState } from 'react';
import { ag } from './ag';
import { useEditorStore } from '../store/useEditorStore';
import { useSettingsStore } from '../store/useSettingsStore';

/** Read the lens from the file, and put it into the EXIF if one is there. */
async function readLensFromFile(path: string) {
  const lens = await ag<string | null>('recover_lens_name', { path }).catch(() => null);
  if (!lens) {
    return;
  }
  useEditorStore.setState((state) => {
    const selected = state.selectedImage;
    if (selected?.path !== path) {
      return {};
    }
    return { selectedImage: { ...selected, exif: { ...selected.exif, LensModel: lens } } };
  });
}

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
  // The photo whose lens was asked of the file, and the one the answer is in for.
  const lensAskedFor = useRef<string | null>(null);
  const [lensReadFor, setLensReadFor] = useState<string | null>(null);

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

    // No lens in the EXIF: ask the file once, and come back when it answers.
    if (!selectedImage.exif.LensModel?.trim() && lensReadFor !== path) {
      if (lensAskedFor.current !== path) {
        lensAskedFor.current = path;
        readLensFromFile(path).finally(() => setLensReadFor(path));
      }
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
    lensReadFor,
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
