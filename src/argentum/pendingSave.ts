// The open photo's edit is queued for saving by an idle timer in useImageProcessing (EDIT_QUEUE_AFTER_MS),
// then held by debouncedSave (EDIT_SAVE_AFTER_MS). Leaving the photo inside the idle wait used to lose
// the change: the flush found nothing queued and the timer was then cancelled. Anything about to leave the open
// photo (another photo, another folder, the library, closing the window) calls this first.
import { useEditorStore } from '../store/useEditorStore';
import { debouncedSave } from '../hooks/useEditorActions';
import type { Adjustments } from '../utils/adjustments';

/** What the timer last queued: the photo and the edit it was queued for. */
export interface QueuedEdit {
  path: string;
  adjustments: Adjustments;
}

/**
 * Queues the open photo's edit if it differs from the one last queued for that same photo, then
 * writes everything queued. The save carries the path the edit was made for, read before anything
 * else opens.
 */
export function flushPendingSave(queued: { current: QueuedEdit | null }): void {
  const { selectedImage, adjustments, previewOverride } = useEditorStore.getState();
  const last = queued.current;
  if (
    selectedImage?.isReady &&
    !previewOverride &&
    last?.path === selectedImage.path &&
    last.adjustments !== adjustments
  ) {
    debouncedSave(selectedImage.path, adjustments);
    queued.current = { path: selectedImage.path, adjustments };
  }
  debouncedSave.flush();
}
