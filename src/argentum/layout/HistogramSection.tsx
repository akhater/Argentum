/**
 * The Histogram, at the top of the RAWmakase arrangement's right column. Ours.
 *
 * Today's own analytics panel (Waveform: luma, RGB, parade, vectorscope,
 * histogram, and the clipping warning), fed from the same store values and
 * through the same controls as the Adjustments and Masks panels feed it. Open
 * or folded is today's isWaveformVisible: the same switch as their chart
 * button and its keyboard shortcut, saved where it always was, and what tells
 * the renderer to compute the waveform at all. Those panels leave their own
 * copy out in this arrangement (their showWaveform prop), so there is one.
 */

import { useShallow } from 'zustand/react/shallow';
import CollapsibleSection from '../../components/ui/CollapsibleSection';
import Resizer from '../../components/ui/Resizer';
import Waveform from '../../components/panel/editor/Waveform';
import { Orientation } from '../../components/ui/AppProperties';
import { useEditorStore } from '../../store/useEditorStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useEditorActions } from '../../hooks/useEditorActions';
import { useWaveformControls } from '../../hooks/useWaveformControls';
import type { Adjustments } from '../../utils/adjustments';
import { useAgTranslation } from '../locales';

export default function HistogramSection() {
  const agT = useAgTranslation();
  const theme = useSettingsStore((s) => s.theme);
  const { setAdjustments } = useEditorActions();
  const { onToggleWaveform, setActiveWaveformChannel, handleWaveformResize } = useWaveformControls();
  const { isWaveformVisible, waveform, histogram, activeWaveformChannel, waveformHeight, showClipping } =
    useEditorStore(
      useShallow((s) => ({
        isWaveformVisible: s.isWaveformVisible,
        waveform: s.waveform,
        histogram: s.histogram,
        activeWaveformChannel: s.activeWaveformChannel,
        waveformHeight: s.waveformHeight,
        showClipping: s.adjustments.showClipping,
      })),
    );

  return (
    <CollapsibleSection
      canToggleVisibility={false}
      isContentVisible
      isOpen={isWaveformVisible}
      onToggle={onToggleWaveform}
      title={agT('layoutHistogram')}
    >
      <div style={{ height: waveformHeight || 200 }}>
        <Waveform
          waveformData={waveform || null}
          histogram={histogram}
          displayMode={activeWaveformChannel || 'luma'}
          setDisplayMode={setActiveWaveformChannel}
          showClipping={showClipping || false}
          onToggleClipping={() =>
            // As the Adjustments panel cycles it: off / all / R / G / B.
            setAdjustments((prev: Adjustments) => ({
              ...prev,
              showClipping: ((Number(prev.showClipping) || 0) + 1) % 5,
            }))
          }
          theme={theme}
        />
      </div>
      <Resizer direction={Orientation.Horizontal} onMouseDown={handleWaveformResize} />
    </CollapsibleSection>
  );
}
