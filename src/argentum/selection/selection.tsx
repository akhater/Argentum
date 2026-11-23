/**
 * Subject, Sky, Background and Object masks, from RAWmakase. Ours.
 *
 * Subject finds the photo's people and animals, Sky its sky, Background
 * everything but the Subject; Object is whatever is clicked: a click adds,
 * Alt-click leaves out, a dragged box says where. The models run in
 * mods/selection.rs, on RAWmakase's code (src-tauri/rm_selection). They
 * replace their Sky and their Object (SAM 1, one box) in the add-mask menus;
 * Foreground stays, and masks made with theirs keep working.
 *
 * A selection is stored as their Foreground mask stores its own (their Sky's
 * for Sky), with `agSelection` naming which, so it renders through their code
 * unchanged. The menu entries are types of ours in Masks.tsx (`ag-subject`,
 * `ag-sky`, `ag-background`) that last only until the mask is made.
 *
 * The models (271 MB) are fetched only when asked, after saying their size,
 * and never when they are already in the app's model folder.
 *
 * Text is RapidRAW 1.6.7's (211ef559): type what to select, and EfficientSAM3
 * finds it (mods/text_mask.rs). It is stored the same way, its words kept as
 * `agWords`, and its own models (387 MB) are asked for the same way. Face is
 * RapidRAW 1.6.7's too (07ef9211): every face, with the parts picked, kept as
 * `agParts` (mods/face_mask.rs); its models (226 MB, 205 of them Text's) the same.
 */

import { listen } from '@tauri-apps/api/event';
import { useCallback } from 'react';
import { toast } from 'react-toastify';
import { Mask, type SubMask } from '../../components/panel/right/Masks';
import { useEditorActions } from '../../hooks/useEditorActions';
import { applyToPhoto } from '../lateEdit';
import { stillOpen } from '../stillOpen';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import type { Adjustments } from '../../utils/adjustments';
import { ag } from '../ag';
import { formatSize } from '../formatSize';
import { agText, type AgKey } from '../locales';
import type { Bounds, Click } from '../ag.generated';
export type { Bounds, Click } from '../ag.generated';

export type SelectionKind = 'subject' | 'sky' | 'background' | 'object' | 'text' | 'face';

/** RapidRAW 1.6.7's FACE_PARTS (Masks.tsx @ 07ef9211), in their order. */
export const FACE_PARTS = [
  'faceSkin',
  'bodySkin',
  'eyebrows',
  'eyeWhites',
  'iris',
  'lips',
  'teeth',
  'nose',
  'hair',
] as const;
export type FacePart = (typeof FACE_PARTS)[number];

const KINDS: Partial<Record<Mask, SelectionKind>> = {
  [Mask.AgSubject]: 'subject',
  [Mask.AgSky]: 'sky',
  [Mask.AgBackground]: 'background',
  [Mask.AgObject]: 'object',
  [Mask.AgText]: 'text',
  [Mask.AgFace]: 'face',
};

/** The selection a menu entry makes, if it is one of ours. */
export const selectionKind = (type: Mask): SelectionKind | null => KINDS[type] ?? null;

/** Which of theirs a selection is stored as. */
export const storedType = (kind: SelectionKind): Mask => (kind === 'sky' ? Mask.AiSky : Mask.AiForeground);

/** The menu entry a stored selection came from, for its icon and name. */
export function menuTypeOf(subMask: Pick<SubMask, 'type' | 'parameters'>): Mask {
  const kind = subMask.parameters?.agSelection as SelectionKind | undefined;
  const entry = Object.entries(KINDS).find(([, k]) => k === kind)?.[0] as Mask | undefined;
  return entry ?? subMask.type;
}

/** What a new selection sub-mask starts with, before the models have answered. */
export const startingParameters = (kind: SelectionKind) => ({
  agSelection: kind,
  maskDataBase64: null,
  grow: 0,
  feather: 0,
  ...(kind === 'object' ? { agClicks: [], agBounds: null } : {}),
  ...(kind === 'text' ? { agWords: '' } : {}),
  // Theirs starts with Face Skin picked (maskUtils.ts @ 07ef9211).
  ...(kind === 'face' ? { agParts: ['faceSkin'] } : {}),
});

/** An Object selection: the canvas takes clicks and boxes for it. */
export const isObjectSelection = (subMask?: Pick<SubMask, 'parameters'> | null): boolean =>
  subMask?.parameters?.agSelection === 'object';

/** A Text selection: the properties show its words box. */
export const isTextSelection = (subMask?: Pick<SubMask, 'parameters'> | null): boolean =>
  subMask?.parameters?.agSelection === 'text';

/** A Face selection: the properties show its picker. */
export const isFaceSelection = (subMask?: Pick<SubMask, 'parameters'> | null): boolean =>
  subMask?.parameters?.agSelection === 'face';

const megabytes = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`;

/** A set of models: how to ask whether they are here, fetch them, and say so. */
export interface Models {
  status: 'selection_models_status' | 'text_mask_models_status' | 'face_mask_models_status';
  download: 'download_selection_models' | 'download_text_mask_models' | 'download_face_mask_models';
  /** Their entry in mods/model_catalog.rs, which their download progress names. */
  id: string;
  ask: AgKey;
  downloading: AgKey;
  downloaded: AgKey;
  size: (bytes: number) => string;
}

const SELECTION_MODELS: Models = {
  status: 'selection_models_status',
  download: 'download_selection_models',
  id: 'selection',
  ask: 'selDownloadAsk',
  downloading: 'selDownloading',
  downloaded: 'selDownloaded',
  size: megabytes,
};

/** The Text mask's: sized as Settings > AI models sizes them. */
const TEXT_MODELS: Models = {
  status: 'text_mask_models_status',
  download: 'download_text_mask_models',
  id: 'text-masks',
  ask: 'textDownloadAsk',
  downloading: 'textDownloading',
  downloaded: 'textDownloaded',
  size: formatSize,
};

/** The Face mask's, the same way. */
const FACE_MODELS: Models = {
  status: 'face_mask_models_status',
  download: 'download_face_mask_models',
  id: 'face-masks',
  ask: 'faceDownloadAsk',
  downloading: 'faceDownloading',
  downloaded: 'faceDownloaded',
  size: formatSize,
};

/** Asks once, in a note with two buttons, before anything is fetched. */
function askToDownload(models: Models, bytes: number): Promise<boolean> {
  return new Promise((resolve) => {
    const id = toast.info(
      ({ closeToast }) => (
        <div className="flex flex-col gap-2">
          <span>{agText(models.ask).replace('{size}', models.size(bytes))}</span>
          <span className="flex gap-3">
            <button
              className="underline"
              onClick={() => {
                resolve(true);
                closeToast();
              }}
            >
              {agText('selDownload')}
            </button>
            <button
              className="underline opacity-70"
              onClick={() => {
                resolve(false);
                closeToast();
              }}
            >
              {agText('selNotNow')}
            </button>
          </span>
        </div>
      ),
      { autoClose: false, closeOnClick: false, onClose: () => resolve(false) },
    );
    void id;
  });
}

/** The set of models a selection runs on. */
export const modelsOf = (kind: SelectionKind): Models =>
  kind === 'text' ? TEXT_MODELS : kind === 'face' ? FACE_MODELS : SELECTION_MODELS;

/** The models are there, or fetched now with the user's yes. */
export async function ensureModels(models: Models = SELECTION_MODELS): Promise<boolean> {
  const status = await ag(models.status);
  if (status.installed) return true;
  if (!(await askToDownload(models, status.missingBytes))) return false;
  const note = toast.info(agText(models.downloading).replace('{percent}', '0'), { autoClose: false });
  // mods/model_download.rs reports every model download on this one event.
  const unlisten = await listen<{ id: string; done: number; total: number }>('ag-model-download', (event) => {
    if (event.payload.id !== models.id) return;
    const percent = Math.floor((100 * event.payload.done) / Math.max(1, event.payload.total));
    toast.update(note, { render: agText(models.downloading).replace('{percent}', String(percent)) });
  });
  try {
    await ag(models.download);
    toast.update(note, { render: agText(models.downloaded), type: 'success', autoClose: 3000 });
    return true;
  } catch (err) {
    toast.update(note, { render: String(err), type: 'error', autoClose: 8000 });
    return false;
  } finally {
    unlisten();
  }
}

/** The edit without what the picture does not depend on, as their AI masks send it. */
export function geometryOf(adjustments: Adjustments) {
  const { masks: _masks, aiPatches: _patches, ...rest } = adjustments as Adjustments & { aiPatches?: unknown };
  return rest;
}

function findSubMask(adjustments: Adjustments, id: string): SubMask | undefined {
  return adjustments.masks?.flatMap((c) => c.subMasks).find((sm: SubMask) => sm.id === id);
}

/** The latest Object request per sub-mask: an answer to an older click is dropped. */
const objectRequests = new Map<string, number>();

/**
 * One gesture on an Object selection (ImageCanvas.tsx): a click (start and
 * end the same) adds a point, or leaves it out with Alt; a drag replaces the
 * box. The gesture is kept on the sub-mask at once, one History step, and the
 * outline follows when the model answers.
 */
export async function refineObject(
  subMaskId: string,
  start: { x: number; y: number },
  end: { x: number; y: number },
  leaveOut: boolean,
  update: (id: string, data: Partial<SubMask>) => void,
) {
  const sm = findSubMask(useEditorStore.getState().adjustments, subMaskId);
  if (!sm) return;
  const before = sm.parameters ?? {};
  const clicks: Click[] = [...(before.agClicks ?? [])];
  let bounds: Bounds | null = before.agBounds ?? null;
  if (start.x === end.x && start.y === end.y) clicks.push({ x: start.x, y: start.y, positive: !leaveOut });
  else bounds = { startX: start.x, startY: start.y, endX: end.x, endY: end.y };
  update(subMaskId, { parameters: { ...before, agClicks: clicks, agBounds: bounds } });
  if (!(await ensureModels())) return;

  const request = (objectRequests.get(subMaskId) ?? 0) + 1;
  objectRequests.set(subMaskId, request);
  const { adjustments, patchesSentToBackend } = useEditorStore.getState();
  const tasks = useProcessStore.getState();
  tasks.registerAiTask(subMaskId, 'subject');
  try {
    const { found: _found, ...result } = await ag('selection_object', {
      jsAdjustments: geometryOf(adjustments),
      clicks,
      bounds,
      rotation: adjustments.rotation,
      flipHorizontal: adjustments.flipHorizontal,
      flipVertical: adjustments.flipVertical,
      orientationSteps: adjustments.orientationSteps,
    });
    if (objectRequests.get(subMaskId) !== request) return;
    const latest = findSubMask(useEditorStore.getState().adjustments, subMaskId)?.parameters ?? {};
    patchesSentToBackend.delete(subMaskId);
    update(subMaskId, { parameters: { ...latest, ...result } });
  } catch (err) {
    toast.error(agText('selFailed').replace('{error}', String(err)));
  } finally {
    if (objectRequests.get(subMaskId) === request) tasks.unregisterAiTask(subMaskId);
  }
}

/** The latest Text request per sub-mask: an answer to older words, or a cancelled one, is dropped. */
const textRequests = new Map<string, number>();

/**
 * A Text selection's words (TextWords.tsx): kept on the sub-mask, which takes
 * their name as RapidRAW 1.6.7 names it ("car" becomes Car), and its outline
 * when the models answer.
 */
export async function findWords(
  subMaskId: string,
  words: string,
  update: (id: string, data: Partial<SubMask>) => void,
) {
  const typed = words.trim();
  const sm = findSubMask(useEditorStore.getState().adjustments, subMaskId);
  if (!sm || !typed) return;
  const name = typed.charAt(0).toUpperCase() + typed.slice(1);
  update(subMaskId, { name, parameters: { ...sm.parameters, agWords: typed } });
  if (!(await ensureModels(TEXT_MODELS))) return;

  const request = (textRequests.get(subMaskId) ?? 0) + 1;
  textRequests.set(subMaskId, request);
  const { adjustments, patchesSentToBackend } = useEditorStore.getState();
  const tasks = useProcessStore.getState();
  tasks.registerAiTask(subMaskId, 'text');
  try {
    const { found, ...result } = await ag('text_mask', {
      jsAdjustments: geometryOf(adjustments),
      words: typed,
      rotation: adjustments.rotation,
      flipHorizontal: adjustments.flipHorizontal,
      flipVertical: adjustments.flipVertical,
      orientationSteps: adjustments.orientationSteps,
    });
    if (textRequests.get(subMaskId) !== request) return;
    const latest = findSubMask(useEditorStore.getState().adjustments, subMaskId)?.parameters ?? {};
    patchesSentToBackend.delete(subMaskId);
    update(subMaskId, { parameters: { ...latest, ...result } });
    if (!found) toast.info(agText('textNotFound').replace('{words}', typed));
  } catch (err) {
    if (textRequests.get(subMaskId) === request) toast.error(agText('selFailed').replace('{error}', String(err)));
  } finally {
    if (textRequests.get(subMaskId) === request) tasks.unregisterAiTask(subMaskId);
  }
}

/** Stops waiting for a Text selection's answer: it is dropped when it comes. */
export function cancelWords(subMaskId: string) {
  textRequests.set(subMaskId, (textRequests.get(subMaskId) ?? 0) + 1);
  useProcessStore.getState().unregisterAiTask(subMaskId);
}

/** The latest Face request per sub-mask: an answer for parts since changed is dropped. */
const faceRequests = new Map<string, number>();

/**
 * A Face selection's parts (FaceParts.tsx, or Face Skin when it is made):
 * every face in the photo is found once and the parts drawn, and its outline
 * follows when the models answer.
 */
export async function findFace(
  subMaskId: string,
  parts: ReadonlyArray<FacePart>,
  update: (id: string, data: Partial<SubMask>) => void,
) {
  if (!(await ensureModels(FACE_MODELS))) return;
  const request = (faceRequests.get(subMaskId) ?? 0) + 1;
  faceRequests.set(subMaskId, request);
  const { adjustments, patchesSentToBackend } = useEditorStore.getState();
  const tasks = useProcessStore.getState();
  tasks.registerAiTask(subMaskId, 'face');
  try {
    const { found, ...result } = await ag('face_mask', {
      jsAdjustments: geometryOf(adjustments),
      parts,
      rotation: adjustments.rotation,
      flipHorizontal: adjustments.flipHorizontal,
      flipVertical: adjustments.flipVertical,
      orientationSteps: adjustments.orientationSteps,
    });
    if (faceRequests.get(subMaskId) !== request) return;
    const latest = findSubMask(useEditorStore.getState().adjustments, subMaskId)?.parameters ?? {};
    patchesSentToBackend.delete(subMaskId);
    update(subMaskId, { parameters: { ...latest, ...result } });
    if (!found) toast.info(agText('faceNotFound'));
  } catch (err) {
    if (faceRequests.get(subMaskId) === request) toast.error(agText('selFailed').replace('{error}', String(err)));
  } finally {
    if (faceRequests.get(subMaskId) === request) tasks.unregisterAiTask(subMaskId);
  }
}

/** Clears an Object selection's clicks, box and outline, to begin again. */
export function startOverObject(subMaskId: string, update: (id: string, data: Partial<SubMask>) => void) {
  const sm = findSubMask(useEditorStore.getState().adjustments, subMaskId);
  if (!sm) return;
  objectRequests.set(subMaskId, (objectRequests.get(subMaskId) ?? 0) + 1);
  useEditorStore.getState().patchesSentToBackend.delete(subMaskId);
  update(subMaskId, { parameters: { ...sm.parameters, agClicks: [], agBounds: null, maskDataBase64: null } });
}

/** Fills a new selection sub-mask: MasksPanel calls it as theirs call their own. */
export function useSelectionMasks() {
  const { setAdjustments } = useEditorActions();
  const registerAiTask = useProcessStore((s) => s.registerAiTask);
  const unregisterAiTask = useProcessStore((s) => s.unregisterAiTask);

  const fill = useCallback(
    (subMaskId: string, parameters: object, path?: string) =>
      // A result for the photo `path` goes to that photo, which may no longer be the open one.
      (path
        ? (update: (prev: Adjustments) => Adjustments) => applyToPhoto(path, update, setAdjustments)
        : setAdjustments)((prev: Adjustments) => ({
        ...prev,
        masks: prev.masks.map((c) => ({
          ...c,
          subMasks: c.subMasks.map((sm: SubMask) =>
            sm.id === subMaskId ? { ...sm, parameters: { ...sm.parameters, ...parameters } } : sm,
          ),
        })),
      })),
    [setAdjustments],
  );

  /** A selection that will not be made: its empty sub-mask goes, and its mask with it if it was alone. */
  const drop = useCallback(
    (subMaskId: string, path?: string) =>
      (path
        ? (update: (prev: Adjustments) => Adjustments) => applyToPhoto(path, update, setAdjustments)
        : setAdjustments)((prev: Adjustments) => ({
        ...prev,
        masks: prev.masks
          .map((c) => ({ ...c, subMasks: c.subMasks.filter((sm: SubMask) => sm.id !== subMaskId) }))
          .filter((c) => c.subMasks.length > 0),
      })),
    [setAdjustments],
  );

  return useCallback(
    async (subMaskId: string, kind: SelectionKind) => {
      const path = useEditorStore.getState().selectedImage?.path;
      if (!path) return;
      if (!(await ensureModels(modelsOf(kind)))) {
        drop(subMaskId, path);
        return;
      }
      // Object waits for clicks on the photo (refineObject), Text for its words (findWords).
      if (kind === 'object' || kind === 'text') return;
      if (kind === 'face') {
        await findFace(subMaskId, ['faceSkin'], (id, data) => fill(id, data.parameters ?? {}));
        return;
      }
      const { adjustments, patchesSentToBackend } = useEditorStore.getState();
      registerAiTask(subMaskId, 'foreground');
      try {
        const { found, ...parameters } = await ag('selection_mask', {
          kind,
          jsAdjustments: geometryOf(adjustments),
          rotation: adjustments.rotation,
          flipHorizontal: adjustments.flipHorizontal,
          flipVertical: adjustments.flipVertical,
          orientationSteps: adjustments.orientationSteps,
        });
        patchesSentToBackend.delete(subMaskId);
        fill(subMaskId, parameters, path);
        if (!found && stillOpen(path)) toast.info(agText(kind === 'sky' ? 'selNoSky' : 'selNoSubject'));
      } catch (err) {
        toast.error(agText('selFailed').replace('{error}', String(err)));
      } finally {
        unregisterAiTask(subMaskId);
      }
    },
    [drop, fill, registerAiTask, unregisterAiTask],
  );
}
