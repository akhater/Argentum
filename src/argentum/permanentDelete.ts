/**
 * A delete the Recycle Bin could not take. Ours.
 *
 * The delete commands move files to the bin and never delete for good on their
 * own (mods/recycle.rs). When the bin refuses some files they come back as
 * "cannot_recycle", untouched, and this asks, with the app's confirm dialog,
 * before `delete_permanently` removes them. Cancel is the dialog's default.
 */

import { invoke } from '@tauri-apps/api/core';
import type { TFunction } from 'i18next';
import { useUIStore } from '../store/useUIStore';

/** What a delete command answers (mods/recycle.rs `DeleteOutcome`). */
export type DeleteOutcome = { status: 'done' } | { status: 'cannot_recycle'; files: string[]; recycled: number };

export type CannotRecycle = Extract<DeleteOutcome, { status: 'cannot_recycle' }>;

export function cannotRecycle(outcome: unknown): outcome is CannotRecycle {
  return (outcome as DeleteOutcome | undefined)?.status === 'cannot_recycle';
}

/**
 * Asks whether to delete `outcome.files` for good. `paths` are the photos or
 * folder the delete was for. `onDeleted` runs once, after they are gone;
 * `onFailed` hears why they were not.
 */
export function askPermanentDelete(
  t: TFunction,
  outcome: CannotRecycle,
  paths: string[],
  onDeleted: () => Promise<void> | void,
  onFailed: (err: unknown) => void,
): void {
  let asked = false;
  useUIStore.getState().setUI({
    confirmModalState: {
      confirmText: t('library.permanentDelete.confirm'),
      confirmVariant: 'destructive',
      focusCancel: true,
      isOpen: true,
      message: t('library.permanentDelete.message', { count: outcome.files.length }),
      onConfirm: () => {
        if (asked) return;
        asked = true;
        invoke('delete_permanently', { files: outcome.files, paths }).then(onDeleted, onFailed);
      },
      title: t('library.permanentDelete.title'),
    },
  });
}
