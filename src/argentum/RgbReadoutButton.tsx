/**
 * Toolbar toggle for the RGB readout. Ours.
 *
 * Sits in the top toolbar beside undo, redo and show-original, because that is
 * where the view-level controls are — it changes what you can see, not what the
 * picture is. It was first put over the image, where the container's own
 * mousedown handler swallowed the click and zoomed instead.
 *
 * Matches their toolbar button styling deliberately, so it does not read as a
 * bolted-on extra. State lives in `rgbReadoutStore` because the readout itself
 * renders over the image, in a different part of the tree.
 */

import { Pipette } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useRgbReadout } from './rgbReadoutStore';

export default function RgbReadoutButton() {
  const { t } = useTranslation();
  const on = useRgbReadout((s) => s.on);
  const toggle = useRgbReadout((s) => s.toggle);

  return (
    <button
      className={`p-2 rounded-full transition-colors ${
        on ? 'bg-accent text-button-text' : 'bg-surface text-text-primary hover:bg-card-active'
      }`}
      onClick={toggle}
      data-tooltip={t('editor.toolbar.tooltips.rgbReadout', 'Sample RGB under the cursor')}
    >
      <Pipette size={20} />
    </button>
  );
}
