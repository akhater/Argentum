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
 * Anchors must be things upstream already renders for its own reasons. We do
 * not add anchors; that would just be the same problem with extra steps.
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
import { useTranslation } from 'react-i18next';
import { createPortal } from 'react-dom';
import './noCloud';
import './historyOnRelease';
import './history/historyKeeper';
import './looks/filmLookInput';
import RgbReadout from './RgbReadout';
import FocusOverlay from './FocusOverlay';
import { useFocusPoints } from './focusSwitch';
import LinearMask from './LinearMask';
import RadialFeather from './RadialFeather';
import RefreshMetadataButton from './RefreshMetadataButton';
import WhiteBalanceMenu from './WhiteBalanceMenu';
import WhiteBalanceUpgrade from './WhiteBalanceUpgrade';
import AboutPanel from './AboutPanel';
import ExportPrecision from './ExportPrecision';
import Sharpening from './Sharpening';
import LocalSharpening from './LocalSharpening';
import MyGear from './MyGear';
import PanelsSetting from './PanelsSetting';
import { useCompactSliders } from './compactSliders';
import { useEditorLayout } from './layout/editorLayout';
import { useModernLook } from './modern/look';
import AiModels from './AiModels';
import { registerArgentumTranslations, useAgTranslation } from './locales';
import { useTruncatedTooltips } from './useTruncatedTooltips';
import { useThresholdPreview } from './useThresholdPreview';
import { useAutoWhiteBalanceFollows } from './autoWhiteBalance';
import { useLensBuiltinOnOpen } from './lensBuiltin';
import { useNewPhotoDefaults } from './newPhotoDefaults';
import SuperResolutionModal from './SuperResolutionModal';

/**
 * Watch for a DOM element of theirs and hand it back once it exists.
 *
 * Their UI mounts, unmounts and re-renders as photos open and close, so this
 * cannot be a one-shot query. The observer re-resolves on every DOM change,
 * which is cheap next to what the editor is already doing.
 */
function useAnchor(selector: string) {
  const [anchor, setAnchor] = useState<Element | null>(null);

  useEffect(() => {
    const resolve = () => {
      const target = document.querySelector(selector);
      setAnchor((current) => (current === target ? current : target));
    };

    resolve();
    const observer = new MutationObserver(resolve);
    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [selector]);

  return anchor;
}

interface ArgentumProps {
  onLibraryRefresh: () => Promise<void>;
  onImageSelect: (path: string) => void;
}

export default function Argentum({ onLibraryRefresh, onImageSelect }: ArgentumProps) {
  // Their namespace, not ours: what is needed here is the label they already
  // print on the Whites and Blacks sliders, in whichever of the thirteen
  // languages is showing.
  const { t } = useTranslation();
  const agT = useAgTranslation();

  // Our strings live in our own i18next namespace, so a new one costs nothing
  // in their thirteen locale files. Registered here rather than at module
  // scope: i18next is only ready for it after its own init has run.
  useEffect(registerArgentumTranslations, []);

  // Full text on hover for anything the layout has cut short — a file name on a
  // library card, a folder path, a preset. Not a portal: it adds behaviour to
  // their existing tooltip rather than rendering anything of its own.
  useTruncatedTooltips();

  // Sliders on one line, Lightroom-style, with the row spacing chosen in
  // Settings > General > Panels. Read once at startup; the layout itself is a
  // stylesheet keyed on <html>.
  useEffect(() => {
    useCompactSliders.getState().load();
  }, []);

  // Whether the camera's focus points are drawn over the photo, as it was left.
  useEffect(() => {
    useFocusPoints.getState().load();
  }, []);

  // How the Modern window was left, its palette and its text size. Read once at startup.
  useEffect(() => {
    useEditorLayout.getState().load();
  }, []);

  // The Modern look: a stylesheet keyed on <html>.
  useModernLook();

  // Auto white balance measured again on each photo it is pasted onto.
  useAutoWhiteBalanceFollows();

  // The camera's built-in lens correction, filled into each photo's edit as it opens.
  useLensBuiltinOnOpen();
  // My Lenses' "Correct new photos": each lens joins as its photo opens.
  useNewPhotoDefaults();

  // Ctrl while dragging Whites or Blacks empties the picture and shows only
  // what is being blown or crushed, which is how those two are actually set.
  // Their own slider labels identify which slider, so their files stay untouched.
  useThresholdPreview({
    white: t('adjustments.basic.whites'),
    black: t('adjustments.basic.blacks'),
    hint: agT('thresholdHint'),
  });

  // The Camera Details heading in the metadata pane: an empty span of ours at
  // the end of it, because there was nothing stable to hang from and matching
  // on translated heading text would break in 12 of the 13 locales. Not the
  // heading itself — React warns when a portal shares an element with text.
  const cameraDetails = useAnchor('[data-argentum="camera-details"]');

  // The white balance row, first in Basic (components/adjustments/WhiteBalance.tsx):
  // the preset menu and Auto go in its slot.
  const whiteBalance = useAnchor('[data-argentum="white-balance"]');

  // The About tab in Settings, which is ours entirely — About, Roadmap and
  // Releases are sections inside it, so more of them cost their file nothing.
  const about = useAnchor('[data-argentum="about"]');

  // The My Gear tab in Settings: cameras and lenses, both filling themselves.
  const gear = useAnchor('[data-argentum="gear"]');

  // Settings > General, in the slot About and My Gear use: the No Cloud tile
  // and the Panels row.
  const general = useAnchor('[data-argentum="general"]');
  // The Modern window's Settings > AI and models: its AI card, then AI Models.
  const aiModels = useAnchor('[data-argentum="ai"]');

  // Under the format buttons in the export panel. Their file renders this marker
  // only while TIFF is selected, so the control appearing and disappearing costs
  // no state of ours and no polling.
  const exportPrecision = useAnchor('[data-argentum="export-precision"]');

  // Inside their Sharpening section in Details, first: our sharpening takes
  // the place of their two sliders there. The same marker in a mask's
  // Details, flagged data-mask, takes a mask's own Sharpen in place of their
  // local slider.
  const sharpening = useAnchor('[data-argentum="sharpening"]:not([data-mask])');
  const localSharpening = useAnchor('[data-argentum="sharpening"][data-mask]');

  return (
    <>
      {cameraDetails && createPortal(<RefreshMetadataButton />, cameraDetails)}
      {whiteBalance && createPortal(<WhiteBalanceMenu />, whiteBalance)}
      {about && createPortal(<AboutPanel />, about)}
      {exportPrecision && createPortal(<ExportPrecision />, exportPrecision)}
      {sharpening && createPortal(<Sharpening />, sharpening)}
      {localSharpening && createPortal(<LocalSharpening />, localSharpening)}
      {gear && createPortal(<MyGear />, gear)}
      {aiModels && createPortal(<AiModels />, aiModels)}
      {general && <PanelsSetting slot={general as HTMLElement} />}
      <RgbReadout />
      <FocusOverlay />
      <LinearMask />
      <RadialFeather />
      <WhiteBalanceUpgrade />
      <SuperResolutionModal onImageSelect={onImageSelect} onLibraryRefresh={onLibraryRefresh} />
    </>
  );
}
