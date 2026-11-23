import { invoke } from '@tauri-apps/api/core';
import { debouncedSave } from '../hooks/useEditorActions';
import { Invokes } from '../components/ui/AppProperties';
import { normalizeLoadedAdjustments, type Adjustments } from '../utils/adjustments';
import { savesSettled, trackSave } from './savesInFlight';
import { stillOpen } from './stillOpen';

/**
 * A result for the photo `path`, which an AI task started on: applies `update` to the open edit when
 * `path` is still open; otherwise to its saved edit on disk, so the photo the user left is not stuck
 * on a spinner and does not lose the answer, and the photo open now is not touched.
 */
export async function applyToPhoto(
  path: string,
  update: (prev: Adjustments) => Adjustments,
  setAdjustments: (update: (prev: Adjustments) => Adjustments) => void,
): Promise<void> {
  if (stillOpen(path)) return setAdjustments(update);
  try {
    // The edit of the photo just left is saved on a timer: let that write finish before reading it.
    debouncedSave.flush();
    await savesSettled();
    const meta = await invoke<{ adjustments?: Adjustments | null } | undefined>(Invokes.LoadMetadata, { path });
    // The user came back while that was read: the edit is open again, and the open one is the truth.
    if (stillOpen(path)) return setAdjustments(update);
    if (!meta?.adjustments) return;
    const adjustments = update(normalizeLoadedAdjustments(meta.adjustments));
    await trackSave(invoke(Invokes.SaveMetadataAndUpdateThumbnail, { path, adjustments }));
  } catch (error) {
    console.error('Could not save the late result for', path, error);
  }
}
