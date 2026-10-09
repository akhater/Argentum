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
 * their Sections menu cannot hide or reorder it and focus mode leaves it alone.
 */

import AdjustmentSubSection from '../components/adjustments/AdjustmentSubSection';
import CameraProfile from './CameraProfile';
import RawToneRendering from './RawToneRendering';
import HighlightRecovery from './HighlightRecovery';
import { useAgTranslation } from './locales';

export default function RawSection() {
  const t = useAgTranslation();

  return (
    <AdjustmentSubSection id="argentumRaw" order={0} title={t('rawSectionTitle')}>
      <div className="flex flex-col gap-3">
        <CameraProfile />
        <RawToneRendering />
        <HighlightRecovery />
      </div>
    </AdjustmentSubSection>
  );
}
