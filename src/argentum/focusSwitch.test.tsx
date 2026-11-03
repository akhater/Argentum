// The Focus points switch and its button (argentum/focusSwitch.ts, FocusPointsButton.tsx).
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Focus } from './ag.generated';
import type { SelectedImage } from '../components/ui/AppProperties';

const saved: boolean[] = [];
let shown: () => Promise<boolean>;
let photoFocus: Focus | null = null;

vi.mock('./ag', () => ({
  ag: vi.fn(async (name: string, args: { on?: boolean }) => {
    if (name === 'focus_points_shown') return shown();
    if (name === 'set_focus_points_shown') saved.push(args.on!);
    if (name === 'focus_points') return photoFocus;
    return null;
  }),
}));
const toasts: string[] = [];
vi.mock('react-toastify', () => ({ toast: { info: (words: string) => toasts.push(words) } }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null) }));

const { useFocusPoints, unrecordedText } = await import('./focusSwitch');
const { default: FocusPointsButton, focusTip } = await import('./FocusPointsButton');
const { useEditorStore } = await import('../store/useEditorStore');

const manual: Focus = { mode: 'Manual focus', points: [] };
const focused: Focus = { mode: 'Single point', points: [{ x: 0.4, y: 0.4, w: 0.1, h: 0.1, state: 'inFocus' }] };
const settle = () => new Promise((done) => setTimeout(done, 0));

describe('the Focus points switch', () => {
  beforeEach(() => {
    saved.length = 0;
    toasts.length = 0;
    useFocusPoints.setState({ on: false });
    useEditorStore.setState({
      selectedImage: { path: `/photos/${Math.random()}.CR3`, isReady: true } as unknown as SelectedImage,
    });
  });

  it('starts as it was left, and off when that cannot be read', async () => {
    shown = async () => true;
    await useFocusPoints.getState().load();
    expect(useFocusPoints.getState().on).toBe(true);
    shown = () => Promise.reject(new Error('library not ready'));
    await useFocusPoints.getState().load();
    expect(useFocusPoints.getState().on).toBe(false);
  });

  it('is kept each time it is flipped', () => {
    useFocusPoints.getState().toggle();
    useFocusPoints.getState().toggle();
    expect(saved).toEqual([true, false]);
  });

  it('says so when switched on over a photo focused by hand', async () => {
    photoFocus = manual;
    useFocusPoints.getState().toggle();
    await settle();
    expect(toasts).toEqual([unrecordedText(manual)]);
  });

  it('says nothing over a photo with a point to draw', async () => {
    photoFocus = focused;
    useFocusPoints.getState().toggle();
    await settle();
    expect(toasts).toEqual([]);
  });
});

describe('the Focus points button', () => {
  it('is one icon, its words in the tooltip, the square filled when on', () => {
    for (const on of [false, true]) {
      Object.assign(useFocusPoints.getInitialState(), { on });
      const html = renderToStaticMarkup(<FocusPointsButton />);
      expect(html).toContain('class="agm-btn' + (on ? ' agm-on"' : '"'));
      expect(html).toContain(`aria-pressed="${on}"`);
      expect(html).toContain(`fill="${on ? 'currentColor' : 'none'}"`);
      expect(html).not.toMatch(/>[A-Za-z]/);
    }
    Object.assign(useFocusPoints.getInitialState(), { on: false });
  });

  it('tells what a click does, and why nothing is drawn on a photo without a point', () => {
    expect(focusTip(false, manual, false)).toBe('focusPointsTip');
    expect(focusTip(true, undefined, false)).toBe('focusPointsHideTip');
    expect(focusTip(true, focused, false)).toBe('focusPointsHideTip');
    expect(focusTip(true, manual, false)).toBe('focusPointsManualTip');
    expect(focusTip(true, null, false)).toBe('focusPointsUnrecordedTip');
    expect(focusTip(true, focused, true)).toBe('focusPointsTransformTip');
  });
});
