/**
 * AI Models, placed straight under their Generative AI card. Ours.
 *
 * RapidRAW 1.6.5 moved its AI card, which chooses how the AI features run, from
 * Settings > Processing to General. AI Models belongs beside it: one card says
 * how the features run, the other what they have downloaded.
 *
 * Their settings file allows Argentum one line, the slot About and My Gear
 * fill, and it renders on General too. That slot comes after their General
 * page, so on its own the card would land at the bottom, under Tagging. This
 * puts it under their AI card instead, from our side: a container of our own
 * is inserted after that card and AI Models is portalled into it. Their page
 * animates in after the slot appears, so an observer waits for it.
 *
 * Their card is found by its title, read from their own translations, so it
 * matches in every language. If it is ever not found — renamed, moved — the
 * card goes in the slot at the end of the page rather than nowhere.
 */

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import AiModels from './AiModels';

export default function AiModelsPlacement({ slot }: { slot: HTMLElement }) {
  const { t } = useTranslation();
  const [host, setHost] = useState<HTMLElement | null>(null);

  useEffect(() => {
    const container = slot.parentElement;
    if (!container) return;
    const title = t('settings.processing.ai.title');
    const own = document.createElement('div');

    const place = () => {
      // Their General page is the element just before the slot, while it is
      // showing. Until it arrives there is nothing to place against.
      const page = slot.previousElementSibling;
      if (!page) return;
      const card = Array.from(page.children).find(
        (child) => child.firstElementChild?.textContent?.trim() === title,
      );
      if (card) {
        // Their page spaces its cards itself.
        own.className = '';
        if (card.nextElementSibling !== own) card.after(own);
      } else if (own.parentElement !== slot) {
        // Outside their page, so it needs the gap their cards have.
        own.className = 'mt-10';
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
  }, [slot, t]);

  return host ? createPortal(<AiModels />, host) : null;
}
