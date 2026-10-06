/**
 * Settings > Processing gets two tabs: Processing Engine and AI Models. Ours.
 *
 * AI models are part of processing — they are what the AI features run — so
 * that is where they are looked for. They were a section of About first.
 *
 * Their settings file is allowed one slot of ours, and it already exists for
 * About and My Gear; it now renders for Processing too, just before their
 * Processing page. While AI Models is picked, that page (the slot's next
 * sibling) is hidden rather than asking their file to know about tabs. It sets
 * no inline display of its own, so putting it back is exact. Their pages animate
 * in, so the page may arrive after the slot: an observer catches it.
 */

import { useEffect, useState } from 'react';
import clsx from 'clsx';
import AiModels from './AiModels';

type Tab = 'engine' | 'models';

const TABS: { id: Tab; label: string }[] = [
  { id: 'engine', label: 'Processing Engine' },
  { id: 'models', label: 'AI Models' },
];

export default function ProcessingTabs({ slot }: { slot: HTMLElement }) {
  const [tab, setTab] = useState<Tab>('engine');

  useEffect(() => {
    const container = slot.parentElement;
    if (!container) {
      return;
    }
    let page: HTMLElement | null = null;
    const apply = () => {
      const next = slot.nextElementSibling as HTMLElement | null;
      if (next !== page && page) {
        page.style.display = '';
      }
      page = next;
      if (page) {
        page.style.display = tab === 'models' ? 'none' : '';
      }
    };
    apply();
    const observer = new MutationObserver(apply);
    observer.observe(container, { childList: true });
    return () => {
      observer.disconnect();
      if (page) {
        page.style.display = '';
      }
    };
  }, [slot, tab]);

  return (
    <div className="mb-10">
      <div className="flex gap-1 p-1 bg-surface rounded-lg w-fit">
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={clsx(
              'px-4 py-1.5 rounded-md text-sm transition-colors',
              tab === t.id
                ? 'bg-accent text-button-text font-medium'
                : 'text-text-secondary hover:text-text-primary',
            )}
          >
            {t.label}
          </button>
        ))}
      </div>
      {tab === 'models' && (
        <div className="mt-5">
          <AiModels />
        </div>
      )}
    </div>
  );
}
