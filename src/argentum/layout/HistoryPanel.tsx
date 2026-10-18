/**
 * The History tab of the RAWmakase arrangement. Ours.
 *
 * RAWmakase keeps History as a tab beside the adjustments (src/app/
 * side_tabs.rs, history_panel.rs). Today's editor has the same history as the
 * menu under its undo and redo buttons; this lists the same steps, named the
 * same way (useHistoryNames, from that toolbar), and a click goes to one
 * through today's own goToHistoryIndex. Nothing is kept here.
 */

import { useEffect, useRef } from 'react';
import clsx from 'clsx';
import Text from '../../components/ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../types/typography';
import { useHistoryNames } from '../../components/panel/editor/EditorToolbar';
import { useEditorStore } from '../../store/useEditorStore';
import { useAgTranslation } from '../locales';

export default function HistoryPanel() {
  const agT = useAgTranslation();
  const history = useEditorStore((s) => s.history);
  const index = useEditorStore((s) => s.historyIndex);
  const goToHistoryIndex = useEditorStore((s) => s.goToHistoryIndex);
  const names = useHistoryNames(history);
  const listRef = useRef<HTMLDivElement>(null);

  // The current step stays in view as steps are added below it.
  useEffect(() => {
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [index, names.length]);

  return (
    <div className="flex flex-col h-full">
      <div className="p-3 flex items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{agT('layoutHistory')}</Text>
      </div>
      <div ref={listRef} className="grow overflow-y-auto custom-scrollbar p-2 flex flex-col gap-0.5">
        {names.length === 0 ? (
          <Text color={TextColors.secondary} className="p-2">
            {agT('layoutHistoryEmpty')}
          </Text>
        ) : (
          names.map((name, i) => {
            const isCurrent = i === index;
            return (
              <button
                key={i}
                data-active={isCurrent}
                onClick={() => goToHistoryIndex(i)}
                className={clsx(
                  'flex justify-between items-center gap-2 text-left px-3 py-1.5 rounded-md transition-colors',
                  isCurrent
                    ? 'bg-accent'
                    : i > index
                      ? 'opacity-50 hover:bg-surface hover:opacity-100'
                      : 'hover:bg-surface',
                )}
              >
                <Text
                  as="span"
                  color={isCurrent ? TextColors.button : TextColors.primary}
                  weight={isCurrent ? TextWeights.medium : TextWeights.normal}
                  className="truncate"
                >
                  {name}
                </Text>
                <Text
                  as="span"
                  variant={TextVariants.small}
                  color={isCurrent ? TextColors.button : TextColors.secondary}
                  className="shrink-0"
                >
                  {i === 0 ? '' : i}
                </Text>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
