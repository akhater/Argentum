import { describe, expect, it, vi } from 'vitest';
import { KEY_REPEAT_ACTIONS, ignoreKeyRepeat } from './keyRepeat';

const press = (repeat: boolean) => ({ repeat, preventDefault: vi.fn() });

describe('ignoreKeyRepeat', () => {
  it('lets a first press through, whatever the action', () => {
    for (const action of ['paste_files', 'delete_selected', 'zoom_in', 'preview_next']) {
      expect(ignoreKeyRepeat(press(false), action)).toBe(false);
    }
  });

  it('drops the OS repeat of an action that runs once per press', () => {
    for (const action of ['paste_files', 'delete_selected', 'delete_rejected', 'toggle_pick', 'rotate_left']) {
      expect(ignoreKeyRepeat(press(true), action)).toBe(true);
    }
  });

  it('keeps driving the actions that are meant to repeat', () => {
    const actions = ['preview_next', 'preview_prev', 'zoom_in', 'zoom_out', 'undo', 'redo', 'brush_size_up'];
    for (const action of [...actions, 'library_arrows']) {
      expect(KEY_REPEAT_ACTIONS.has(action)).toBe(true);
      expect(ignoreKeyRepeat(press(true), action)).toBe(false);
    }
  });

  it('treats an action it does not know as once per press', () => {
    expect(ignoreKeyRepeat(press(true), 'something_new')).toBe(true);
  });

  it('still prevents the browser default of a dropped repeat (no scroll, select-all, download)', () => {
    const dropped = press(true);
    expect(ignoreKeyRepeat(dropped, 'select_all')).toBe(true);
    expect(dropped.preventDefault).toHaveBeenCalledTimes(1);
  });

  it('leaves the default alone when it lets the key through (the action decides)', () => {
    const through = press(true);
    expect(ignoreKeyRepeat(through, 'zoom_in')).toBe(false);
    expect(through.preventDefault).not.toHaveBeenCalled();
  });
});
