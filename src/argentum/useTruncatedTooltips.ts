/**
 * Show the full text of anything that has been cut short. Ours.
 *
 * A library card is narrower than most file names, so what you see is
 * "2026-09-06_Canon EOS 5D Mark II…" and the part that tells one frame from
 * another is the part that got cut. The same is true of a folder path, a preset
 * name, a lens — anywhere `truncate` is used, which upstream uses a great many
 * places.
 *
 * WHY THIS IS NOT A TOOLTIP ON THE FILE NAME
 *
 * Because then it would be a tooltip on the file name, and next week a tooltip
 * on the folder path, and each one is a line in a file of theirs. Their
 * `GlobalTooltip` already shows a tooltip for any element carrying a
 * `data-tooltip` attribute, wherever it is; it just has no way to know that a
 * particular element is clipped, because that is a fact about layout and only
 * true at the moment you point at it.
 *
 * So this fills that in. On the way in — capture phase, before their listener
 * on `document` runs — it looks at what the pointer is over, asks the browser
 * whether the text is actually clipped, and if it is, writes the full text into
 * `data-tooltip`. Their tooltip does the rest. Nothing of theirs changes, and
 * every clipped label in the app gets the behaviour at once, including ones
 * that do not exist yet.
 *
 * WHAT IT DELIBERATELY DOES NOT DO
 *
 * It never overrides a tooltip somebody wrote on purpose: if the element or
 * anything above it already has `data-tooltip`, that one is left alone. And it
 * only fires on text that is genuinely ellipsised — not on every container that
 * happens to overflow — so pointing at ordinary text produces nothing.
 */

import { useEffect } from 'react';

/** Ours, so cleanup removes only what we added. */
const MARK = 'data-ag-truncated';

/**
 * Is this element's text actually cut short right now?
 *
 * `scrollWidth > clientWidth` alone is not enough — plenty of elements overflow
 * without showing an ellipsis, and a tooltip on those would be noise. Requiring
 * `text-overflow: ellipsis` is what makes this mean "the user can see that
 * something is missing".
 */
function isClipped(el: HTMLElement): boolean {
  const style = window.getComputedStyle(el);
  if (style.textOverflow !== 'ellipsis') {
    return false;
  }
  // A pixel of slack: sub-pixel layout makes exact equality unreliable.
  return el.scrollWidth > el.clientWidth + 1;
}

export function useTruncatedTooltips() {
  useEffect(() => {
    let marked: HTMLElement | null = null;

    const clear = () => {
      if (marked) {
        marked.removeAttribute('data-tooltip');
        marked.removeAttribute(MARK);
        marked = null;
      }
    };

    const onOver = (e: MouseEvent) => {
      const target = e.target;
      if (!(target instanceof HTMLElement)) {
        return;
      }
      if (marked && marked !== target) {
        clear();
      }

      // Someone else's tooltip wins, wherever it is above us.
      if (target.closest('[data-tooltip]')) {
        return;
      }

      const text = target.textContent?.trim();
      if (!text || !isClipped(target)) {
        return;
      }

      target.setAttribute('data-tooltip', text);
      target.setAttribute(MARK, '');
      marked = target;
    };

    // Capture, so the attribute is in place before their `mouseover` listener
    // on `document` looks for it.
    document.addEventListener('mouseover', onOver, true);
    document.addEventListener('mouseout', clear, true);
    return () => {
      document.removeEventListener('mouseover', onOver, true);
      document.removeEventListener('mouseout', clear, true);
      clear();
    };
  }, []);
}
