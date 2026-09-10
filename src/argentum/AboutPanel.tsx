/**
 * The About tab in Settings. Ours.
 *
 * WHY IT EXISTS
 *
 * The acknowledgements used to sit at the bottom of the General tab, below the
 * tag-clearing controls, where nobody would look for them — and they read as
 * RapidRAW's own credits, which is wrong in both directions now. Argentum owes
 * RapidRAW the entire application, and darktable the colour science it is being
 * built to harvest. Neither was said plainly.
 *
 * Only those two are named here. The long list of models and libraries that
 * came with RapidRAW is RapidRAW's to credit, and it does; repeating it under
 * Argentum's name would take credit for assembling something we inherited.
 *
 * WHAT IT COSTS THEM
 *
 * One line of `SettingsPanel.tsx` — an empty div — which this renders into
 * through a portal. See docs/ARCHITECTURE.md, "What the next feature costs".
 */

import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { useAgTranslation } from './locales';

interface Credit {
  name: string;
  href: string;
  what: string;
}

const BUILT_ON: Credit[] = [
  {
    name: 'RapidRAW',
    href: 'https://github.com/CyberTimon/RapidRAW',
    what:
      'By Timon Käch. Argentum is a fork of it. The interface, the GPU pipeline, '
      + 'the catalogue and very nearly all of the application are his work, along '
      + 'with the libraries and models he credits inside it.',
  },
  {
    name: 'darktable',
    href: 'https://github.com/darktable-org/darktable',
    what:
      'The reference Argentum is measured against, and where its colour science '
      + 'comes from — chromatic adaptation, and the raw-level behaviour its '
      + 'decoder gets right.',
  },
];

export default function AboutPanel() {
  const t = useAgTranslation();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  return (
    <div className="space-y-6 pb-6">
      <div className="p-6 bg-surface rounded-xl shadow-md">
        <h2 className="text-2xl font-semibold text-accent">Argentum</h2>
        {version && (
          <p className="mt-1 text-sm text-text-secondary">
            {t('aboutVersion')} {version}
          </p>
        )}
        <p className="mt-4 text-text-secondary">{t('aboutWhat')}</p>
        <p className="mt-3 text-text-secondary">{t('aboutWhy')}</p>
      </div>

      <div className="p-6 bg-surface rounded-xl shadow-md">
        <h3 className="text-lg font-semibold text-accent mb-2">{t('aboutBuiltOn')}</h3>
        <p className="mb-4 text-text-secondary">{t('aboutBuiltOnDesc')}</p>
        <ul className="space-y-3 list-disc ml-5 pl-1 text-text-secondary">
          {BUILT_ON.map((c) => (
            <li key={c.name}>
              <a
                href={c.href}
                target="_blank"
                rel="noopener noreferrer"
                className="font-semibold text-accent hover:underline"
              >
                {c.name}
              </a>
              : {c.what}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
