/**
 * The camera's focus points over the photo, while Focus points is on. Ours.
 *
 * Drawn into the layer of the editor's overlay svg, as the mask guides are
 * (photoLayer.ts), so pan and zoom carry it; each point is placed through the
 * edit's turns, mirrors, straightening and crop (focusPlacement.ts). Not drawn
 * while the crop tool shows the whole picture, while Before shows the
 * original, or while Transform changes the perspective.
 */

import type { CSSProperties } from 'react';
import { createPortal } from 'react-dom';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { Panel } from '../components/ui/AppProperties';
import { FocusPoints, hasFocusPoints } from './FocusPoints';
import { usePhotoFocus, useFocusPoints } from './focusSwitch';
import { hiddenByTransform, placeOnPhoto } from './focusPlacement';
import { cropFrame } from './linearEdges';
import { usePhotoLayer } from './photoLayer';

export default function FocusOverlay() {
  const on = useFocusPoints((s) => s.on);
  const selectedImage = useEditorStore((s) => s.selectedImage);
  const adjustments = useEditorStore((s) => s.adjustments);
  const showOriginal = useEditorStore((s) => s.showOriginal);
  const activePanel = useUIStore((s) => s.activePanel);
  const focus = usePhotoFocus(on);
  const drawn =
    on && hasFocusPoints(focus) && !showOriginal && activePanel !== Panel.Crop && !hiddenByTransform(adjustments);
  const layout = usePhotoLayer(drawn);
  const frame = cropFrame();
  if (!drawn || !focus || !selectedImage || !layout?.svg.parentElement || !frame) {
    return null;
  }

  const turned = (adjustments.orientationSteps || 0) % 2 === 1;
  const picture = {
    width: turned ? selectedImage.height : selectedImage.width,
    height: turned ? selectedImage.width : selectedImage.height,
  };
  const points = placeOnPhoto(focus.points, adjustments, picture, frame, layout);

  return createPortal(
    <div
      style={
        {
          position: 'absolute',
          left: layout.left,
          top: layout.top,
          width: layout.width,
          height: layout.height,
          pointerEvents: 'none',
          zIndex: 5,
          '--af-zoom': layout.zoom || 1,
        } as CSSProperties
      }
    >
      <FocusPoints points={points} />
    </div>,
    layout.svg.parentElement,
  );
}
