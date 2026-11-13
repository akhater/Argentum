/**
 * How a new photo opens: My Gear's choices for photos with no saved edit. Ours.
 *
 * PER LENS: "Correct new photos", in My Lenses.
 *
 * Every photo opened adds its lens to My Lenses: by the lens database's name
 * when it knows the lens, otherwise by the camera's maker and the lens (or,
 * for a camera with a fixed lens, the body) its EXIF names. A lens joins with
 * its switch off.
 *
 * Off is the app as it was: nothing here touches the photo. On, a RAW photo
 * with no saved edit opens as if Auto had been pressed in Crop > Lens
 * Correction: the lens database's profile when it knows the lens, and the
 * camera's own correction when the file has one (which replaces the profile
 * while it is on, see argentum/lensBuiltin.ts). That is a change to the
 * photo, so it is saved like any other. A photo with an edit is never
 * touched, whatever its lens's switch says.
 *
 * PER CAMERA: "New photos open with", in My Cameras: the camera profile a
 * RAW photo with no saved edit opens with (mods/profiles.rs). Unset, the
 * built-in matrix, as before. A photo with an edit keeps its own.
 */

import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEditorStore } from '../store/useEditorStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { ag } from './ag';
import { ask, type Found } from './lensBuiltin';
import type { Adjustments } from '../utils/adjustments';

/** A row of My Lenses (`MyLens` in src-tauri/src/app_settings.rs). */
export interface MyLens {
  maker: string;
  model: string;
  correctNew?: boolean;
}

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

/** My Lenses' row for this lens, added (switched off) when it is not there yet. */
export function rememberLens(maker: string, model: string): MyLens | null {
  const { appSettings, handleSettingsChange } = useSettingsStore.getState();
  if (!appSettings) {
    return null;
  }
  const known: MyLens[] = (appSettings as { myLenses?: MyLens[] }).myLenses || [];
  const row = known.find((l) => same(l.maker, maker) && same(l.model, model));
  if (row) {
    return row;
  }
  const lens = { maker, model };
  const next = [...known, lens].sort((a, b) => a.maker.localeCompare(b.maker) || a.model.localeCompare(b.model));
  handleSettingsChange({ ...appSettings, myLenses: next } as typeof appSettings);
  return lens;
}

/** Paths handled this session: a photo is looked at once, when it first opens. */
const handled = new Set<string>();

async function onOpen(path: string, exif: Record<string, string>, isRaw: boolean) {
  const found = await ask(path);
  await Promise.all([lensOnOpen(path, exif, isRaw, found), profileOnOpen(path, exif, isRaw, found)]);
}

/** A question that failed counts as "none", and the log says it failed rather than found nothing. */
function unanswered(err: unknown): null {
  console.warn('New photo defaults: a question to the app failed:', err);
  return null;
}

/** The edit as it is now, when `path` is still the open photo. */
function stillOpen(path: string) {
  const { selectedImage, adjustments, setEditor } = useEditorStore.getState();
  return selectedImage?.path === path ? { adjustments, setEditor } : null;
}

async function profileOnOpen(path: string, exif: Record<string, string>, isRaw: boolean, found: Found) {
  const model = exif.Model?.trim();
  if (!found.fresh || !isRaw || !model) {
    return;
  }
  const file = await ag('camera_default_profile', { model }).catch(unanswered);
  const open = file ? stillOpen(path) : null;
  open?.setEditor({ adjustments: { ...open.adjustments, cameraProfile: file } as Adjustments });
}

export async function lensOnOpen(path: string, exif: Record<string, string>, isRaw: boolean, found: Found) {
  const make = exif.Make?.trim();
  if (!make) {
    return;
  }
  // An EXIF cached before CR3 lenses were read has none: ask the file, as Auto does.
  const lensName = exif.LensModel?.trim() || (await ag('recover_lens_name', { path }).catch(unanswered)) || '';
  const detected = await invoke<[string, string] | null>('autodetect_lens', {
    maker: make,
    model: lensName,
    cameraModel: exif.Model ?? '',
    exif,
  }).catch(unanswered);
  const name = detected ?? [make, lensName || exif.Model?.trim()];
  if (!name[1]) {
    return;
  }
  if (!correctsOnOpen(rememberLens(name[0], name[1]), found, isRaw)) {
    return;
  }

  const profile = detected
    ? await invoke('get_lens_distortion_params', {
        maker: detected[0],
        model: detected[1],
        exif,
      }).catch(unanswered)
    : null;
  const open = stillOpen(path);
  open?.setEditor({
    adjustments: corrected(
      open.adjustments,
      detected && profile ? { lens: detected, profile } : null,
      !!found.correction,
    ),
  });
}

/** Whether a photo opens corrected: its lens is switched on, it is a RAW, and it has no saved edit. */
export const correctsOnOpen = (lens: MyLens | null, found: Found, isRaw: boolean): boolean =>
  lens?.correctNew === true && found.fresh && isRaw;

/**
 * The edit as if Auto had been pressed: the lens database's profile when it
 * knows the lens, and the camera's own correction when the file has one.
 */
export const corrected = (
  adjustments: Adjustments,
  database: { lens: [string, string]; profile: unknown } | null,
  hasCameraTables: boolean,
): Adjustments =>
  ({
    ...adjustments,
    ...(database
      ? {
          lensCorrectionMode: 'auto',
          lensMaker: database.lens[0],
          lensModel: database.lens[1],
          lensDistortionParams: database.profile,
        }
      : {}),
    ...(hasCameraTables ? { lensBuiltin: true } : {}),
  }) as Adjustments;

/** Mounted once, in Argentum.tsx. */
export function useNewPhotoDefaults() {
  const path = useEditorStore((s) => s.selectedImage?.path);
  const ready = useEditorStore((s) => s.selectedImage?.isReady === true && !!s.selectedImage?.exif?.Make);

  useEffect(() => {
    if (!path || !ready || handled.has(path)) {
      return;
    }
    handled.add(path);
    const { selectedImage } = useEditorStore.getState();
    onOpen(path, selectedImage?.exif ?? {}, selectedImage?.isRaw === true).catch((err) =>
      console.error('New photo defaults could not be applied:', path, err),
    );
  }, [path, ready]);
}
