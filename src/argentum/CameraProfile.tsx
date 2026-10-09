/**
 * The camera profile control in the Color panel. Ours.
 *
 * One dropdown, always: Built-in, any profile the library holds for this
 * camera, and "Find one…" while RawTherapee's profile for it is not here yet.
 *
 * WHAT THIS WENT THROUGH, SO IT IS NOT REPEATED
 *
 * It was a checkbox, and the choice was per camera. Both were wrong.
 *
 * A checkbox says "on or off". The semantics are "one of several" — a camera
 * can have a maker's rendering, someone's calibration, your own — and only one
 * applies. That is a dropdown, and it always was.
 *
 * Per camera was wrong for a better reason. I argued a profile is a
 * calibration and calibrations belong to equipment, which is true of the
 * physics and irrelevant to the person using it: profiles differ in look, and
 * what you actually do is try one on the photo in front of you and compare.
 * A setting you cannot compare on the picture you care about is barely a
 * setting. So it is an adjustment, saved to the photo's sidecar, undone and
 * copied like every other one.
 *
 * It also told the user to "reopen the photo to see a change", twice, in a
 * paragraph, in a panel 180 pixels wide. Changing it does need the RAW decoded
 * again — but that is `load_image`, which the app already has, so it does it.
 *
 * Then, with no profile installed, the dropdown became the words "Built-in"
 * under the label and "Find one" became a button beside it: a choice shown as
 * a fact, and the way to widen it somewhere else. Finding one is a way of
 * choosing one, so it is the last entry in the list. What it finds is
 * selected; when nothing is published the list goes back to what it was and
 * stops offering.
 */

import { useCallback, useEffect, useState } from 'react';
import { useEditorStore } from '../store/useEditorStore';
import { useEditorActions } from '../hooks/useEditorActions';
import { ag } from './ag';
import { useAgTranslation } from './locales';

interface Installed {
  file: string;
  name: string | null;
}

interface Status {
  camera: string | null;
  make: string | null;
  available: Installed[];
  /** RawTherapee's own file for this body is already here. */
  publishedInstalled: boolean;
}

/** The entry that means "go and look". `?` cannot be in a Windows file name. */
const FIND = '?find';

export default function CameraProfile() {
  const t = useAgTranslation();
  const selectedImage = useEditorStore((s: any) => s.selectedImage);
  const adjustments = useEditorStore((s: any) => s.adjustments);
  const { setAdjustments } = useEditorActions();
  const path: string | undefined = selectedImage?.path;

  const [status, setStatus] = useState<Status | null>(null);
  const [looking, setLooking] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  /** The camera RawTherapee had nothing for, so the list stops offering. */
  const [missed, setMissed] = useState<string | null>(null);

  const refresh = useCallback(async (): Promise<Status | null> => {
    let next: Status | null = null;
    if (path) {
      try {
        next = await ag<Status>('camera_profile_status', { path });
      } catch {
        next = null;
      }
    }
    setStatus(next);
    return next;
  }, [path]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    setNote(null);
  }, [status?.camera]);

  /**
   * Change the profile and redraw.
   *
   * One writer, and only one. This used to set the store, save the sidecar
   * itself, and force a reload. Three writers for one value: the debounced
   * auto-save fired afterwards with a stale copy and won, so a chosen profile
   * reverted to the previous one a second later — the picture went right, then
   * wrong again. Setting the store and letting their own save persist it is the
   * whole of it.
   */
  const choose = (file: string) => {
    setNote(null);
    setAdjustments((prev: any) => ({ ...prev, cameraProfile: file === '' ? null : file }));
  };

  const findOnline = async () => {
    const camera = status?.camera;
    if (!camera) {
      return;
    }
    setNote(null);
    setLooking(true);
    try {
      const file = await ag<string | null>('get_profile_online', {
        make: status?.make ?? '',
        model: camera,
      });
      const next = await refresh();
      // Another photo may be open by now; what was found is for this one.
      if (useEditorStore.getState().selectedImage?.path !== path) {
        return;
      }
      if (file && next?.available.some((p) => p.file === file)) {
        choose(file);
      } else if (!file) {
        setMissed(camera);
        setNote(t('profileNotPublished'));
      }
    } catch (e) {
      // A network failure, not an answer: the entry stays so it can be tried again.
      setNote(String(e));
    } finally {
      setLooking(false);
    }
  };

  if (!status?.camera) {
    return null;
  }

  const current: string = adjustments?.cameraProfile ?? '';
  // RawTherapee publishes one profile per camera, so the offer to fetch it
  // stands until that file is here — an imported profile does not answer it.
  const canFind = !status.publishedInstalled && missed !== status.camera;

  return (
    <div>
      <div className="flex justify-between items-center mb-2">
        <span className="text-sm font-medium text-text-secondary select-none">{t('profileLabel')}</span>
      </div>

      <select
        value={looking ? FIND : current}
        disabled={looking}
        onChange={(e) => (e.target.value === FIND ? findOnline() : choose(e.target.value))}
        className="w-full text-xs bg-bg-primary text-text-primary rounded px-2 py-1.5 truncate disabled:opacity-50"
      >
        <option value="">{t('profileBuiltIn')}</option>
        {status.available.map((p) => (
          <option key={p.file} value={p.file}>
            {p.name ?? p.file}
          </option>
        ))}
        {canFind && <option value={FIND}>{looking ? t('gearLooking') : t('profileFind')}</option>}
      </select>
      {note && <p className="mt-1 text-xs text-text-secondary">{note}</p>}
    </div>
  );
}
