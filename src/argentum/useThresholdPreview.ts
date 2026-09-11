/**
 * Ctrl while dragging Whites or Blacks empties the picture. Ours.
 *
 * WHAT IT IS FOR
 *
 * Setting a white point means pushing Whites until the brightest thing in the
 * picture is *just* about to lose detail, and one step further is too far. You
 * cannot see that on the picture — the difference between 250 and 255 is
 * nothing to look at. So the picture goes away and only what is being blown is
 * drawn, on black. Push until the first speckles appear, back off. Blacks is
 * the same on white.
 *
 * Lightroom holds Alt for this. Here Alt and Shift on a slider already mean
 * fine adjustment, so it is Ctrl.
 *
 * WHY IT DOES NOT TOUCH THE PHOTO'S ADJUSTMENTS
 *
 * `showClipping` is saved per photo. A view you hold for two seconds must not
 * be saved, must not mark the photo edited, and must not change the thumbnail —
 * thumbnail cache keys are built from the adjustments, so writing to them mid
 * drag would rebuild the library.
 *
 * `previewOverride` is theirs and is exactly this: adjustments used for
 * rendering only, never saved. Their before/after view already works that way.
 *
 * WHY IT WATCHES THE STORE WHILE IT IS UP
 *
 * The override is a *copy* of the adjustments, so it freezes the moment it is
 * set — and the whole point is to drag the slider while looking at it. So it
 * follows: every change to the real adjustments while the view is up is copied
 * across. Without that the mask shows where the clipping was when you pressed
 * the key, which is worse than useless.
 */

import { useEffect, useRef } from 'react';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';

/** Matches `mods/clipping.rs`. */
const WHITE_POINT = 5;
const BLACK_POINT = 6;

/**
 * Which slider the pointer went down on, by its visible label.
 *
 * There is no id on their sliders and adding one would be a line in their file
 * for every slider we ever care about. The label is already there, already
 * unique within the panel, and already translated — so the same translation
 * this app is showing the user is what gets compared, and it works in all
 * thirteen languages without a list of our own.
 */
function sliderUnder(target: EventTarget | null, labels: { white: string; black: string }): number | null {
    if (!(target instanceof HTMLElement)) {
        return null;
    }
    // Their slider is a labelled block; four levels is enough to get from the
    // rail or the thumb up to it without escaping into the panel.
    let el: HTMLElement | null = target;
    for (let depth = 0; el && depth < 5; depth += 1, el = el.parentElement) {
        const text = el.textContent ?? '';
        if (text.length > 40) {
            break; // Too much text to be one slider; we have gone too far up.
        }
        if (text.includes(labels.white)) {
            return WHITE_POINT;
        }
        if (text.includes(labels.black)) {
            return BLACK_POINT;
        }
    }
    return null;
}

export function useThresholdPreview(labels: { white: string; black: string; hint: string }) {
    // Which slider is under the pointer, and whether the preview is actually
    // up, are two different facts and were one variable.
    //
    // Pointer-down set the slider whether Ctrl was held or not, and the
    // subscriber that keeps the mask in step with the drag only checked that a
    // slider was set — so an ordinary drag of Whites blanked the picture. And
    // releasing Ctrl cleared the slider along with the preview, so pressing it
    // again in the same drag did nothing. Found by an audit that ran the hook
    // in a harness rather than by using it, which is the only way this shows up
    // without a mouse in hand.
    const slider = useRef<number | null>(null);
    const showing = useRef<number | null>(null);
    const dragging = useRef(false);
    const held = useRef(false);

    useEffect(() => {
        const store = useEditorStore.getState() as any;

        const show = (which: number) => {
            const state = useEditorStore.getState() as any;
            if (useUIStore.getState().activeView !== 'editor' || !state.selectedImage?.isReady) {
                return;
            }
            // Never on top of somebody else's override — the before/after view
            // uses the same field.
            if (showing.current === null && state.previewOverride) {
                return;
            }
            showing.current = which;
            state.setEditor({ previewOverride: { ...state.adjustments, showClipping: which } });
        };

        const hide = () => {
            if (showing.current === null) {
                return;
            }
            showing.current = null;
            (useEditorStore.getState() as any).setEditor({ previewOverride: null });
        };

        const update = () => {
            if (dragging.current && held.current && slider.current !== null) {
                show(slider.current);
            } else {
                hide();
            }
        };

        const onPointerDown = (e: PointerEvent) => {
            const which = sliderUnder(e.target, labels);
            if (which === null) {
                return;
            }
            // Remember the slider either way; only `update` decides whether
            // anything is shown.
            slider.current = which;
            dragging.current = true;
            held.current = e.ctrlKey || e.metaKey;
            update();
        };

        const onPointerUp = () => {
            dragging.current = false;
            slider.current = null;
            hide();
        };

        // Ctrl can be pressed and released mid drag, and holding it before the
        // drag starts is the more natural way round.
        const onKey = (e: KeyboardEvent) => {
            held.current = e.ctrlKey || e.metaKey;
            update();
        };

        // The adjustments move while the slider is dragged, and the override is
        // a copy — so it has to be rewritten each time or the mask is stale.
        const unsubscribe = useEditorStore.subscribe((s: any, prev: any) => {
            if (showing.current !== null && s.adjustments !== prev.adjustments) {
                show(showing.current);
            }
        });

        // Nobody discovers a held key on their own. The hint goes on the two
        // sliders it applies to, using their own global tooltip, and only while
        // the pointer is over one — so nothing is written into their DOM that
        // outlives the hover.
        let hinted: HTMLElement | null = null;
        const onPointerOver = (e: PointerEvent) => {
            if (hinted) {
                hinted.removeAttribute('data-tooltip');
                hinted = null;
            }
            const target = e.target;
            if (!(target instanceof HTMLElement) || sliderUnder(target, labels) === null) {
                return;
            }
            let el: HTMLElement | null = target;
            for (let depth = 0; el && depth < 5; depth += 1, el = el.parentElement) {
                const text = (el.textContent ?? '').trim();
                if (text !== labels.white && text !== labels.black) {
                    continue;
                }
                if (el.hasAttribute('data-tooltip')) {
                    return; // Theirs wins.
                }
                el.setAttribute('data-tooltip', labels.hint);
                hinted = el;
                return;
            }
        };

        window.addEventListener('pointerover', onPointerOver, true);
        window.addEventListener('pointerdown', onPointerDown, true);
        window.addEventListener('pointerup', onPointerUp, true);
        window.addEventListener('keydown', onKey, true);
        window.addEventListener('keyup', onKey, true);
        window.addEventListener('blur', onPointerUp);
        return () => {
            window.removeEventListener('pointerover', onPointerOver, true);
            window.removeEventListener('pointerdown', onPointerDown, true);
            if (hinted) {
                hinted.removeAttribute('data-tooltip');
            }
            window.removeEventListener('pointerup', onPointerUp, true);
            window.removeEventListener('keydown', onKey, true);
            window.removeEventListener('keyup', onKey, true);
            window.removeEventListener('blur', onPointerUp);
            unsubscribe();
            hide();
        };
        // `store` is read for its type only; the effect reads fresh state each time.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [labels.white, labels.black, labels.hint]);
}
