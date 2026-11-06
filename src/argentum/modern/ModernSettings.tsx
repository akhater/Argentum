/**
 * Settings in the Modern window: a page in the centre, opened by the gear or
 * Ctrl+, and closed by Esc, the ✕ or the gear again. Ours.
 *
 * The section list is ours; each section is today's Settings page
 * (SettingsPanel.tsx) showing that part. AI and models, My Gear and About have
 * a section each, so they are always one click from the gear. The folder column
 * and the tools stay where they are while it is open.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Camera, Cpu, Info, Keyboard, SlidersHorizontal, Sparkles, X, type LucideIcon } from 'lucide-react';
import clsx from 'clsx';
import SettingsPanel from '../../components/panel/SettingsPanel';
import type { AppSettings } from '../../components/ui/AppProperties';
import { useUIStore } from '../../store/useUIStore';
import { useAgTranslation, type AgKey } from '../locales';

export interface SettingsProps {
  appSettings: AppSettings | null;
  onLibraryRefresh(): void;
  onSettingsChange(settings: AppSettings): Promise<void>;
}

const SECTIONS: { id: string; icon: LucideIcon; theirs?: string; ours?: AgKey }[] = [
  { id: 'general', icon: SlidersHorizontal, theirs: 'settings.categories.general' },
  { id: 'ai', icon: Sparkles, ours: 'mwSettingsAi' },
  { id: 'gear', icon: Camera, ours: 'mwSettingsGear' },
  { id: 'processing', icon: Cpu, theirs: 'settings.categories.processing' },
  { id: 'shortcuts', icon: Keyboard, theirs: 'settings.categories.shortcuts' },
  { id: 'about', icon: Info, ours: 'mwSettingsAbout' },
];

export default function ModernSettings(props: SettingsProps) {
  const { t } = useTranslation();
  const agT = useAgTranslation();
  const setUI = useUIStore((s) => s.setUI);
  const [section, setSection] = useState('general');
  const close = () => setUI({ isSettingsOpen: false });

  return (
    <div className="agm-settings">
      <nav className="agm-settings-nav">
        <div className="agm-settings-title">
          {agT('mwSettingsCrumb')}
          <button className="agm-btn" onClick={close} data-tooltip={agT('mwSettingsClose')}>
            <X size={16} />
          </button>
        </div>
        {SECTIONS.map((entry) => (
          <button
            key={entry.id}
            className={clsx('agm-settings-item', section === entry.id && 'agm-on')}
            onClick={() => setSection(entry.id)}
          >
            <entry.icon size={15} />
            {entry.ours ? agT(entry.ours) : t(entry.theirs as never)}
          </button>
        ))}
        <div className="agm-settings-foot">{agT('mwSettingsKeys')}</div>
      </nav>
      <div className="agm-settings-body custom-scrollbar">
        <SettingsPanel {...props} modernCategory={section} />
      </div>
    </div>
  );
}
