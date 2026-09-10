/**
 * The About tab in Settings. Ours, all of it.
 *
 * WHY ONE TAB WITH SECTIONS
 *
 * About, Roadmap and Releases were briefly three things; two of them as tabs in
 * their settings header. That header has a fixed width and five tabs did not fit
 * — the fifth rendered clipped, as "Chang…". Their layout is not ours to
 * rebuild, so the fix is to stop asking it for more room: one tab of theirs, and
 * our own switcher inside it.
 *
 * It is also the cheaper shape. A future Argentum section is a line in
 * `SECTIONS` below, not another tab competing for their header.
 *
 * WHAT IT COSTS THEM
 *
 * One line of `SettingsPanel.tsx` — an empty div this renders into through a
 * portal. See docs/ARCHITECTURE.md, "What the next feature costs".
 */

import { useEffect, useRef, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import clsx from 'clsx';
import { useAgTranslation } from './locales';
import { ROADMAP, Stage } from './roadmap';
import { KNOWN_ISSUES } from './knownIssues';
import { CREDITS } from './credits';
import { RELEASES } from './releases';

type Section = 'about' | 'credits' | 'roadmap' | 'issues' | 'releases';

const SECTIONS: { id: Section; label: string }[] = [
  { id: 'about', label: 'About' },
  { id: 'credits', label: 'Credits' },
  { id: 'roadmap', label: 'Roadmap' },
  { id: 'issues', label: 'Known issues' },
  { id: 'releases', label: 'Releases' },
];

const STAGE_LABEL: Record<Stage, string> = {
  done: 'Done',
  building: 'Building',
  planned: 'Planned',
};

const STAGE_STYLE: Record<Stage, string> = {
  done: 'bg-accent/15 text-accent',
  building: 'bg-yellow-500/15 text-yellow-500',
  planned: 'bg-text-secondary/10 text-text-secondary',
};

/**
 * A settings card, with the line length capped.
 *
 * Their panels stretch to the window, which is right for rows of controls and
 * wrong for prose: on a wide monitor the changelog ran ~250 characters a line
 * and was unreadable. 78ch is about a printed column.
 */
function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="p-6 bg-surface rounded-xl shadow-md max-w-[78ch]">{children}</div>
  );
}

type Translate = ReturnType<typeof useAgTranslation>;

function AboutSection({ version, t }: { version: string | null; t: Translate }) {
  return (
    <div className="space-y-6">
      <Card>
        <h2 className="text-2xl font-semibold text-accent">Argentum</h2>
        {version && (
          <p className="mt-1 text-sm text-text-secondary">
            {t('aboutVersion')} {version}
          </p>
        )}
        <p className="mt-4 text-text-secondary leading-relaxed">{t('aboutWhat')}</p>
        <p className="mt-3 text-text-secondary leading-relaxed">{t('aboutWhy')}</p>
      </Card>

      <Card>
        <h3 className="text-lg font-semibold text-accent mb-2">{t('aboutMergeable')}</h3>
        <p className="text-text-secondary leading-relaxed">{t('aboutMergeableDesc')}</p>
        <p className="mt-3 text-text-secondary leading-relaxed">{t('aboutMergeableDesc2')}</p>
      </Card>
    </div>
  );
}

function CreditsSection() {
  return (
    <div className="space-y-6">
      {CREDITS.map((group) => (
        <Card key={group.heading}>
          <h3 className="text-lg font-semibold text-accent mb-2">{group.heading}</h3>
          {group.blurb && <p className="mb-4 text-text-secondary">{group.blurb}</p>}
          <ul className="space-y-4">
            {group.entries.map((c) => (
              <li key={c.name}>
                <a
                  href={c.href}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="font-semibold text-accent hover:underline"
                >
                  {c.name}
                </a>
                <p className="mt-1 text-text-secondary leading-relaxed">{c.what}</p>
              </li>
            ))}
          </ul>
        </Card>
      ))}
    </div>
  );
}

function RoadmapSection({ t }: { t: Translate }) {
  return (
    <Card>
      <h3 className="text-lg font-semibold text-accent mb-2">Roadmap</h3>
      <p className="mb-5 text-text-secondary">{t('roadmapDesc')}</p>
      <ul className="space-y-4">
        {ROADMAP.map((m) => (
          <li key={m.what} className="flex gap-3">
            <span
              className={clsx(
                'shrink-0 mt-0.5 px-2 py-0.5 rounded text-xs font-medium h-fit w-20 text-center',
                STAGE_STYLE[m.stage],
              )}
            >
              {STAGE_LABEL[m.stage]}
            </span>
            <span>
              <span className="font-semibold text-text-primary">{m.what}</span>
              {m.release && (
                <span className="ml-2 text-xs text-text-secondary">{m.release}</span>
              )}
              <p className="mt-0.5 text-text-secondary leading-relaxed">{m.why}</p>
            </span>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function KnownIssuesSection({ t }: { t: Translate }) {
  return (
    <Card>
      <h3 className="text-lg font-semibold text-accent mb-2">Known issues</h3>
      <p className="mb-5 text-text-secondary">{t('knownIssuesDesc')}</p>
      <ul className="space-y-4">
        {KNOWN_ISSUES.map((issue) => (
          <li key={issue.what}>
            <span className="font-semibold text-text-primary">{issue.what}</span>
            {issue.inherited && (
              <span className="ml-2 px-2 py-0.5 rounded text-xs bg-text-secondary/10 text-text-secondary">
                inherited
              </span>
            )}
            <p className="mt-0.5 text-text-secondary leading-relaxed">{issue.detail}</p>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function ReleasesSection({ t }: { t: Translate }) {
  return (
    <Card>
      <h3 className="text-lg font-semibold text-accent mb-2">Releases</h3>
      <p className="mb-5 text-text-secondary">{t('releasesDesc')}</p>
      <ul className="space-y-5">
        {RELEASES.map((r) => (
          <li key={r.version}>
            <div className="flex items-baseline gap-3">
              <span className="font-semibold text-text-primary">{r.version}</span>
              <span className="text-xs text-text-secondary">{r.date}</span>
            </div>
            <ul className="mt-1 space-y-1 list-disc ml-5 pl-1">
              {r.notes.map((note) => (
                <li key={note} className="text-text-secondary leading-relaxed">
                  {note}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
    </Card>
  );
}

export default function AboutPanel() {
  const t = useAgTranslation();
  const root = useRef<HTMLDivElement>(null);
  const [section, setSection] = useState<Section>('about');
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  // Their settings pane keeps its scroll position across a section change, so
  // switching to Releases — which is long — landed halfway down it with the
  // switcher scrolled off the top. Go back to the start of whatever was picked.
  useEffect(() => {
    let el = root.current?.parentElement;
    while (el && el.scrollHeight <= el.clientHeight) {
      el = el.parentElement;
    }
    el?.scrollTo({ top: 0 });
  }, [section]);

  return (
    <div className="pb-6" ref={root}>
      {/* Sticky, so the switcher is still reachable partway down a long release list. */}
      <div className="sticky top-0 z-10 flex gap-1 mb-5 p-1 bg-surface rounded-lg w-fit">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            onClick={() => setSection(s.id)}
            className={clsx(
              'px-4 py-1.5 rounded-md text-sm transition-colors',
              section === s.id
                ? 'bg-accent text-button-text font-medium'
                : 'text-text-secondary hover:text-text-primary',
            )}
          >
            {s.label}
          </button>
        ))}
      </div>

      {section === 'about' && <AboutSection version={version} t={t} />}
      {section === 'credits' && <CreditsSection />}
      {section === 'roadmap' && <RoadmapSection t={t} />}
      {section === 'issues' && <KnownIssuesSection t={t} />}
      {section === 'releases' && <ReleasesSection t={t} />}

    </div>
  );
}
