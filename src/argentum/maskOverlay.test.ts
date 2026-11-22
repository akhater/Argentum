import { beforeEach, describe, expect, it } from 'vitest';
import { useEditorStore } from '../store/useEditorStore';
import { KEYBIND_DEFINITIONS } from '../utils/keyboardUtils';
import { OVERLAY_SHOWN, afterAdjusting, afterSelecting, toggled, useMaskOverlay } from './maskOverlay';

const shown = () => useMaskOverlay.getState().shown;
const adjust = () => useMaskOverlay.getState().adjusted();
const pressO = () => useMaskOverlay.getState().toggle();
const select = (container: string | null, component: string | null = null) =>
  useEditorStore.setState({ activeMaskContainerId: container, activeMaskId: component });

describe('mask overlay', () => {
  beforeEach(() => {
    select('a');
    useMaskOverlay.setState(OVERLAY_SHOWN);
  });

  it('is shown until a slider moves, and stays hidden after', () => {
    expect(shown()).toBe(true);
    adjust();
    expect(shown()).toBe(false);
    adjust();
    expect(shown()).toBe(false);
  });

  it('comes back with O after adjusting', () => {
    adjust();
    pressO();
    expect(shown()).toBe(true);
    expect(useMaskOverlay.getState().hiddenByAdjusting).toBe(false);
  });

  it('comes back for another mask, or a component added or selected', () => {
    adjust();
    select('b');
    expect(shown()).toBe(true);
    adjust();
    select('b', 'b1');
    expect(shown()).toBe(true);
    adjust();
    select(null);
    expect(shown()).toBe(true);
  });

  it('stays the same mask after a change that is not a selection', () => {
    adjust();
    useEditorStore.setState({ isSliderDragging: true });
    useEditorStore.setState({ isSliderDragging: false });
    expect(shown()).toBe(false);
  });

  it('turned off with O stays off for the next mask, and adjusting does not claim it', () => {
    pressO();
    expect(shown()).toBe(false);
    adjust();
    select('b');
    expect(shown()).toBe(false);
    pressO();
    expect(shown()).toBe(true);
  });

  it('follows RAWmakase’s rules as plain functions', () => {
    const hidden = afterAdjusting(OVERLAY_SHOWN);
    expect(hidden).toEqual({ shown: false, hiddenByAdjusting: true });
    expect(afterSelecting(hidden)).toEqual(OVERLAY_SHOWN);
    const off = toggled(OVERLAY_SHOWN);
    expect(afterAdjusting(off)).toBe(off);
    expect(afterSelecting(off)).toBe(off);
  });

  it('is O in the shortcuts list, which no other shortcut takes', () => {
    const def = KEYBIND_DEFINITIONS.find((d) => d.action === 'toggle_mask_overlay');
    expect(def?.defaultCombo).toEqual(['KeyO']);
    expect(def?.section).toBe('view');
    const others = KEYBIND_DEFINITIONS.filter((d) => d !== def && d.defaultCombo.join('+') === 'KeyO');
    expect(others).toEqual([]);
  });
});
