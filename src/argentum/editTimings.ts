/**
 * The delays that decide when an edit is saved, when a History step is recorded and when the
 * History is written. Each is defined here once; the modules that use them import the name.
 *
 * Where one delay must come after another, the later one is derived from the earlier ones, so the
 * ordering is code. editTimings.test.ts asserts the orderings the code relies on.
 */

/**
 * After the last slider change, an idle timer waits this long before it applies the preview and
 * queues the edit for saving (useImageProcessing). Leaving the photo inside this window still
 * saves the change: the close flush queues it first (pendingSave.ts).
 */
export const EDIT_QUEUE_AFTER_MS = 50;

/** A queued edit is saved this long after the last queueing of it (debouncedSave, useEditorActions). */
export const EDIT_SAVE_AFTER_MS = 300;

/**
 * A change made without a drag becomes a History step this long after the last change
 * (historyOnRelease.ts). A drag records its step on release instead.
 */
export const HISTORY_STEP_AFTER_MS = 500;

/** The gap between the last of the edit's save and step and the write of the photo's History. */
const HISTORY_WRITE_GAP_MS = 100;

/**
 * The photo's History is written this long after the last change (historyKeeper.ts). It comes
 * after both the edit's save and the History step, so the two never write the sidecar at once.
 */
export const HISTORY_WRITE_AFTER_MS = Math.max(EDIT_SAVE_AFTER_MS, HISTORY_STEP_AFTER_MS) + HISTORY_WRITE_GAP_MS;

/** A preset change is saved this long after the last change (savePresetsSoon, usePresets). */
export const PRESETS_SAVE_AFTER_MS = 500;

/**
 * The longest the window waits for pending writes before it closes (saveOnClose.ts). A sidecar
 * write takes milliseconds; two seconds covers a slow or busy disk.
 */
export const CLOSE_WAIT_MS = 2000;
