/**
 * The Profile section, first in the Edit panel: how the file is read, before
 * any slider touches it. Ours.
 *
 * The camera profile, the RAW rendering and highlight recovery were a RAW card
 * at the top of the Color section, below Basic, so the first thing to set on a
 * photo sat in the third section. The tone mapper joins them from Basic, where
 * it shared a card with Exposure.
 *
 * A section like the others, folding with them (focus mode included) and
 * remembered with them (`adjustmentLayout.openSections`, under "profile"). No
 * eye, since none of it can be turned off; its reset puts the camera profile,
 * the rendering and the tone mapper back (editSections.ts PROFILE_KEYS).
 * Highlight recovery is a preference of the app, not of the photo, so the reset
 * leaves it.
 */

import { useEffect } from 'react';
import clsx from 'clsx';
import { RotateCcw } from 'lucide-react';
import CollapsibleSection from '../components/ui/CollapsibleSection';
import { ToneMapperSwitch } from '../components/adjustments/Basic';
import type { Adjustments } from '../utils/adjustments';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import CameraProfile from './CameraProfile';
import HighlightRecovery from './HighlightRecovery';
import RawDecoder from './RawDecoder';
import RawToneRendering from './RawToneRendering';
import { initialValues, PROFILE_KEYS, rawToneRenderingOf } from './editSections';
import { useAgTranslation } from './locales';

/** Its key in their open sections. */
export const PROFILE_SECTION = 'profile';

interface ProfileSectionProps {
  adjustments: Adjustments;
  setAdjustments(update: (prev: Adjustments) => Adjustments): void;
  /** Their tone mapper override (Settings > Processing) fixes the tone mapper for every photo. */
  tonemapperOverride: boolean;
  onToggle(): void;
}

export default function ProfileSection({
  adjustments,
  setAdjustments,
  tonemapperOverride,
  onToggle,
}: ProfileSectionProps) {
  const t = useAgTranslation();
  // Their state names their five sections; this one is kept beside them.
  const isOpen = useUIStore(
    (s) => (s.collapsibleSectionsState as unknown as Record<string, boolean | undefined>)[PROFILE_SECTION],
  );
  const isRaw = useEditorStore((s) => s.selectedImage?.isRaw === true);
  // On a RAW, every rendering but Default replaces the tone mapper's result
  // (ag_stage_tone in shaders/modules.wgsl), so it is shown greyed then.
  const toneMapperApplies = !isRaw || rawToneRenderingOf(adjustments) === 'default';

  // Open until folded: their open sections start without this one, and their
  // toggle opens a section it finds missing.
  useEffect(() => {
    if (isOpen === undefined) {
      useUIStore.setState((s) => ({
        collapsibleSectionsState: { ...s.collapsibleSectionsState, [PROFILE_SECTION]: true },
      }));
    }
  }, [isOpen]);

  return (
    <div className="shrink-0 group relative">
      <CollapsibleSection
        canToggleVisibility={false}
        isContentVisible
        isOpen={isOpen !== false}
        onToggle={onToggle}
        title={t('profileSectionTitle')}
      >
        <div className="flex flex-col gap-2" data-ag-profile>
          <RawDecoder />
          <CameraProfile />
          <RawToneRendering />
          {!tonemapperOverride && (
            <div data-tooltip={toneMapperApplies ? undefined : t('toneMapperOnlyDefault')}>
              <div className={clsx(!toneMapperApplies && 'opacity-40')} inert={!toneMapperApplies}>
                <ToneMapperSwitch
                  selectedMapper={adjustments.toneMapper || 'agx'}
                  onMapperChange={(mapper) =>
                    setAdjustments((prev) => ({ ...prev, toneMapper: mapper as Adjustments['toneMapper'] }))
                  }
                />
              </div>
            </div>
          )}
          {isRaw && <HighlightRecovery />}
        </div>
      </CollapsibleSection>
      {/* Over the header's right end, as on their sections in the Modern look. */}
      <button
        className="ag-section-reset"
        onClick={() => setAdjustments((prev) => ({ ...prev, ...initialValues(PROFILE_KEYS) }))}
        data-tooltip={t('profileSectionReset')}
        aria-label={t('profileSectionReset')}
      >
        <RotateCcw size={13} />
      </button>
    </div>
  );
}
