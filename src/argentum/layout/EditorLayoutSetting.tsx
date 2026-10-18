/**
 * The Layout choice, in Settings > General. Ours.
 *
 * Placed from our side the way Compact panels is (CompactSlidersSetting.tsx),
 * straight under that row, since both change how the editor is laid out; their
 * settings file is not touched. If the Compact panels row is not there, the
 * choice goes in the slot at the end of the page, in a card of its own, rather
 * than nowhere. Their Dropdown, so the row looks like the ones around it.
 */

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import Dropdown from '../../components/ui/Dropdown';
import Text from '../../components/ui/Text';
import { TextVariants } from '../../types/typography';
import { useAgTranslation } from '../locales';
import { COMPACT_SLIDERS_ROW } from '../CompactSlidersSetting';
import { type EditorLayoutName, useEditorLayout } from './editorLayout';

function EditorLayoutRow() {
  const t = useAgTranslation();
  const layout = useEditorLayout((s) => s.layout);
  const choose = useEditorLayout((s) => s.choose);

  return (
    <div>
      <Text variant={TextVariants.heading} className="block mb-2">
        {t('layoutSetting')}
      </Text>
      <Dropdown<EditorLayoutName>
        onChange={(value) => choose(value)}
        options={[
          { value: 'classic', label: t('layoutClassic') },
          { value: 'rawmakase', label: t('layoutRawmakase') },
        ]}
        value={layout}
        disabled={layout === null}
        triggerClassName="bg-bg-primary"
      />
      <Text variant={TextVariants.small} className="mt-2">
        {t('layoutSettingDesc')}
      </Text>
    </div>
  );
}

export default function EditorLayoutSetting({ slot }: { slot: HTMLElement }) {
  const [host, setHost] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const container = slot.parentElement;
    if (!container) return;
    const own = document.createElement('div');

    const place = () => {
      const compact = container.querySelector(`[${COMPACT_SLIDERS_ROW}]`);
      if (compact && compact.parentElement !== slot) {
        // Inside their card, under Compact panels, spaced by the card.
        own.className = '';
        if (compact.nextElementSibling !== own) compact.after(own);
      } else if (own.parentElement !== slot) {
        own.className = 'mt-10 p-6 bg-surface rounded-xl shadow-md';
        slot.appendChild(own);
      }
      setHost(own);
    };

    place();
    const observer = new MutationObserver(place);
    observer.observe(container, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      own.remove();
    };
  }, [slot]);

  return host ? createPortal(<EditorLayoutRow />, host) : null;
}
