/**
 * My Cameras, in the My Gear tab. Ours.
 *
 * This manages the *library*: which cameras you shoot with, and which profiles
 * you keep for each. Which profile a photo uses is chosen per photo, in the
 * Color panel, because that is where you can see what it does.
 *
 * The split matters, and getting it wrong is what this went through. A "Use it"
 * checkbox here meant the choice was per camera, so trying a profile on one
 * photo silently changed every photo from that body. And the only delete button
 * was on the camera — the one thing on the row you do not own. What you own is
 * profiles, so that is what has a bin next to it.
 *
 * RapidRAW keeps a list of lenses and nothing for cameras, because nothing
 * needed one: lens corrections are looked up by name when they are applied. A
 * profile is a file that has to be obtained, so it needs somewhere to live and
 * something to list it.
 */

import { useCallback, useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { Trash2 } from 'lucide-react';
import { ag } from './ag';
import { useAgTranslation } from './locales';

interface Camera {
  make: string;
  model: string;
}

interface Installed {
  file: string;
  name: string | null;
}

interface ForCamera {
  profiles: Installed[];
  /** RawTherapee's own file for this body is already here. */
  publishedInstalled: boolean;
}

export default function MyCameras() {
  const t = useAgTranslation();
  const [cameras, setCameras] = useState<Camera[]>([]);
  const [profiles, setProfiles] = useState<Record<string, ForCamera>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<{ model: string; text: string } | null>(null);

  const refresh = useCallback(async () => {
    let listed: Camera[] = [];
    try {
      listed = await ag<Camera[]>('list_cameras');
    } catch {
      setCameras([]);
      return;
    }
    setCameras(listed);

    const found: Record<string, ForCamera> = {};
    await Promise.all(
      listed.map(async (c) => {
        try {
          found[c.model] = await ag<ForCamera>('profiles_for_camera', {
            make: c.make,
            model: c.model,
          });
        } catch {
          found[c.model] = { profiles: [], publishedInstalled: false };
        }
      }),
    );
    setProfiles(found);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const findOnline = async (c: Camera) => {
    setNote(null);
    setBusy(c.model);
    try {
      const file = await ag<string | null>('get_profile_online', {
        make: c.make,
        model: c.model,
      });
      await refresh();
      if (!file) {
        setNote({ model: c.model, text: t('gearNotPublished') });
      }
    } catch (e) {
      setNote({ model: c.model, text: String(e) });
    } finally {
      setBusy(null);
    }
  };

  const addFile = async (c: Camera) => {
    setNote(null);
    const picked = await open({
      multiple: false,
      filters: [{ name: 'Camera profile', extensions: ['dcp'] }],
    });
    if (typeof picked !== 'string') {
      return;
    }
    setBusy(c.model);
    try {
      const added = await ag<{ camera: string | null }>('import_camera_profile', { path: picked });
      await refresh();
      if (!added.camera) {
        setNote({ model: c.model, text: t('profileNoCamera') });
      }
    } catch (e) {
      setNote({ model: c.model, text: String(e) });
    } finally {
      setBusy(null);
    }
  };

  const removeProfile = async (file: string) => {
    await ag('remove_camera_profile', { file }).catch(() => {});
    refresh();
  };

  const forgetCamera = async (model: string) => {
    await ag('forget_camera', { model }).catch(() => {});
    refresh();
  };

  return (
    <div className="p-6 bg-surface rounded-xl shadow-md">
      <h2 className="text-lg font-semibold text-accent mb-2">{t('gearCameras')}</h2>
      <p className="mb-5 text-text-secondary text-sm">{t('gearCamerasDesc')}</p>

      {cameras.length === 0 ? (
        <p className="italic text-text-secondary text-sm">{t('gearNoCameras')}</p>
      ) : (
        <div className="space-y-3">
          {cameras.map((c) => (
            <div key={c.model} className="p-3 bg-bg-primary rounded-lg">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <p className="font-medium text-text-primary truncate">{c.model}</p>
                  <p className="text-xs text-text-secondary truncate">{c.make}</p>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  {/*
                    RawTherapee publishes one profile per camera, so once it is
                    here there is nothing left to find — the button would fetch
                    the same bytes and save them beside themselves.
                  */}
                  {!profiles[c.model]?.publishedInstalled && (
                    <button
                      onClick={() => findOnline(c)}
                      disabled={busy === c.model}
                      className="px-3 py-1.5 rounded-md bg-bg-secondary hover:bg-surface text-sm disabled:opacity-50"
                    >
                      {busy === c.model ? t('gearLooking') : t('gearFind')}
                    </button>
                  )}
                  <button
                    onClick={() => addFile(c)}
                    disabled={busy === c.model}
                    className="px-3 py-1.5 rounded-md bg-bg-secondary hover:bg-surface text-sm disabled:opacity-50"
                  >
                    {t('gearAddProfile')}
                  </button>
                  <button
                    onClick={() => forgetCamera(c.model)}
                    className="p-2 text-text-secondary hover:text-red-400 hover:bg-bg-secondary rounded-md transition-colors"
                    data-tooltip={t('gearForget')}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              </div>

              <div className="mt-2 pt-2 border-t border-surface/50">
                {(profiles[c.model]?.profiles.length ?? 0) === 0 ? (
                  <p className="text-xs italic text-text-secondary">{t('gearNoProfilesFor')}</p>
                ) : (
                  <ul className="space-y-1">
                    {profiles[c.model].profiles.map((p) => (
                      <li key={p.file} className="flex items-center justify-between gap-2">
                        <span className="text-xs text-text-secondary truncate" title={p.file}>
                          {p.name ?? p.file}
                        </span>
                        <button
                          onClick={() => removeProfile(p.file)}
                          className="p-1 text-text-secondary hover:text-red-400 rounded transition-colors shrink-0"
                          data-tooltip={t('gearRemoveProfile')}
                        >
                          <Trash2 size={14} />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              {note?.model === c.model && <p className="mt-2 text-xs text-red-400">{note.text}</p>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
