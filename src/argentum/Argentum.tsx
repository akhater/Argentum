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
import RenderStatus from './RenderStatus';
import RefreshMetadataButton from './RefreshMetadataButton';
import AutoWhiteBalanceButton from './AutoWhiteBalanceButton';
import AboutPanel from './AboutPanel';
import CameraProfile from './CameraProfile';
import MyGear from './MyGear';
import { registerArgentumTranslations } from './locales';
import { useTruncatedTooltips } from './useTruncatedTooltips';

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
  // Our strings live in our own i18next namespace, so a new one costs nothing
  // in their thirteen locale files. Registered here rather than at module
  // scope: i18next is only ready for it after its own init has run.
  useEffect(registerArgentumTranslations, []);

  // Full text on hover for anything the layout has cut short — a file name on a
  // library card, a folder path, a preset. Not a portal: it adds behaviour to
  // their existing tooltip rather than rendering anything of its own.
  useTruncatedTooltips();

  // The toolbar's undo button, whose parent is the button row. Upstream tags it
  // for its own benchmarks, so it is a stable thing to hang from.
  const toolbar = useAnchor('[data-bench-id="undo"]', (el) => el.parentElement);

  // The Camera Details heading in the metadata pane. This is the one anchor we
  // added to a file of theirs — a bare data attribute, one line, because there
  // was nothing stable to hang from and matching on translated heading text
  // would break in 12 of the 13 locales.
  const cameraDetails = useAnchor('[data-argentum="camera-details"] > *:first-child');

  // The white balance row in the Color panel. Same reasoning as above, and the
  // slot every future Argentum colour control mounts into — which is the point
  // of a marker rather than a direct tag: the first one costs a line of theirs,
  // the next ten cost nothing.
  const colorTools = useAnchor('[data-argentum="color-tools"]');

  // The About tab in Settings, which is ours entirely — About, Roadmap and
  // Releases are sections inside it, so more of them cost their file nothing.
  const about = useAnchor('[data-argentum="about"]');

  // Below the white balance sliders: which camera profile this photo is using,
  // and an import when there is none.
  const cameraProfile = useAnchor('[data-argentum="camera-profile"]');

  // The My Gear tab in Settings: cameras and lenses, both filling themselves.
  const gear = useAnchor('[data-argentum="gear"]');

  return (
    <>
      {toolbar &&
        createPortal(
          <>
            <RenderStatus />
            <RgbReadoutButton />
          </>,
          toolbar,
        )}
      {cameraDetails && createPortal(<RefreshMetadataButton />, cameraDetails)}
      {colorTools && createPortal(<AutoWhiteBalanceButton />, colorTools)}
      {about && createPortal(<AboutPanel />, about)}
      {cameraProfile && createPortal(<CameraProfile />, cameraProfile)}
      {gear && createPortal(<MyGear />, gear)}
      <RgbReadout />
    </>
  );
}
