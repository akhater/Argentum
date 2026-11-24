/**
 * A pane that folds by its heading, and stays folded. Ours.
 *
 * For today's panels whose parts are a heading over their controls rather
 * than a CollapsibleSection, such as the Crop tab's Aspect Ratio,
 * Rotation, Orientation, Transform and Lens Correction. The heading is the
 * shared Text (heading variant) with a chevron in front; what was beside it, such as
 * the overlay and orientation buttons, stays beside it and does not fold the
 * pane. Which panes are folded is kept with the layout (editorLayout.ts
 * panels.folded), by an id such as "crop.rotation".
 */

import type { ReactNode } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import Text from '../components/ui/Text';
import { TextVariants } from '../types/typography';
import { useEditorLayout } from './layout/editorLayout';

interface FoldingPaneProps {
  id: string;
  title: ReactNode;
  /** Controls beside the heading, which do not fold the pane. */
  actions?: ReactNode;
  children: ReactNode;
}

export default function FoldingPane({ id, title, actions, children }: FoldingPaneProps) {
  const folded = useEditorLayout((s) => s.panels.folded.includes(id));
  const changePanels = useEditorLayout((s) => s.changePanels);

  const toggle = () => {
    const was = useEditorLayout.getState().panels.folded;
    changePanels({ folded: folded ? was.filter((pane) => pane !== id) : [...was, id] });
  };

  return (
    <div className="space-y-4">
      <Text
        variant={TextVariants.heading}
        className="mb-2 flex items-center gap-1.5 cursor-pointer select-none"
        onClick={toggle}
        aria-expanded={!folded}
      >
        {folded ? <ChevronRight size={14} className="shrink-0" /> : <ChevronDown size={14} className="shrink-0" />}
        <span className="flex-1 min-w-0">{title}</span>
        {actions && (
          <span className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
            {actions}
          </span>
        )}
      </Text>
      {!folded && children}
    </div>
  );
}
