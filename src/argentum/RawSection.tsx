/**
 * The RAW card at the top of the Color panel. Ours.
 *
 * Camera profile, RAW tone rendering and highlight recovery all decide how the
 * file is read, before any slider touches it. They used to be three cards of
 * their own floating above White Balance, the only things in the panel without
 * a heading, and looked stray.
 *
 * Their AdjustmentSubSection, not a card of ours: it looks and folds like White
 * Balance under it, and remembers being folded in the same setting theirs use
 * (`adjustmentLayout.collapsedTools`). The id is not one of their tools, so
 * their Sections menu cannot hide or reorder it.
 *
 * That also keeps it out of their focus mode, which folds a tool's siblings
 * from their own list when it opens. So the card takes part from here: it
 * watches the setting, and when it opens it folds the Color tools, and when one
 * of them opens it folds itself.
 */

import { useEffect, useRef } from 'react';
import AdjustmentSubSection from '../components/adjustments/AdjustmentSubSection';
import { useSettingsStore } from '../store/useSettingsStore';
import { getAdjustmentSectionToolIds, withAdjustmentLayout } from '../utils/adjustments';
import CameraProfile from './CameraProfile';
import RawToneRendering from './RawToneRendering';
import HighlightRecovery from './HighlightRecovery';
import { useAgTranslation } from './locales';

const ID = 'argentumRaw';

function useFocusMode() {
  const collapsed = useSettingsStore((s) => s.appSettings?.adjustmentLayout?.collapsedTools);
  const before = useRef(collapsed);

  useEffect(() => {
    const prev = before.current ?? [];
    before.current = collapsed;
    const { appSettings, handleSettingsChange } = useSettingsStore.getState();
    if (!appSettings?.enableToolFocusMode || !collapsed) {
      return;
    }

    const colorTools = getAdjustmentSectionToolIds('color');
    const opened = prev.filter((id) => !collapsed.includes(id));
    let next = collapsed;
    if (opened.includes(ID)) {
      next = [...new Set([...collapsed, ...colorTools])];
    } else if (opened.some((id) => colorTools.includes(id)) && !collapsed.includes(ID)) {
      next = [...collapsed, ID];
    }
    // Our write changes the setting again, but opens nothing, so it stops here.
    if (next.length !== collapsed.length) {
      handleSettingsChange(withAdjustmentLayout(appSettings, { collapsedTools: next }));
    }
  }, [collapsed]);
}

export default function RawSection() {
  const t = useAgTranslation();
  useFocusMode();

  return (
    <AdjustmentSubSection id={ID} order={0} title={t('rawSectionTitle')}>
      <div className="flex flex-col gap-3">
        <CameraProfile />
        <RawToneRendering />
        <HighlightRecovery />
      </div>
    </AdjustmentSubSection>
  );
}
