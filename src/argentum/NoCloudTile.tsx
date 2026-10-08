/**
 * The Cloud tile, hidden. Ours.
 *
 * RapidRAW 1.6.5 offers Cloud beside Built-in, AI Connector and AI-Free in
 * their Generative AI card. Argentum has no Cloud (see noCloud.ts), so the tile
 * goes. Commenting it out of their provider list, as 1.6.4 shipped it, would be
 * a line in their settings file, which is over its budget; so it is hidden from
 * here, through the slot their General page already renders for us.
 *
 * Found by its label, read from their own translations, so it matches in every
 * language. If it is ever not found, the tile shows, and choosing it says Cloud
 * is unsupported: noCloud.ts has already told their store so.
 */

import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

export default function NoCloudTile({ slot }: { slot: HTMLElement }) {
  const { t } = useTranslation();

  useEffect(() => {
    const container = slot.parentElement;
    if (!container) return;
    const label = t('settings.processing.ai.providers.cloud');

    const hide = () => {
      // Their General page, the element just before the slot once it arrives.
      const page = slot.previousElementSibling;
      if (!page) return;
      page.querySelectorAll('button').forEach((button) => {
        if (button.textContent?.trim() === label && button.style.display !== 'none') {
          button.style.display = 'none';
        }
      });
    };

    hide();
    const observer = new MutationObserver(hide);
    observer.observe(container, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [slot, t]);

  return null;
}
