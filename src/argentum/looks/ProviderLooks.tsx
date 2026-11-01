/**
 * The Provider looks section of the Looks tab, and the look on the photo.
 * Ours.
 *
 * It shows only when a provider is set and the photo can have its looks, so
 * with `lutProvider` absent or empty nothing here renders. Its switch picks
 * which list is usable: on, these looks, the Camera picker and any options work
 * and the regular Presets and LUTs are off; off, the reverse. It never takes a
 * look off a photo. A click starts a job and the photo stays as it was; the
 * store puts the look on when its file is ready. Preview all makes every look
 * of the open photo as a temporary file to compare by hover, and only its
 * button starts it. Delete all previews sends that photo's look files to the
 * Recycle Bin, but for those an edit uses, and drops its temporary files. The
 * head folds the section away, switch left in place, and that stops nothing.
 * The looks show under the groups the provider names (LookGroup.tsx).
 */

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ChevronRight, X } from 'lucide-react';
import clsx from 'clsx';
import Slider from '../../components/ui/Slider';
import { useEditorStore } from '../../store/useEditorStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useUIStore } from '../../store/useUIStore';
import { ag } from '../ag';
import type { OfferedLook } from '../ag.generated';
import { useAgTranslation } from '../locales';
import LookGroup, { groupsOf } from './LookGroup';
import { hoverEdit, lookKeys, lookOf, offeredOptions, optionIsOn, sameOptions } from './providerLook';
import { previewsFor, restoreLook, useProviderLooks } from './providerLooksStore';

/** How long the pointer rests on a look before the photo shows it, as in the lists below. */
const PREVIEW_AFTER_MS = 120;
const SWATCH_PX = 72;
/**
 * Kept beside the Edit panel's sections, as Profile is. Folded rather than open: they start
 * without it, so it is saved for no one who has not folded this, and Focus mode, which shuts
 * every section it finds, can only open this one.
 */
const FOLDED = 'providerLooksFolded';

/**
 * Swatches from the look files of the open photo, by file, one at a time so a whole
 * set of previews never holds the app up. Files added later only add their own.
 */
function useFileSwatches(files: { path: string }[], photo: string | null, ready: boolean) {
  const [swatches, setSwatches] = useState<Record<string, string>>({});
  const made = useProviderLooks((s) => s.filesMade);
  const asked = useRef<{ round: number; paths: Set<string> }>({ round: 0, paths: new Set() });
  const key = files.map((f) => f.path).join('|');

  useEffect(() => {
    asked.current = { round: asked.current.round + 1, paths: new Set() };
    setSwatches({});
  }, [photo, made]);

  useEffect(() => {
    if (!photo || !ready || !key) return;
    const { round, paths } = asked.current;
    const wanted = key.split('|').filter((path) => !paths.has(path));
    wanted.forEach((path) => paths.add(path));
    (async () => {
      for (const path of wanted) {
        const results = await invoke<{ path: string; thumb: string | null }[]>('generate_lut_previews', {
          luts: [{ path, isBuiltIn: false, linear: true }],
          size: SWATCH_PX,
        }).catch((err) => {
          console.warn('Provider looks: swatches:', err);
          return [];
        });
        if (asked.current.round !== round) return;
        const thumb = results[0]?.thumb;
        if (thumb) setSwatches((was) => ({ ...was, [path]: thumb }));
      }
    })();
  }, [key, photo, ready, made]);

  return swatches;
}

interface Props {
  /** The open photo; null in the grid, where a click goes to the selection. */
  photo: string | null;
  ready: boolean;
  onTakeOff: () => void;
}

export default function ProviderLooks({ photo, ready, onTakeOff }: Props) {
  const t = useAgTranslation();
  const adjustments = useEditorStore((s) => s.adjustments);
  const setEditor = useEditorStore((s) => s.setEditor);
  const gridPhoto = useLibraryStore((s) => s.libraryActivePath ?? s.multiSelectedPaths[0] ?? null);
  const subject = photo ?? gridPhoto;
  const {
    offer,
    offerOf,
    waiting,
    note,
    batch,
    load,
    cameraOf,
    chooseCamera,
    optionsOf,
    chooseOption,
    pick,
    putOnSelection,
    cancelBatch,
    previewing,
    previewAll,
    deleting,
    deleteLooks,
    setOn,
  } = useProviderLooks();
  const folded = useUIStore(
    (s) => (s.collapsibleSectionsState as unknown as Record<string, boolean | undefined>)[FOLDED] === true,
  );
  const hover = useRef<number | undefined>(undefined);
  const files = offer?.photo?.files ?? [];

  // In the grid there is no open edit: what shows is the hand choice alone.
  const edit = photo ? adjustments : {};
  const options = subject ? optionsOf(subject, edit) : {};
  const optionsKey = JSON.stringify(options);

  const previews = previewsFor({ offer, offerOf }, photo, options);
  const swatches = useFileSwatches([...files, ...previews], photo, ready);

  // A provider that fails its check is asked again when the tab next opens, not per photo.
  const available = !!offer;
  const asked = useRef(false);
  useEffect(() => {
    if (asked.current && !available) return;
    asked.current = true;
    load(subject, JSON.parse(optionsKey));
  }, [subject, available, load, optionsKey]);

  useEffect(
    () => () => {
      window.clearTimeout(hover.current);
    },
    [],
  );

  // Opening a photo whose look has no file.
  useEffect(() => {
    if (photo && ready && offer) restoreLook(photo, useEditorStore.getState().adjustments);
  }, [photo, ready, offer]);

  // Esc lets go of a look still being made, before it does anything else.
  const spinning = !!photo && !!waiting[photo]?.spinning;
  useEffect(() => {
    if (!spinning || !photo) return;
    const ui = useUIStore.getState();
    const before = ui.customEscapeHandler;
    const handler = () => useProviderLooks.getState().cancel(photo);
    ui.setCustomEscapeHandler(handler);
    return () => {
      const now = useUIStore.getState();
      if (now.customEscapeHandler === handler) now.setCustomEscapeHandler(before);
    };
  }, [spinning, photo]);

  if (!offer?.photo || !subject) return null;

  const { on } = offer;
  const own = offer.photo.camera;
  const camera = cameraOf(subject, adjustments) ?? own;
  const onPhoto = photo ? lookOf(adjustments) : null;
  const fileOf = (look: OfferedLook) => files.find((f) => f.camera === camera && f.look === look.id);
  const previewOf = (look: OfferedLook) => previews.find((f) => f.camera === camera && f.look === look.id);
  const isOn = (look: OfferedLook) =>
    onPhoto?.camera === camera &&
    onPhoto.look === look.id &&
    sameOptions(offeredOptions(offer.options, onPhoto.options), options);

  const stopPreview = () => {
    window.clearTimeout(hover.current);
    setEditor({ previewOverride: null });
  };

  /** Hover never starts a job: a look with no file has nothing to show. */
  const startPreview = (look: OfferedLook) => {
    const preview = fileOf(look) ? undefined : previewOf(look)?.path;
    if (!photo || !on) return;
    if (!fileOf(look) && !preview) {
      stopPreview();
      return;
    }
    window.clearTimeout(hover.current);
    hover.current = window.setTimeout(() => {
      const now = useEditorStore.getState().adjustments;
      const keys = lookKeys(camera, look.id, look.name, options, offer.photo?.ownWb);
      setEditor({ previewOverride: hoverEdit(now, keys, preview) });
    }, PREVIEW_AFTER_MS);
  };

  const choose = (look: OfferedLook) => {
    stopPreview();
    if (!photo) {
      const { multiSelectedPaths } = useLibraryStore.getState();
      const paths = multiSelectedPaths.length > 0 ? multiSelectedPaths : [subject];
      const chosen = useProviderLooks.getState().chosen[subject] ?? null;
      putOnSelection(paths, look, chosen, options);
    } else if (isOn(look)) onTakeOff();
    else pick(photo, look, camera, options);
  };

  const previewCount =
    previewing &&
    t('looksProviderPreviewing')
      .replace('{done}', String(previewing.ended))
      .replace('{total}', String(previewing.total));

  const toggleFolded = () =>
    useUIStore.setState((s) => ({
      collapsibleSectionsState: { ...s.collapsibleSectionsState, [FOLDED]: !folded },
    }));

  const lookRow = (look: OfferedLook) => {
    const file = fileOf(look) ?? previewOf(look);
    const swatch = file && swatches[file.path];
    return (
      <div key={look.id}>
        <div
          className={clsx('agm-look', isOn(look) && 'agm-on', !on && 'agm-look-dim')}
          onClick={() => choose(look)}
          onMouseEnter={() => startPreview(look)}
        >
          <span className="agm-swatch" style={swatch ? { backgroundImage: `url(${swatch})` } : undefined} />
          <span className="agm-look-name">{look.name}</span>
          {subject && waiting[subject]?.look === look.id && waiting[subject].spinning && (
            <span aria-label={t('looksProviderMaking')} className="agm-spin" />
          )}
        </div>
        {note?.photo === subject && note.look === look.id && <div className="agm-provider-rownote">{t(note.text)}</div>}
      </div>
    );
  };

  return (
    <div className="agm-provider" onMouseLeave={stopPreview}>
      <div className="agm-provider-head" onClick={toggleFolded}>
        {folded ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        <span className="min-w-0 truncate">{t('looksProvider')}</span>
        {folded && previewCount && <span className="agm-provider-running">{previewCount}</span>}
        <button
          aria-checked={on}
          aria-label={t('looksProvider')}
          className="agm-switch"
          onClick={(e) => {
            e.stopPropagation();
            setOn(!on);
          }}
          role="switch"
        >
          <span>{t(on ? 'looksProviderOn' : 'looksProviderOff')}</span>
          <i />
        </button>
      </div>
      {!folded && (
        <>
          <div className="agm-provider-note">{t(on ? 'looksProviderOnNote' : 'looksProviderOffNote')}</div>
          <div className={clsx('agm-provider-camera', !on && 'agm-look-dim')}>
            <label htmlFor="agm-provider-camera">{t('looksProviderCamera')}</label>
            <select
              disabled={!on}
              id="agm-provider-camera"
              onChange={(e) => chooseCamera(subject, e.target.value, adjustments)}
              value={camera}
            >
              {offer.cameras.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
            <span className={clsx('agm-provider-matched', camera !== own && 'agm-provider-other')}>
              {t(camera === own ? 'looksProviderMatched' : 'looksProviderOther')}
            </span>
          </div>
          {offer.options.map((option) => (
            <label className={clsx('agm-provider-option', !on && 'agm-look-dim')} key={option.id}>
              <input
                checked={optionIsOn(option, options)}
                disabled={!on}
                onChange={(e) => chooseOption(subject, option, e.target.checked, edit)}
                type="checkbox"
              />
              <span className="min-w-0">{option.label}</span>
            </label>
          ))}
          {photo && (
            <div className={clsx('agm-provider-tools', !on && 'agm-look-dim')}>
              <button disabled={!on || !ready || !!previewing} onClick={() => previewAll(photo, camera, options)}>
                {previewCount || t('looksProviderPreviewAll')}
              </button>
              <button
                className="agm-provider-delete"
                disabled={!on || !ready || deleting || offer.photo.clearable === 0}
                onClick={() => deleteLooks(photo)}
              >
                {t('looksProviderDeleteAll')}
              </button>
            </div>
          )}
          <div className={clsx(!on && 'agm-looks-off')}>
            {groupsOf(offer.looks).map(({ name, looks }) =>
              name === null ? (
                looks.map(lookRow)
              ) : (
                <LookGroup
                  count={looks.length}
                  hasOnPhoto={looks.some(isOn)}
                  key={name}
                  name={name}
                  onMouseEnter={stopPreview}
                >
                  {looks.map(lookRow)}
                </LookGroup>
              ),
            )}
          </div>
        </>
      )}
      {batch && (
        <div className="agm-provider-batch">
          <span>
            {t('looksProviderBatch').replace('{done}', String(batch.done)).replace('{total}', String(batch.total))}
          </span>
          <button onClick={cancelBatch}>{t('looksProviderCancel')}</button>
        </div>
      )}
    </div>
  );
}

interface StripProps {
  /** The provider can make its file. */
  canMake: boolean;
  /** Its camera. */
  kind: string;
  name: string;
  onMake: () => void;
  onStrength: (value: number) => void;
  onTakeOff: () => void;
  strength: number;
  swatch: string | undefined;
  /** Its file is beside the photo. */
  there: boolean;
}

/** A provider look on the photo: as a LUT's row, with what only it has. */
export function LookStrip(props: StripProps) {
  const t = useAgTranslation();
  const { canMake, kind, name, onMake, onStrength, onTakeOff, strength, swatch, there } = props;
  return (
    <div className="agm-onphoto-item">
      <div className="agm-look agm-on">
        <span className="agm-swatch" style={swatch ? { backgroundImage: `url(${swatch})` } : undefined} />
        <span className="agm-look-name">{name}</span>
        <span className="agm-kind">{kind}</span>
        <button className="agm-btn" onClick={onTakeOff} data-tooltip={t('looksTakeOff')}>
          <X size={12} />
        </button>
      </div>
      {there ? (
        <Slider
          label={t('looksStrength')}
          min={0}
          max={200}
          step={1}
          defaultValue={100}
          value={strength}
          onChange={(e) => onStrength(Number(e.target.value))}
        />
      ) : (
        <div className="agm-provider-missing">{t('looksProviderMissing')}</div>
      )}
      {canMake && (
        <div className="agm-provider-tools">
          <button onClick={onMake}>{t(there ? 'looksProviderRemake' : 'looksProviderMakeAgain')}</button>
        </div>
      )}
    </div>
  );
}

/** The look on the open photo, read from its edit and its file. */
export function OnPhotoLook({
  photo,
  onStrength,
  onTakeOff,
}: {
  photo: string;
  onStrength: (value: number) => void;
  onTakeOff: () => void;
}) {
  const t = useAgTranslation();
  const adjustments = useEditorStore((s) => s.adjustments);
  const { offer, filesMade, keepLook } = useProviderLooks();
  const on = lookOf(adjustments);
  const camera = on?.camera ?? '';
  const look = on?.look ?? '';
  const optionsKey = JSON.stringify(on?.options ?? {});
  const ownWbKey = JSON.stringify(on?.ownWb ?? null);
  const [there, setThere] = useState(true);

  useEffect(() => {
    let current = true;
    const [options, ownWb] = [JSON.parse(optionsKey), JSON.parse(ownWbKey)];
    ag('provider_look_file', { path: photo, camera, look, options, ownWb })
      .then((found) => current && setThere(found))
      .catch(() => current && setThere(false));
    return () => {
      current = false;
    };
  }, [photo, camera, look, optionsKey, ownWbKey, filesMade]);

  const file = offer?.photo?.files.find((f) => f.camera === camera && f.look === look);
  const swatches = useFileSwatches(file ? [file] : [], photo, true);
  const offered = offer?.looks.find((l) => l.id === look);
  const lookOptions = on?.options ?? {};
  const listed =
    !!offered &&
    !!offer?.cameras.some((c) => c.id === camera) &&
    sameOptions(offeredOptions(offer.options, lookOptions), lookOptions);

  return (
    <LookStrip
      canMake={listed}
      kind={offer?.cameras.find((c) => c.id === camera)?.name ?? t('looksProviderKind')}
      name={adjustments.lutName ?? ''}
      onMake={() => offered && keepLook([photo], offered, camera, lookOptions, true, on?.ownWb)}
      onStrength={onStrength}
      onTakeOff={onTakeOff}
      strength={adjustments.lutIntensity ?? 100}
      swatch={file && swatches[file.path]}
      there={there}
    />
  );
}
