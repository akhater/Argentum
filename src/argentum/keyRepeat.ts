/**
 * Which shortcuts may repeat while a key is held. Ours.
 *
 * Holding a key makes the OS send keydown again and again (`event.repeat`). An
 * action meant to run once per press (delete, paste files, a toggle) must not
 * start again for each of those; one meant to follow the key (step to the next
 * photo, zoom, undo) must. An action not named here is once per press.
 *
 * Repeatable, by action id of `utils/keyboardUtils` KEYBIND_DEFINITIONS:
 * preview_prev, preview_next (step through photos), zoom_in, zoom_out,
 * zoom_in_step, zoom_out_step (zoom), undo, redo (walk the history),
 * brush_size_up, brush_size_down. The arrow keys of the library grid are the
 * same kind (see `builtinShortcuts`). Everything else is once per press:
 * delete_selected, delete_rejected, paste_files, copy_files, select_all, the
 * ratings, flags and colour labels, rotate_left, rotate_right, auto_straighten,
 * every toggle_* and show_original, cycle_zoom, zoom_fit, zoom_100, open_image,
 * open_settings, focus_search, toggle_crop, and the copy/paste of adjustments.
 */

export const KEY_REPEAT_ACTIONS: ReadonlySet<string> = new Set([
  'preview_prev',
  'preview_next',
  'zoom_in',
  'zoom_out',
  'zoom_in_step',
  'zoom_out_step',
  'undo',
  'redo',
  'brush_size_up',
  'brush_size_down',
]);

/** True for the OS repeat of a key whose action runs once per press. */
export function ignoreKeyRepeat(event: { repeat: boolean }, action: string): boolean {
  return event.repeat && !KEY_REPEAT_ACTIONS.has(action);
}
