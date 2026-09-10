/**
 * The camera profile control in the Color panel. Ours.
 *
 * One dropdown: Built-in, or any profile the library holds for this camera.
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
 */

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
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

export default function CameraProfile() {
  const t = useAgTranslation();
  const selectedImage = useEditorStore((s: any) => s.selectedImage);
  const adjustments = useEditorStore((s: any) => s.adjustments);
  const { setAdjustments } = useEditorActions();
  const path: string | undefined = selectedImage?.path;

  const [status, setStatus] = useState<Status | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    if (!path) {
      setStatus(null);
      return;
    }
    try {
      setStatus(await ag<Status>('camera_profile_status', { path }));
    } catch {
      setStatus(null);
    }
  }, [path]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  /**
   * Change the profile and redraw.
   *
   * The matrix is applied while the RAW is decoded, so the decode has to happen
   * again; the sidecar is written first because that is where the decode reads
   * the choice from, and nudging the adjustments afterwards is what makes the
   * pipeline produce a new preview.
   */
  const choose = async (file: string) => {
    if (!path) {
      return;
    }
    const chosen = file === '' ? null : file;
    setBusy(true);
    try {
      // One writer, and only one.
      //
      // This used to set the store, save the sidecar itself, and force a
      // reload. Three writers for one value: the debounced auto-save fired
      // afterwards with a stale copy and won, so a chosen profile reverted to
      // the previous one a second later — the picture went right, then wrong
      // again. Setting the store and letting their own save persist it is the
      // whole of it.
      setAdjustments((prev: any) => ({ ...prev, cameraProfile: chosen }));
    } finally {
      setBusy(false);
    }
  };

  const findOnline = async () => {
    setBusy(true);
    try {
      await ag('get_profile_online', { model: status?.camera ?? '' });
      await refresh();
    } catch {
      // Nothing published for this camera. The dropdown stays as it is.
    } finally {
      setBusy(false);
    }
  };

  if (!status?.camera) {
    return null;
  }

  const current: string = adjustments?.cameraProfile ?? '';

  return (
    <div className="p-2 bg-bg-tertiary rounded-md">
      <div className="flex justify-between items-center mb-2">
        <span className="text-sm font-semibold text-text-primary">{t('profileLabel')}</span>
        {/*
          RawTherapee publishes one profile per camera, so the offer to fetch it
          stands until that file is here — an imported profile does not answer
          it, and owning none is a different question.
        */}
        {!status.publishedInstalled && (
          <button
            onClick={findOnline}
            disabled={busy}
            className="px-2 py-0.5 rounded text-xs bg-bg-secondary hover:bg-surface text-text-primary disabled:opacity-50"
            data-tooltip={t('profileFindTooltip')}
          >
            {busy ? '…' : t('gearFind')}
          </button>
        )}
      </div>

      {status.available.length > 0 ? (
        <select
          value={current}
          disabled={busy}
          onChange={(e) => choose(e.target.value)}
          className="w-full text-xs bg-bg-primary text-text-primary rounded px-2 py-1.5 truncate disabled:opacity-50"
        >
          <option value="">{t('profileBuiltIn')}</option>
          {status.available.map((p) => (
            <option key={p.file} value={p.file}>
              {p.name ?? p.file}
            </option>
          ))}
        </select>
      ) : (
        <p className="text-xs text-text-secondary">{t('profileBuiltIn')}</p>
      )}
    </div>
  );
}
