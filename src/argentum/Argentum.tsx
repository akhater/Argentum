/**
 * Argentum's entire UI, mounted from one place.
 *
 * THE POINT OF THIS FILE — read before adding UI anywhere else.
 *
 * A fork dies when your changes and upstream's land in the same lines of the
 * same files. The mergeability linter caps how much of their code we touch, but
 * a cap is not a solution: at one or two lines per feature, we run out. Six of
 * their files had already been edited before this existed.
 *
 * So instead of adding a tag to their component wherever we want something,
 * everything of ours renders through a **portal**, anchored to a DOM element
 * they already have. Their JSX never changes. The cost to upstream is one tag
 * in `App.tsx`, once, no matter how much we build afterwards.
 *
 * Anchors must be things upstream already renders for its own reasons —
 * `data-bench-id="undo"` is theirs, for benchmarking. We do not add anchors;
 * that would just be the same problem with extra steps.
 *
 * If an anchor disappears in an upstream update, the portal quietly renders
 * nothing and the app still runs. That is the right failure: a missing button
 * beats a merge conflict, and `useAnchor` keeps looking, so it reappears if the
 * element comes back.
 *
 * TO ADD UI: write the component, give it an anchor here. Do not touch a file
 * of theirs.
 */

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import RgbReadout from './RgbReadout';
import RgbReadoutButton from './RgbReadoutButton';

/**
 * Watch for a DOM element of theirs and hand it back once it exists.
 *
 * Their UI mounts, unmounts and re-renders as photos open and close, so this
 * cannot be a one-shot query. The observer re-resolves on every DOM change,
 * which is cheap next to what the editor is already doing.
 */
function useAnchor(selector: string, pick: (el: Element) => Element | null = (el) => el) {
  const [anchor, setAnchor] = useState<Element | null>(null);

  useEffect(() => {
    const resolve = () => {
      const found = document.querySelector(selector);
      const target = found ? pick(found) : null;
      setAnchor((current) => (current === target ? current : target));
    };

    resolve();
    const observer = new MutationObserver(resolve);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
    // `pick` is a literal at each call site; re-running on identity would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selector]);

  return anchor;
}

export default function Argentum() {
  // The toolbar's undo button, whose parent is the button row. Upstream tags it
  // for its own benchmarks, so it is a stable thing to hang from.
  const toolbar = useAnchor('[data-bench-id="undo"]', (el) => el.parentElement);

  return (
    <>
      {toolbar && createPortal(<RgbReadoutButton />, toolbar)}
      <RgbReadout />
    </>
  );
}
