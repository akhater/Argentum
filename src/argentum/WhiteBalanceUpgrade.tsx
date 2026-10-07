/**
 * White balance saved before RapidRAW 1.6.5, converted. Ours, and draws nothing.
 *
 * 1.6.5's Temperature and Tint mean something different from the ones they
 * replaced, so an edit saved earlier would change colour. `mods/wb_legacy.rs`
 * does the conversion; this decides when, because the places their code reads
 * an edit — `load_metadata`, `get_all_adjustments_from_json` — are files with
 * no anchor allowance left, and an allowance does not go up for a feature.
 *
 * Two things, both through their own machinery:
 *
 * - **A folder sweep.** When the library lists a folder, Rust converts the old
 *   edits in it and saves them through their `save_metadata_and_update_thumbnail`,
 *   which keeps the rest of the sidecar and redraws the thumbnail, so the grid
 *   and any batch export see the converted edit. One at a time, each waiting for
 *   its thumbnail: the first version fired every save at once, each redraw
 *   decoded a whole RAW on a thread of its own, and on AK's first start the
 *   preview worker died under it. See wb_legacy::save_in_turn.
 * - **The open photo.** The editor may have loaded an old edit before the sweep
 *   reached it — session restore at launch, or a quick click. Their loader
 *   fills in `whiteBalance: null`, so the next save would store the old numbers
 *   as new ones and fix the wrong colour for good. So when the editor holds a
 *   photo whose saved edit is old, and its white balance numbers are still the
 *   old ones, only those numbers are replaced, as a fresh load (history reset)
 *   rather than an edit, and saved. Anything else already changed is kept.
 *   Rust remembers what it converted this session, so the answer is the same
 *   whether or not the sweep has saved the file yet.
 *
 * Each path is looked at once per session; a converted edit carries the
 * `whiteBalance` key and is never converted again.
 */

import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ag } from './ag';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { Invokes } from '../components/ui/AppProperties';
import { Adjustments } from '../utils/adjustments';

interface Numbers {
  temperature: number;
  tint: number;
}

interface Found {
  path: string;
  adjustments: Adjustments;
  was: Numbers & { masks: Numbers[] };
}

const same = (a: number | undefined, b: number) => (a ?? 0) === b;

/** The editor still shows the old white balance for this photo. */
const stillOld = (adjustments: Adjustments, was: Found['was']) =>
  adjustments.whiteBalance == null &&
  same(adjustments.temperature, was.temperature) &&
  same(adjustments.tint, was.tint) &&
  (adjustments.masks?.length ?? 0) === was.masks.length &&
  (adjustments.masks ?? []).every(
    (mask, i) => same(mask.adjustments?.temperature, was.masks[i].temperature) && same(mask.adjustments?.tint, was.masks[i].tint),
  );

/** The open edit with only its white balance numbers taken from the converted one. */
const withConvertedWhiteBalance = (open: Adjustments, converted: Adjustments): Adjustments => ({
  ...open,
  temperature: converted.temperature,
  tint: converted.tint,
  whiteBalance: converted.whiteBalance ?? null,
  masks: (open.masks ?? []).map((mask, i) => {
    const target = converted.masks?.[i]?.adjustments;
    return target
      ? { ...mask, adjustments: { ...mask.adjustments, temperature: target.temperature, tint: target.tint } }
      : mask;
  }),
});

const save = (path: string, adjustments: Adjustments) =>
  invoke(Invokes.SaveMetadataAndUpdateThumbnail, { path, adjustments }).catch((err) =>
    console.error('White balance conversion could not be saved:', path, err),
  );

/**
 * Correct the open photo once the editor has loaded it and still holds its old
 * numbers, which may be before or after this is called. Returns a cancel.
 */
const fixOpen = (found: Found): (() => void) => {
  const attempt = () => {
    const { selectedImage, adjustments, setEditor, resetHistory } = useEditorStore.getState();
    // isReady: their loader reads the edit before the image and only then marks
    // the photo ready, so until it is, the adjustments may still be the last
    // photo's, and saving them here would put them on this one.
    if (selectedImage?.path !== found.path || !selectedImage.isReady) return false;
    if (!stillOld(adjustments, found.was)) return false;
    const fixed = withConvertedWhiteBalance(adjustments, found.adjustments);
    setEditor({ adjustments: fixed });
    resetHistory(fixed);
    save(found.path, fixed);
    return true;
  };
  if (attempt()) return () => {};
  const unsubscribe = useEditorStore.subscribe(() => {
    if (attempt()) unsubscribe();
  });
  return unsubscribe;
};

export default function WhiteBalanceUpgrade() {
  const imageList = useLibraryStore((s) => s.imageList);
  const openPath = useEditorStore((s) => s.selectedImage?.path);
  const looked = useRef(new Set<string>());

  useEffect(() => {
    const paths = imageList.map((image) => image.path).filter((path) => !looked.current.has(path));
    if (paths.length === 0) return;
    paths.forEach((path) => looked.current.add(path));
    // Rust saves what it converts, one at a time, and leaves the open photo to
    // the editor.
    ag<Found[]>('upgrade_white_balance', { paths }).catch((err) =>
      console.error('White balance conversion failed:', err),
    );
  }, [imageList]);

  useEffect(() => {
    if (!openPath) return;
    let cancel = () => {};
    let stopped = false;
    ag<Found[]>('upgrade_white_balance', { paths: [openPath] })
      .then(([found]) => {
        if (found && !stopped) cancel = fixOpen(found);
      })
      .catch((err) => console.error('White balance conversion failed:', err));
    return () => {
      stopped = true;
      cancel();
    };
  }, [openPath]);

  return null;
}
