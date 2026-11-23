import { describe, expect, it } from 'vitest';
import { KEY_REPEAT_ACTIONS, ignoreKeyRepeat } from './keyRepeat';

describe('ignoreKeyRepeat', () => {
  it('lets a first press through, whatever the action', () => {
    for (const action of ['paste_files', 'delete_selected', 'zoom_in', 'preview_next']) {
      expect(ignoreKeyRepeat({ repeat: false }, action)).toBe(false);
    }
  });

  it('drops the OS repeat of an action that runs once per press', () => {
    for (const action of ['paste_files', 'delete_selected', 'delete_rejected', 'toggle_pick', 'rotate_left']) {
      expect(ignoreKeyRepeat({ repeat: true }, action)).toBe(true);
    }
  });

  it('keeps driving the actions that are meant to repeat', () => {
    for (const action of ['preview_next', 'preview_prev', 'zoom_in', 'zoom_out', 'undo', 'redo', 'brush_size_up']) {
      expect(KEY_REPEAT_ACTIONS.has(action)).toBe(true);
      expect(ignoreKeyRepeat({ repeat: true }, action)).toBe(false);
    }
  });

  it('treats an action it does not know as once per press', () => {
    expect(ignoreKeyRepeat({ repeat: true }, 'something_new')).toBe(true);
  });
});
