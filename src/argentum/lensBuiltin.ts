/**
 * The camera's built-in lens correction, filled into the open photo's edit. Ours.
 *
 * Fujifilm stores its lens's distortion, colour fringing and corner shading
 * in every RAF, for the focal length and aperture used, and Lightroom applies
 * it by default; Sony stores the same in an ARW, where it starts off, and a
 * DNG (Leica, DJI's drones, phones, Lightroom's own) as opcodes, on.
 * `lens_builtin` (mods/lens_builtin.rs) reads it from the file;
 * this puts it into the edit as `lensBuiltinParams`, with the switch,
 * `lensBuiltin`, at the photo's own default when the edit has none yet. From
 * there it travels like lensfun's numbers: preview, export, masks and the
 * thumbnail all read it from the edit.
 *
 * WHEN
 *
 * The file is asked once per photo per session, as soon as the photo is
 * chosen. The edit is filled the moment the photo is ready, which is before
 * its first render, so opening a photo saves nothing: the edit is only
 * written once something is changed. The same check runs on every change to
 * the editor while the photo is open, so a reset that drops the keys gets
 * them back, with the switch at its default.
 *
 * Not an edit, so not in the history: undoing past it fills it in again.
 */

import { useEffect } from 'react';
import { ag } from './ag';
import { useEditorStore } from '../store/useEditorStore';
import type { Adjustments } from '../utils/adjustments';

interface Radial {
  knots: number[];
  values: number[];
}

/**
 * HAND-WRITTEN STUB of `rm_lens_builtin::LensCorrection`
 * (src-tauri/engine/src/rm_lens_builtin.rs), until today's interface has
 * generated types. Only `source` and which parts are present are read here.
 */
export interface LensCorrection {
  source: string;
  default_on: boolean;
  vignetting: Radial | null;
  distortion: Radial | null;
  chromatic: [Radial, Radial] | null;
  /** A DNG's OpcodeList3 gain map (`GainGrid`), absent when it has none. */
  gain_map?: object;
}

/** What `lens_builtin` answers. */
interface Found {
  correction: LensCorrection | null;
  defaultOn: boolean;
}

/** The edit's keys for it (mods/lens_builtin.rs). */
interface BuiltinEdit {
  lensBuiltin?: boolean;
  lensBuiltinParams?: LensCorrection | null;
}

const edit = (adjustments: Adjustments) => adjustments as Adjustments & BuiltinEdit;

/** The photo's own tables, when its edit holds them. */
export const builtinTables = (adjustments: Adjustments): LensCorrection | null =>
  edit(adjustments).lensBuiltinParams ?? null;

/** The correction the edit applies: the photo's tables while the switch is on. */
export const builtinInUse = (adjustments: Adjustments): LensCorrection | null =>
  edit(adjustments).lensBuiltin === true ? builtinTables(adjustments) : null;

/** The edit with the switch set. */
export const withBuiltin = (adjustments: Adjustments, on: boolean): Adjustments =>
  ({ ...adjustments, lensBuiltin: on }) as Adjustments;

/** Spread into a reset: the switch goes back to the photo's own default. */
export const LENS_BUILTIN_RESET = { lensBuiltin: undefined } as Partial<Adjustments>;

/** Per path, for the session: a file's tables do not change while the app runs. */
const asked = new Map<string, Promise<Found>>();

const ask = (path: string): Promise<Found> => {
  let answer = asked.get(path);
  if (!answer) {
    answer = ag<Found>('lens_builtin', { path });
    asked.set(path, answer);
    answer.catch(() => asked.delete(path));
  }
  return answer;
};

/** Fills the open photo's edit in, once it is this photo's and when it lacks either key. */
const fillFor = (path: string, found: Found & { correction: LensCorrection }) => {
  // The tables last found equal to the photo's, so most changes compare a reference.
  let checked: unknown = null;
  return () => {
    const { selectedImage, adjustments, setEditor } = useEditorStore.getState();
    // isReady: until then the adjustments may still be the last photo's.
    if (selectedImage?.path !== path || !selectedImage.isReady) return;
    const current = edit(adjustments);
    if (current.lensBuiltin !== undefined && current.lensBuiltinParams === checked) return;
    const same = JSON.stringify(current.lensBuiltinParams ?? null) === JSON.stringify(found.correction);
    if (same && current.lensBuiltin !== undefined) {
      checked = current.lensBuiltinParams;
      return;
    }
    checked = found.correction;
    setEditor({
      adjustments: {
        ...adjustments,
        lensBuiltinParams: found.correction,
        lensBuiltin: current.lensBuiltin ?? found.defaultOn,
      } as Adjustments,
    });
  };
};

/** Mounted once, in Argentum.tsx. */
export function useLensBuiltinOnOpen() {
  const path = useEditorStore((s) => s.selectedImage?.path);

  useEffect(() => {
    if (!path) return;
    let stopped = false;
    let unsubscribe = () => {};
    ask(path)
      .then((found) => {
        if (stopped || !found.correction) return;
        const fill = fillFor(path, { ...found, correction: found.correction });
        fill();
        unsubscribe = useEditorStore.subscribe(fill);
      })
      .catch((err) => console.error('Built-in lens correction could not be read:', path, err));
    return () => {
      stopped = true;
      unsubscribe();
    };
  }, [path]);
}

/** The lensfun choice's look while the camera's correction replaces it. */
export const LENSFUN_UNUSED = 'opacity-50 grayscale pointer-events-none';
