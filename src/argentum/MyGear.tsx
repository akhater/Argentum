/**
 * My Gear: the cameras and lenses you actually shoot with. Ours.
 *
 * WHY IT IS A TAB
 *
 * Lenses used to be a card three screens down inside General, next to theme,
 * language, tagging and sidecar deletion. Cameras had nowhere at all — RapidRAW
 * never needed a list of bodies, because lens corrections are looked up by name
 * at the moment they are applied and nothing else cared. Camera profiles do
 * care: a profile is a file, obtained once, per body.
 *
 * So the two belong together, under their own heading, and General goes back to
 * being about the application rather than about equipment.
 *
 * A fifth tab fits because their tab row was pinned to a fixed 450px whatever
 * it contained — one word, `w-112.5` to `w-auto`, and it sizes to its tabs.
 *
 * BOTH LISTS FILL THEMSELVES
 *
 * A detected lens is added to My Lenses (see `useAutoDetectOnLoad`), and every
 * photo opened records its body. Neither should have to be typed in — the app
 * already knows, and asking the user to repeat it is asking them to do the
 * computer's job. Adding a lens by hand is still here for the case detection
 * cannot cover: a lens the camera does not report.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Trash2 } from 'lucide-react';
import { useSettingsStore } from '../store/useSettingsStore';
import { useAgTranslation } from './locales';
import MyCameras from './MyCameras';

interface Lens {
  maker: string;
  model: string;
}

function Card({ children }: { children: React.ReactNode }) {
  return <div className="p-6 bg-surface rounded-xl shadow-md">{children}</div>;
}

function Select({
  value,
  onChange,
  options,
  placeholder,
  disabled,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
  placeholder: string;
  disabled?: boolean;
}) {
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className="w-full px-3 py-2 rounded-md bg-bg-primary text-text-primary text-sm disabled:opacity-50"
    >
      <option value="">{placeholder}</option>
      {options.map((o) => (
        <option key={o} value={o}>
          {o}
        </option>
      ))}
    </select>
  );
}

function MyLenses() {
  const t = useAgTranslation();
  const appSettings = useSettingsStore((s) => s.appSettings) as any;
  const save = useSettingsStore((s) => s.handleSettingsChange);

  const [makers, setMakers] = useState<string[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [maker, setMaker] = useState('');
  const [model, setModel] = useState('');

  const lenses: Lens[] = useMemo(() => appSettings?.myLenses || [], [appSettings]);

  useEffect(() => {
    invoke<string[]>('get_lensfun_makers').then(setMakers).catch(() => setMakers([]));
  }, []);

  const pickMaker = useCallback((m: string) => {
    setMaker(m);
    setModel('');
    setModels([]);
    if (m) {
      invoke<string[]>('get_lensfun_lenses_for_maker', { maker: m })
        .then(setModels)
        .catch(() => setModels([]));
    }
  }, []);

  const add = () => {
    if (!maker || !model || !appSettings) {
      return;
    }
    if (lenses.some((l) => l.maker === maker && l.model === model)) {
      return;
    }
    const next = [...lenses, { maker, model }].sort(
      (a, b) => a.maker.localeCompare(b.maker) || a.model.localeCompare(b.model),
    );
    save({ ...appSettings, myLenses: next });
    setMaker('');
    setModel('');
    setModels([]);
  };

  const removeAt = (index: number) => {
    if (!appSettings) {
      return;
    }
    const next = [...lenses];
    next.splice(index, 1);
    save({ ...appSettings, myLenses: next });
  };

  return (
    <Card>
      <h2 className="text-lg font-semibold text-accent mb-2">{t('gearLenses')}</h2>
      <p className="mb-5 text-text-secondary text-sm">{t('gearLensesDesc')}</p>

      <div className="p-4 bg-bg-primary rounded-lg mb-5 space-y-3">
        <p className="text-sm font-medium text-text-primary">{t('gearAddLens')}</p>
        <Select
          value={maker}
          onChange={pickMaker}
          options={makers}
          placeholder={t('gearPickMaker')}
        />
        <Select
          value={model}
          onChange={setModel}
          options={models}
          placeholder={t('gearPickModel')}
          disabled={!maker}
        />
        <button
          onClick={add}
          disabled={!maker || !model}
          className="px-3 py-1.5 rounded-md bg-accent text-button-text text-sm font-medium disabled:opacity-40"
        >
          {t('gearAdd')}
        </button>
      </div>

      {lenses.length === 0 ? (
        <p className="italic text-text-secondary text-sm">{t('gearNoLenses')}</p>
      ) : (
        <div className="space-y-2">
          {lenses.map((lens, i) => (
            <div
              key={`${lens.maker}/${lens.model}`}
              className="flex items-center justify-between gap-3 p-3 bg-bg-primary rounded-lg"
            >
              <div className="min-w-0">
                <p className="font-medium text-text-primary truncate">{lens.model}</p>
                <p className="text-xs text-text-secondary truncate">{lens.maker}</p>
              </div>
              <button
                onClick={() => removeAt(i)}
                className="p-2 text-text-secondary hover:text-red-400 hover:bg-bg-secondary rounded-md transition-colors shrink-0"
                data-tooltip={t('gearRemoveLens')}
              >
                <Trash2 size={16} />
              </button>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

export default function MyGear() {
  return (
    <div className="space-y-6 pb-6 max-w-[78ch]">
      <MyCameras />
      <MyLenses />
    </div>
  );
}
