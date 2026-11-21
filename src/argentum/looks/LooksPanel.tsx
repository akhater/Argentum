/**
 * The Looks tab: presets, LUTs and film looks in one list. Ours.
 *
 * With a provider set, its looks have a section above the lists, whose switch
 * turns the lists off (ProviderLooks.tsx).
 *
 * Search, Import, and chips for All, Favourites, Presets and LUTs; then the
 * LUTs suggested for the open photo, learned from the LUTs put on photos
 * before (suggestions.ts); then what
 * is on the photo, with a LUT's Strength and a preset's Amount; then the
 * groups: Presets (those that come with the app,
 * RAWmakase's, by group), My presets and My LUTs. Pointing at a look shows it
 * on the photo; a click puts it on, a click on the one on takes it off; the
 * star keeps it in Favourites. In the grid a click puts the look on every
 * selected photo at once, with an Undo in the note that says so.
 *
 * Save makes a preset of the open photo's settings (their Configure dialog) or
 * a LUT file of them (the Export tab, set to CUBE). A preset of the user's has
 * their whole menu on a right-click: Configure (its name and what it holds),
 * Update with the current settings, Duplicate, Move to a folder, Export and
 * Delete; a folder of them Rename, Export and Delete.
 *
 * Ctrl-click and Shift-click pick several of the user's LUTs, or several of
 * their presets, as in the photo grid; a right-click on them moves or deletes
 * them all, and Esc lets them go. Folders are listed A to Z.
 *
 * Community presets, opened over the photo, leaves the tab working on that
 * photo; a change to it goes back to the photo first (community.ts).
 *
 * Every part is today's: a LUT goes on and previews as their handleLutSelect
 * and setLutPreviewOverride do it, with the mark that feeds a film look what
 * its file declares (filmLookInput.ts), swatches come from their
 * generate_lut_previews and generate_preset_preview, the user's presets from
 * their usePresets and import from their commands. A preset's Amount follows
 * presetAmount.ts. The Effects section's LUT block and today's presets panel
 * did these separately; the Modern window shows this instead of both.
 */

import { type MouseEvent, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { ChevronDown, ChevronRight, Download, MoreHorizontal, Plus, Search, Star, X } from 'lucide-react';
import clsx from 'clsx';
import { toast } from 'react-toastify';
import { type AutoMarker, settleAutoWhiteBalance } from '../autoWhiteBalance';
import Slider from '../../components/ui/Slider';
import ConfigurePresetModal from '../../components/modals/ConfigurePresetModal';
import CreateFolderModal from '../../components/modals/CreateFolderModal';
import RenameFolderModal from '../../components/modals/RenameFolderModal';
import { Invokes, OPTION_SEPARATOR, Panel } from '../../components/ui/AppProperties';
import { useLibraryStore } from '../../store/useLibraryStore';
import { globalImageCache } from '../../utils/ImageLRUCache';
import { useContextMenu } from '../../context/ContextMenuContext';
import { useEditorActions } from '../../hooks/useEditorActions';
import { usePresets, type UserPreset } from '../../hooks/usePresets';
import type { Preset } from '../../components/ui/AppProperties';

type Modal =
  | { kind: 'save' }
  | { kind: 'configure'; preset: Preset }
  | { kind: 'newFolder' }
  | { kind: 'renameFolder'; id: string; name: string }
  | { kind: 'newLutFolder' }
  | { kind: 'renameLutFolder'; id: string; name: string }
  | null;
import { useEditorStore } from '../../store/useEditorStore';
import { useUIStore } from '../../store/useUIStore';
import { type Adjustments, getActiveTools, INITIAL_ADJUSTMENTS, setToolActive } from '../../utils/adjustments';
import { type AgKey, useAgTranslation } from '../locales';
import { leaveCommunity, openCommunity, useCommunity, useCommunityOverPhoto } from './community';
import { lutKeys } from './filmLookInput';
import { presetAt } from './presetAmount';
import { lookOf, lookOnto, PROVIDER_LOOK_PATH, providerLookOff } from './providerLook';
import ProviderLooks, { OnPhotoLook } from './ProviderLooks';
import { regularLooksAreOff, restoreLook, restoreLooks, useProviderLooks } from './providerLooksStore';
import { type LutEntry, useLooks } from './looksStore';
import { asksOf, withOutlines, withoutOutlines } from '../selection/foundAgain';
import { findMasksAgain } from '../selection/findAgain';
import { type Light, MIN_PICKS, SHOWN, type Suggested, type Suggestion, useSuggested } from './suggestions';

type Kind = 'preset' | 'lut';
type Chip = 'all' | 'favourites' | 'presets' | 'luts';

interface Look {
  id: string;
  name: string;
  kind: Kind;
  lut?: LutEntry;
  preset?: Partial<Adjustments>;
  /** A preset of the user's: its record, for their menu. */
  own?: Preset;
}

interface Group {
  id: string;
  name: string;
  looks: Look[];
  folders?: Group[];
}

const CHIPS: { id: Chip; label: AgKey }[] = [
  { id: 'all', label: 'looksAll' },
  { id: 'favourites', label: 'looksFavourites' },
  { id: 'presets', label: 'looksPresets' },
  { id: 'luts', label: 'looksLuts' },
];

const KIND_LABEL: Record<Kind, AgKey> = { preset: 'looksKindPreset', lut: 'looksKindLut' };

const LUT_FILES = ['cube', '3dl', 'png', 'jpg', 'jpeg', 'tiff'];
const PRESET_FILES = ['rrpreset', 'xmp', 'lrtemplate'];

/** How long the pointer rests on a look before the photo shows it. */
const PREVIEW_AFTER_MS = 120;
/** How long the hint says a pick was remembered. */
const NOTE_FOR_MS = 2600;
const LIGHT_LABEL: Record<Light, AgKey> = { lamp: 'looksLightLamp', daylight: 'looksLightDaylight' };
const SWATCH_PX = 72;
const UNCATEGORIZED = 'uncategorized';

/** Portrait2 before Portrait10; looks and folders alike. */
const byName = (a: { name?: string }, b: { name?: string }) =>
  (a.name ?? '').localeCompare(b.name ?? '', undefined, { numeric: true, sensitivity: 'base' });

/** Only the user's own LUTs and presets can move or go. */
const movable = (look: Look) => !!look.own || (!!look.lut && !look.lut.isBuiltIn);

/** Their list without the given presets, and those presets in list order. */
function takePresets(items: UserPreset[], ids: Set<string>): { kept: UserPreset[]; taken: Preset[] } {
  const taken: Preset[] = [];
  const kept = items.flatMap((item): UserPreset[] => {
    if (item.preset) {
      if (!ids.has(item.preset.id)) return [item];
      taken.push(item.preset);
      return [];
    }
    if (item.folder) {
      const children = item.folder.children as Preset[];
      taken.push(...children.filter((p) => ids.has(p.id)));
      return [{ folder: { ...item.folder, children: children.filter((p) => !ids.has(p.id)) } }];
    }
    return [item];
  });
  return { kept, taken };
}

const lutLook = (entry: LutEntry): Look => ({
  id: entry.path,
  name: entry.name.replace(/\.[^.]+$/, ''),
  kind: 'lut',
  lut: entry,
});

const presetLook = (preset: Preset): Look => ({
  id: preset.id,
  name: preset.name,
  kind: 'preset',
  preset: preset.adjustments,
  own: preset,
});

/** Their list: presets at the top level as `{ preset }`, folders as `{ folder }` holding plain presets. */
function userGroups(items: UserPreset[]): { root: Look[]; folders: Group[] } {
  const root = items.flatMap((item) => (item.preset ? [presetLook(item.preset)] : []));
  const folders = items.flatMap((item) =>
    item.folder
      ? [
          {
            id: `folder:${item.folder.id}`,
            name: item.folder.name ?? '',
            looks: ((item.folder.children ?? []) as Preset[]).map(presetLook),
          },
        ]
      : [],
  );
  return { root, folders };
}

export default function LooksPanel({ editing }: { editing: boolean }) {
  const t = useAgTranslation();
  const { showContextMenu } = useContextMenu();
  const { setAdjustments: setPhotoAdjustments } = useEditorActions();
  const overCommunity = useCommunityOverPhoto();
  /** Every change to the photo; over Community presets it goes back to the photo first, so the change shows. */
  const setAdjustments: typeof setPhotoAdjustments = (change) => {
    if (overCommunity) leaveCommunity();
    setPhotoAdjustments(change);
  };
  const adjustments = useEditorStore((s) => s.adjustments);
  const photo = useEditorStore((s) => (editing || overCommunity ? s.selectedImage : null));
  const setEditor = useEditorStore((s) => s.setEditor);
  const { builtin, luts, libraries, favourites, applied, load, refreshLuts, toggleFavourite, setApplied } = useLooks();
  const regularOff = useProviderLooks(regularLooksAreOff);
  const cancelProviderLook = useProviderLooks((s) => s.cancel);
  const {
    presets: userItems,
    importPresetsFromFiles,
    deleteItem,
    addPreset,
    addFolder,
    configurePreset,
    overwritePreset,
    duplicatePreset,
    movePreset,
    renameItem,
    exportPresetsToFile,
    refreshPresets,
  } = usePresets(adjustments);
  const [modal, setModal] = useState<Modal>(null);
  const [chip, setChip] = useState<Chip>('all');
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState<Set<string>>(() => new Set(['suggested', 'presets', 'mine', 'luts', 'lib:film']));
  const hover = useRef<number | undefined>(undefined);
  const { suggested, lutWentOn, lutCameOff, setLearning, forget } = useSuggested(photo?.path ?? null, !!photo?.isReady);
  /** What the hint says for a moment after a pick, in place of the usual hint. */
  const [note, setNote] = useState<string | null>(null);
  const noteTimer = useRef<number | undefined>(undefined);
  /** Looks picked with Ctrl or Shift, all of one kind; Shift runs from the anchor. */
  const [picked, setPicked] = useState<string[]>([]);
  const anchor = useRef<string | null>(null);

  useEffect(() => {
    load();
  }, [load]);

  // A preset saved on Community presets is in the user's list now: read it
  // again, and show My presets, where its Community folder is.
  const communitySaves = useCommunity((s) => s.saved);
  const seenSaves = useRef(communitySaves);
  useEffect(() => {
    if (communitySaves === seenSaves.current) return;
    seenSaves.current = communitySaves;
    refreshPresets();
    setOpen((was) => new Set([...was, 'mine']));
  }, [communitySaves, refreshPresets]);

  // A preset on another photo is not this photo's.
  const onPhoto = applied && photo && applied.photo === photo.path ? applied : null;
  useEffect(() => {
    if (applied && photo && applied.photo !== photo.path) setApplied(null);
  }, [applied, photo, setApplied]);

  useEffect(
    () => () => {
      window.clearTimeout(hover.current);
      window.clearTimeout(noteTimer.current);
    },
    [],
  );

  const showNote = (text: string) => {
    window.clearTimeout(noteTimer.current);
    setNote(text);
    noteTimer.current = window.setTimeout(() => setNote(null), NOTE_FOR_MS);
  };

  const sections = useMemo(() => {
    const mine = userGroups(userItems);
    // The shipped looks are named with where they come from: these presets are
    // RAWmakase's (resources/presets/rawmakase), the film LUTs spektrafilm's.
    // The user's own stay unnamed, under My presets.
    const presets: Group = {
      id: 'presets',
      name: t('looksGroupPresets'),
      looks: [],
      folders: builtin
        .map((group) => ({
          id: `builtin:${group.name}`,
          name: group.name,
          looks: group.presets.map((p) => ({
            id: p.id,
            name: p.name,
            kind: 'preset' as const,
            preset: p.adjustments as Partial<Adjustments>,
          })),
        }))
        .sort(byName),
    };
    const own: Group = { id: 'mine', name: t('looksGroupMine'), looks: mine.root, folders: mine.folders.sort(byName) };
    // The film looks are LUTs that come with the app: a folder of LUTs like the user's.
    const lutGroup: Group = {
      id: 'luts',
      name: t('looksGroupLuts'),
      looks: [],
      folders: [
        {
          id: 'lib:film',
          name: t('looksGroupFilm'),
          looks: luts
            .filter((l) => l.isBuiltIn)
            .map(lutLook)
            .sort(byName),
        },
        // Their list may hold the uncategorized library itself: once, named Imported.
        ...[
          { id: UNCATEGORIZED, name: t('looksImported') },
          ...libraries.filter((library) => library.id !== UNCATEGORIZED),
        ].map((library) => ({
          id: `lib:${library.id}`,
          name: library.name,
          looks: luts
            .filter((l) => !l.isBuiltIn && (l.libraryId ?? UNCATEGORIZED) === library.id)
            .map(lutLook)
            .sort(byName),
        })),
      ]
        .filter((group) => group.looks.length > 0)
        .sort(byName),
    };
    return { presets, own, lutGroup };
  }, [builtin, luts, libraries, userItems, t]);

  const everyLook = useMemo(() => {
    const flat = (g: Group): Look[] => [...g.looks, ...(g.folders ?? []).flatMap(flat)];
    return [sections.presets, sections.own, sections.lutGroup].flatMap(flat);
  }, [sections]);

  // A picked look that was moved away or deleted drops out on its own.
  const pickedLooks = useMemo(() => everyLook.filter((l) => picked.includes(l.id)), [everyLook, picked]);

  // Esc lets them go before it does anything else (as onPhoto.tsx's handles).
  const anyPicked = pickedLooks.length > 0;
  useEffect(() => {
    if (!anyPicked) return;
    const ui = useUIStore.getState();
    const before = ui.customEscapeHandler;
    const handler = () => setPicked([]);
    ui.setCustomEscapeHandler(handler);
    return () => {
      const now = useUIStore.getState();
      if (now.customEscapeHandler === handler) now.setCustomEscapeHandler(before);
    };
  }, [anyPicked]);

  // ------------------------------------------------------------- actions

  const isOn = (look: Look) => (look.lut ? adjustments.lutPath === look.lut.path : onPhoto?.id === look.id);

  /** Back to the photo as it is: the pointer has left the list, or a look was clicked. */
  const stopPreview = () => {
    window.clearTimeout(hover.current);
    setEditor({ previewOverride: null });
  };

  /**
   * The pointer left one look. The look already on screen stays there until
   * the next one replaces it, so moving from look to look never flashes the
   * photo between them; only a look not yet shown is called off.
   */
  const leaveLook = () => window.clearTimeout(hover.current);

  const startPreview = (look: Look) => {
    // Under Community presets the photo is out of sight.
    if (!photo || overCommunity) return;
    window.clearTimeout(hover.current);
    hover.current = window.setTimeout(() => {
      const now = useEditorStore.getState().adjustments;
      if (look.lut) {
        setEditor({
          previewOverride: { ...now, ...providerLookOff(now), ...lutKeys(look.lut.path, look.lut.isBuiltIn) },
        });
      } else if (look.preset) {
        const preset = withoutOutlines(look.preset, asksOf(look.preset.masks));
        setEditor({ previewOverride: { ...now, ...presetAt(now, lookOnto(now, preset), 100) } });
      }
    }, PREVIEW_AFTER_MS);
  };

  /** The LUT onto the open photo, at full strength. */
  const putOnLut = async (lut: LutEntry) => {
    if (photo) cancelProviderLook(photo.path);
    try {
      const { size } = await invoke<{ size: number }>('load_and_parse_lut', { path: lut.path });
      setAdjustments((prev: Adjustments) => ({
        ...prev,
        ...providerLookOff(prev),
        ...lutKeys(lut.path, lut.isBuiltIn),
        lutSize: size,
        lutIntensity: 100,
        activeTools: setToolActive(getActiveTools(prev), 'lut', true),
      }));
      tellPick(lutWentOn(lut.path), 'looksSuggestRemembered');
    } catch (err) {
      toast.error(`Failed to load LUT: ${err}`);
    }
  };

  const clearLut = () => {
    if (photo) cancelProviderLook(photo.path);
    setAdjustments((prev: Adjustments) => ({
      ...prev,
      ...providerLookOff(prev),
      lutPath: null,
      lutName: null,
      lutData: null,
      lutSize: 0,
      lutIntensity: 100,
      lutIsSceneReferred: false,
    }));
    tellPick(lutCameOff(), 'looksSuggestForgotOne');
  };

  /** Say in the hint that a pick was remembered or forgotten, while learning. */
  const tellPick = (answer: Promise<Suggested | null>, key: AgKey) =>
    answer.then((now) => {
      if (now?.learning)
        showNote(t(key).replace('{light}', t(LIGHT_LABEL[now.light])).replace('{count}', String(now.picks)));
    });

  const takeOffPreset = () => {
    if (!onPhoto) return;
    setAdjustments((prev: Adjustments) => ({ ...prev, ...presetAt(onPhoto.before, onPhoto.adjustments, 0) }));
    setApplied(null);
  };

  /**
   * In the grid: the look onto every selected photo, as their paste does
   * (ApplyAdjustmentsToPaths), each photo's settings it changes kept first so
   * the note's Undo can put them back.
   */
  const applyToSelection = async (look: Look) => {
    const { multiSelectedPaths, libraryActivePath } = useLibraryStore.getState();
    const paths = multiSelectedPaths.length > 0 ? multiSelectedPaths : libraryActivePath ? [libraryActivePath] : [];
    if (paths.length === 0) {
      toast.info(t('looksSelectFirst'));
      return;
    }
    try {
      let changes: Partial<Adjustments>;
      const asks = asksOf(look.preset?.masks);
      if (look.lut) {
        const { size } = await invoke<{ size: number }>('load_and_parse_lut', { path: look.lut.path });
        changes = { ...lutKeys(look.lut.path, look.lut.isBuiltIn), lutSize: size, lutIntensity: 100 };
      } else if (look.preset) {
        changes = withoutOutlines(look.preset, asks);
      } else return;
      const before = await Promise.all(
        paths.map(async (path) => {
          const meta = await invoke<{ adjustments?: Partial<Adjustments> | null }>('load_metadata', { path }).catch(
            () => null,
          );
          const was = meta?.adjustments ?? {};
          const own = lookOnto(was, changes);
          const keys = Object.keys(own) as (keyof Adjustments)[];
          return {
            path,
            own,
            values: Object.fromEntries(keys.map((k) => [k, was[k] ?? INITIAL_ADJUSTMENTS[k] ?? null])),
          };
        }),
      );
      paths.forEach((p) => globalImageCache.delete(p));
      if (lookOf(changes)) restoreLooks(paths, changes);
      // In one call unless a photo's own values differ.
      const same = new Map<string, { paths: string[]; adjustments: Partial<Adjustments> }>();
      for (const { path, own } of before) {
        const key = JSON.stringify(own);
        const group = same.get(key);
        if (group) group.paths.push(path);
        else same.set(key, { paths: [path], adjustments: own });
      }
      await Promise.all([...same.values()].map((group) => invoke(Invokes.ApplyAdjustmentsToPaths, group)));
      // Its AI masks find each photo's own subject, sky, words or faces.
      void findMasksAgain(paths, asks, setAdjustments);
      // A preset kept with Auto white balance: each photo measured for its own light.
      const auto = (changes as { whiteBalanceAuto?: AutoMarker }).whiteBalanceAuto;
      if (auto?.photo)
        settleAutoWhiteBalance(
          paths.filter((p) => p !== auto.photo),
          auto.photo,
        );
      const undo = async () => {
        for (const { path, values } of before) {
          globalImageCache.delete(path);
          await invoke(Invokes.ApplyAdjustmentsToPaths, { paths: [path], adjustments: values });
        }
        toast.info(t('looksUndone'));
      };
      toast.success(
        <span className="flex items-center gap-3">
          {t('looksApplied').replace('{name}', look.name).replace('{count}', String(paths.length))}
          <button className="underline" onClick={undo}>
            {t('looksUndo')}
          </button>
        </span>,
      );
    } catch (err) {
      console.error('Looks: could not apply to the selection:', err);
      toast.error(String(err));
    }
  };

  const choose = (look: Look) => {
    stopPreview();
    if (!photo) {
      applyToSelection(look);
      return;
    }
    if (look.lut) {
      if (isOn(look)) clearLut();
      else putOnLut(look.lut);
      return;
    }
    if (!look.preset) return;
    if (isOn(look)) {
      takeOffPreset();
      return;
    }
    // One preset at a time: the one on comes off before the next goes on.
    const now = useEditorStore.getState().adjustments;
    const before = onPhoto ? { ...now, ...presetAt(onPhoto.before, onPhoto.adjustments, 0) } : now;
    const asks = asksOf(look.preset.masks);
    const preset = lookOnto(before, withoutOutlines(look.preset, asks));
    setAdjustments(() => ({ ...before, ...presetAt(before, preset, 100) }));
    setApplied({ photo: photo.path, id: look.id, name: look.name, adjustments: preset, before, amount: 100 });
    restoreLook(photo.path, preset);
    // Its AI masks find this photo's own; the Amount then mixes towards those.
    void findMasksAgain([photo.path], asks, setAdjustments, (found) => {
      const now = useLooks.getState().applied;
      if (now?.photo !== photo.path || now.id !== look.id || !now.adjustments.masks) return;
      setApplied({ ...now, adjustments: { ...now.adjustments, masks: withOutlines(now.adjustments.masks, found) } });
    });
  };

  const changeAmount = (amount: number) => {
    if (!onPhoto) return;
    setAdjustments((prev: Adjustments) => ({ ...prev, ...presetAt(onPhoto.before, onPhoto.adjustments, amount) }));
    setApplied({ ...onPhoto, amount });
  };

  const importLooks = async () => {
    const chosen = await openDialog({
      multiple: true,
      title: t('looksImportTip'),
      filters: [
        { name: t('looksImportTip'), extensions: [...LUT_FILES, ...PRESET_FILES] },
        { name: t('looksLuts'), extensions: LUT_FILES },
        { name: t('looksPresets'), extensions: PRESET_FILES },
      ],
    });
    const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
    const ext = (p: string) => p.split('.').pop()?.toLowerCase() ?? '';
    const lutPaths = paths.filter((p) => LUT_FILES.includes(ext(p)));
    const presetPaths = paths.filter((p) => PRESET_FILES.includes(ext(p)));
    try {
      if (lutPaths.length) {
        await invoke('import_luts', { sourcePaths: lutPaths, libraryId: UNCATEGORIZED });
        await refreshLuts();
      }
      if (presetPaths.length) {
        const { failures } = await importPresetsFromFiles(presetPaths);
        if (failures.length) toast.error(t('looksImportFailed'));
      }
    } catch (err) {
      console.error('Looks: import failed:', err);
      toast.error(t('looksImportFailed'));
    }
  };

  const exportItem = async (item: UserPreset, name: string) => {
    const filePath = await saveDialog({
      defaultPath: `${name}.rrpreset`.replace(/[<>:"/\\|?*]/g, '_'),
      filters: [{ name: t('looksPresets'), extensions: ['rrpreset'] }],
    });
    if (filePath) await exportPresetsToFile([item], filePath).catch(() => toast.error(t('looksExportFailed')));
  };

  const folders = userItems.flatMap((item) => (item.folder ? [item.folder] : [])).sort(byName);

  const lookMenu = (event: MouseEvent, look: Look) => {
    event.preventDefault();
    if (pickedLooks.length > 1 && picked.includes(look.id)) {
      pickedMenu(event, pickedLooks);
      return;
    }
    // A right-click elsewhere is about that look alone.
    setPicked([]);
    const starred = favourites.includes(look.id);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- their menu's option type is not exported
    const options: any[] = [
      { label: t(starred ? 'looksUnstar' : 'looksStar'), onClick: () => toggleFavourite(look.id) },
    ];
    const own = look.own;
    if (own) {
      options.push(
        { type: OPTION_SEPARATOR },
        { label: t('looksConfigure'), onClick: () => setModal({ kind: 'configure', preset: own }) },
        { label: t('looksOverwrite'), disabled: !photo, onClick: () => overwritePreset(own.id) },
        { label: t('looksDuplicate'), onClick: () => duplicatePreset(own.id) },
        {
          label: t('looksMoveTo'),
          submenu: [
            { label: t('looksTopLevel'), onClick: () => movePreset(own.id, null) },
            ...folders.map((folder) => ({ label: folder.name, onClick: () => movePreset(own.id, folder.id ?? null) })),
          ],
        },
        { label: t('looksExport'), onClick: () => exportItem({ preset: own }, own.name) },
        { type: OPTION_SEPARATOR },
        { label: t('looksDelete'), isDestructive: true, onClick: () => deleteItem(own.id) },
      );
    }
    if (!own && look.preset) {
      const preset = look.preset;
      options.push(
        { type: OPTION_SEPARATOR },
        {
          label: t('looksCopyToMine'),
          onClick: () => copyToMine({ preset: { id: crypto.randomUUID(), name: look.name, adjustments: preset } }),
        },
      );
    }
    if ((!own && look.preset) || look.lut?.isBuiltIn) {
      options.push({ type: OPTION_SEPARATOR }, { label: t('looksComesWithApp'), disabled: true });
    }
    if (look.lut && !look.lut.isBuiltIn) {
      const path = look.lut.path;
      const here = look.lut.libraryId ?? UNCATEGORIZED;
      options.push({ type: OPTION_SEPARATOR });
      options.push({
        label: t('looksMoveTo'),
        submenu: lutFolders
          .filter((library) => library.id !== here)
          .map((library) => ({
            label: library.name,
            onClick: () =>
              invoke('set_lut_library', { path, libraryId: library.id })
                .then(refreshLuts)
                .catch((err) => toast.error(String(err))),
          })),
      });
      options.push({
        label: t('looksDelete'),
        isDestructive: true,
        onClick: () =>
          invoke('remove_lut', { path })
            .then(refreshLuts)
            .catch(() => toast.error(String(path))),
      });
    }
    showContextMenu(event.clientX, event.clientY, options);
  };

  /** The picked looks, all LUTs or all presets: Move them to a folder, or Delete them. */
  const pickedMenu = (event: MouseEvent, looks: Look[]) => {
    const luts = looks[0].kind === 'lut';
    const what = t(luts ? 'looksSomeLuts' : 'looksSomePresets').replace('{count}', String(looks.length));
    const targets = luts
      ? lutFolders
          .filter((library) => !looks.every((l) => (l.lut?.libraryId ?? UNCATEGORIZED) === library.id))
          .map((library) => ({ label: library.name, onClick: () => moveLuts(looks, library.id) }))
      : [
          { label: t('looksTopLevel'), onClick: () => movePresets(looks, null) },
          ...folders.map((folder) => ({ label: folder.name, onClick: () => movePresets(looks, folder.id ?? null) })),
        ];
    showContextMenu(event.clientX, event.clientY, [
      { label: t('looksMoveSome').replace('{what}', what), submenu: targets },
      { type: OPTION_SEPARATOR },
      {
        label: t('looksDeleteSome').replace('{what}', what),
        isDestructive: true,
        onClick: () =>
          useUIStore.getState().setUI({
            confirmModalState: {
              isOpen: true,
              title: t('looksDeleteSomeAsk').replace('{what}', what),
              message: t('looksDeleteSomeNote'),
              confirmText: t('looksDelete'),
              confirmVariant: 'destructive',
              onConfirm: () => deleteLooks(looks),
            },
          }),
      },
    ]);
  };

  /** One at a time: each move rewrites their library manifest. */
  const moveLuts = async (looks: Look[], libraryId: string) => {
    setPicked([]);
    try {
      for (const look of looks) {
        if (look.lut && (look.lut.libraryId ?? UNCATEGORIZED) !== libraryId)
          await invoke('set_lut_library', { path: look.lut.path, libraryId });
      }
    } catch (err) {
      toast.error(String(err));
    }
    await refreshLuts();
  };

  /** Into a folder of the user's, or the top level for null; those already there stay where they are. */
  const movePresets = (looks: Look[], folderId: string | null) => {
    setPicked([]);
    const already = new Set(
      folderId === null
        ? userItems.flatMap((item) => (item.preset ? [item.preset.id] : []))
        : ((userItems.find((item) => item.folder?.id === folderId)?.folder?.children ?? []) as Preset[]).map(
            (p) => p.id,
          ),
    );
    const { kept, taken } = takePresets(userItems, new Set(looks.map((l) => l.id).filter((id) => !already.has(id))));
    const moved =
      folderId === null
        ? [...kept, ...taken.map((preset) => ({ preset }))]
        : kept.map((item) =>
            item.folder?.id === folderId
              ? { folder: { ...item.folder, children: [...item.folder.children, ...taken] } }
              : item,
          );
    savePresetList(moved);
  };

  const deleteLooks = async (looks: Look[]) => {
    setPicked([]);
    if (looks[0]?.kind === 'preset') {
      savePresetList(takePresets(userItems, new Set(looks.map((l) => l.id))).kept);
      return;
    }
    try {
      for (const look of looks) if (look.lut) await invoke('remove_lut', { path: look.lut.path });
    } catch (err) {
      toast.error(String(err));
    }
    await refreshLuts();
  };

  /** The user's presets as a whole, as copyToMine saves them: their hook's own calls change one at a time. */
  const savePresetList = (items: UserPreset[]) =>
    invoke(Invokes.SavePresets, { presets: items })
      .then(refreshPresets)
      .catch((err) => toast.error(String(err)));

  /** A copy of what comes with the app, as the user's own: it can be renamed and changed. */
  const copyToMine = (item: UserPreset) =>
    invoke(Invokes.SavePresets, { presets: [...userItems, item] })
      .then(() => {
        refreshPresets();
        setOpen((was) => new Set([...was, 'mine']));
      })
      .catch((err) => toast.error(String(err)));

  /**
   * Imported is their uncategorized library, which cannot be renamed: its LUTs
   * move into a new folder with the name instead, and the next import starts
   * Imported again.
   */
  const renameImported = async (name: string) => {
    const library = await invoke<{ id: string }>('create_lut_library', { name });
    for (const lut of luts.filter((l) => !l.isBuiltIn && (l.libraryId ?? UNCATEGORIZED) === UNCATEGORIZED)) {
      await invoke('set_lut_library', { path: lut.path, libraryId: library.id });
    }
  };

  /** The LUT folders a LUT can be in: Imported and the user's, A to Z. */
  const lutFolders = [
    { id: UNCATEGORIZED, name: t('looksImported') },
    ...libraries.filter((library) => library.id !== UNCATEGORIZED),
  ].sort(byName);

  const lutFolderMenu = (event: MouseEvent, groupId: string) => {
    event.preventDefault();
    event.stopPropagation();
    if (groupId === 'lib:film') {
      showContextMenu(event.clientX, event.clientY, [{ label: t('looksComesWithApp'), disabled: true }]);
      return;
    }
    if (groupId === `lib:${UNCATEGORIZED}`) {
      showContextMenu(event.clientX, event.clientY, [
        {
          label: t('looksRenameFolder'),
          onClick: () => setModal({ kind: 'renameLutFolder', id: UNCATEGORIZED, name: t('looksImported') }),
        },
      ]);
      return;
    }
    const library = libraries.find((l) => `lib:${l.id}` === groupId);
    if (!library) return;
    showContextMenu(event.clientX, event.clientY, [
      {
        label: t('looksRenameFolder'),
        onClick: () => setModal({ kind: 'renameLutFolder', id: library.id, name: library.name }),
      },
      { type: OPTION_SEPARATOR },
      {
        label: t('looksDelete'),
        isDestructive: true,
        onClick: () =>
          invoke('delete_lut_library', { libraryId: library.id })
            .then(refreshLuts)
            .catch((err) => toast.error(String(err))),
      },
    ]);
  };

  const folderMenu = (event: MouseEvent, groupId: string) => {
    const folder = folders.find((f) => `folder:${f.id}` === groupId);
    if (!folder?.id) return;
    event.preventDefault();
    event.stopPropagation();
    const id = folder.id;
    showContextMenu(event.clientX, event.clientY, [
      { label: t('looksRenameFolder'), onClick: () => setModal({ kind: 'renameFolder', id, name: folder.name ?? '' }) },
      { label: t('looksExport'), onClick: () => exportItem({ folder }, folder.name ?? 'presets') },
      { type: OPTION_SEPARATOR },
      { label: t('looksDelete'), isDestructive: true, onClick: () => deleteItem(id) },
    ]);
  };

  /** A group that comes with the app: copied whole into My presets, as a folder of the user's. */
  const builtinFolderMenu = (event: MouseEvent, group: Group) => {
    event.preventDefault();
    event.stopPropagation();
    showContextMenu(event.clientX, event.clientY, [
      {
        label: t('looksCopyToMine'),
        onClick: () =>
          copyToMine({
            folder: {
              id: crypto.randomUUID(),
              name: group.name,
              children: group.looks.map((look) => ({
                id: crypto.randomUUID(),
                name: look.name,
                adjustments: look.preset ?? {},
              })),
            },
          }),
      },
      { type: OPTION_SEPARATOR },
      { label: t('looksComesWithApp'), disabled: true },
    ]);
  };

  /** Every folder answers a right-click; the sections above them do not. */
  const groupMenu = (event: MouseEvent, group: Group) => {
    if (group.id.startsWith('folder:')) folderMenu(event, group.id);
    else if (group.id.startsWith('lib:')) lutFolderMenu(event, group.id);
    else if (group.id.startsWith('builtin:')) builtinFolderMenu(event, group);
  };

  const saveMenu = (event: MouseEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    showContextMenu(rect.left, rect.bottom, [
      { label: t('looksSavePreset'), disabled: !photo, onClick: () => setModal({ kind: 'save' }) },
      {
        label: t('looksSaveLut'),
        disabled: !photo,
        onClick: () => {
          // The Export tab, set to CUBE (ExportPanel.tsx reads this once), on the photo.
          useLooks.setState({ wantLutExport: true });
          if (overCommunity) leaveCommunity();
          useUIStore.getState().setUI({ activePanel: Panel.Export });
        },
      },
    ]);
  };

  const moreMenu = (event: MouseEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    showContextMenu(rect.left, rect.bottom, [
      { label: t('looksNewFolder'), onClick: () => setModal({ kind: 'newFolder' }) },
      { label: t('looksNewLutFolder'), onClick: () => setModal({ kind: 'newLutFolder' }) },
      { label: t('looksCommunity'), onClick: () => openCommunity(!!photo) },
    ]);
  };

  /** Suggested's ⋯: learning on or off, and forgetting every pick. */
  const suggestMenu = (event: MouseEvent) => {
    event.stopPropagation();
    if (!suggested) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const count = String(suggested.picks);
    showContextMenu(rect.left, rect.bottom, [
      {
        label: t(suggested.learning ? 'looksSuggestStop' : 'looksSuggestStart'),
        onClick: () => setLearning(!suggested.learning),
      },
      {
        label: t('looksSuggestForget').replace('{count}', count),
        isDestructive: true,
        disabled: suggested.picks === 0,
        onClick: () =>
          useUIStore.getState().setUI({
            confirmModalState: {
              isOpen: true,
              title: t('looksSuggestForgetAsk').replace('{count}', count),
              message: t('looksSuggestForgetNote'),
              confirmText: t('looksSuggestForgetYes'),
              confirmVariant: 'destructive',
              onConfirm: forget,
            },
          }),
      },
      ...(suggested.of > 0
        ? [
            { type: OPTION_SEPARATOR },
            {
              label: t('looksSuggestRight')
                .replace('{right}', String(suggested.right))
                .replace('{of}', String(suggested.of)),
              disabled: true,
            },
          ]
        : []),
    ]);
  };

  /** Why a LUT is suggested: how many photos like this one it was picked for. */
  const whyOf = (s: Suggestion) => {
    const similar = s.votes > 0;
    const count = similar ? s.votes : s.picks;
    const key: AgKey = similar
      ? count === 1
        ? 'looksSuggestWhy1'
        : 'looksSuggestWhyN'
      : count === 1
        ? 'looksSuggestElse1'
        : 'looksSuggestElseN';
    return t(key).replace('{count}', String(count)).replace('{light}', t(LIGHT_LABEL[s.light]));
  };

  // ------------------------------------------------------------- what shows

  const needle = query.trim().toLowerCase();
  const matches = (look: Look) => !needle || look.name.toLowerCase().includes(needle);
  const flatList = chip === 'favourites' || !!needle;
  const shownFlat = flatList
    ? everyLook.filter((l) => matches(l) && (chip !== 'favourites' || favourites.includes(l.id)) && inChip(l, chip))
    : [];
  const shownSections = flatList
    ? []
    : chip === 'presets'
      ? [sections.presets, sections.own]
      : chip === 'luts'
        ? [sections.lutGroup]
        : [sections.presets, sections.own, sections.lutGroup];

  // What is on screen, in its order: swatches only for these (a group asks for
  // its own when it opens), and Shift picks a run of them.
  const visible = flatList
    ? shownFlat
    : shownSections.flatMap((section) =>
        open.has(section.id)
          ? [...(section.folders ?? []).flatMap((f) => (open.has(f.id) ? f.looks : [])), ...section.looks]
          : [],
      );
  const onPhotoLooks = everyLook.filter((l) => l.id === adjustments.lutPath || l.id === onPhoto?.id);
  // Suggested for this photo: the LUTs it still has, under the list's header
  // and above what is on the photo, so they stay put while one is tried.
  const showSuggested = !!photo && !!suggested && !flatList && chip !== 'presets';
  const suggestedLooks = (suggested?.suggestions ?? [])
    .flatMap((s) => {
      const entry = luts.find((l) => l.path === s.lut);
      return entry ? [{ look: lutLook(entry), why: s }] : [];
    })
    .slice(0, SHOWN);
  const shownSuggested = showSuggested && open.has('suggested') ? suggestedLooks.map((s) => s.look) : [];
  const swatchLooks = [...new Map([...visible, ...onPhotoLooks, ...shownSuggested].map((l) => [l.id, l])).values()];
  const swatches = useSwatches(swatchLooks, photo?.path ?? null, !!photo?.isReady);

  const toggle = (id: string) =>
    setOpen((was) => {
      const next = new Set(was);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  /**
   * A plain click puts the look on, as before. Ctrl adds or drops one of the
   * user's looks; Shift picks the run from the last one clicked, as Explorer
   * does. A look of the other kind starts the picking again.
   */
  const click = (event: MouseEvent, look: Look) => {
    const adding = event.ctrlKey || event.metaKey;
    if (!adding && !event.shiftKey) {
      setPicked([]);
      anchor.current = look.id;
      choose(look);
      return;
    }
    if (!movable(look)) return;
    const kept = adding ? pickedLooks.filter((l) => l.kind === look.kind).map((l) => l.id) : [];
    if (event.shiftKey) {
      const from = visible.findIndex((l) => l.id === anchor.current);
      const to = visible.findIndex((l) => l.id === look.id);
      const run = from === -1 ? [look] : visible.slice(Math.min(from, to), Math.max(from, to) + 1);
      setPicked([...new Set([...kept, ...run.filter((l) => movable(l) && l.kind === look.kind).map((l) => l.id)])]);
      if (from === -1) anchor.current = look.id;
      return;
    }
    setPicked(kept.includes(look.id) ? kept.filter((id) => id !== look.id) : [...kept, look.id]);
    anchor.current = look.id;
  };

  /** A look's row; a suggested one says why under its name. */
  const row = (look: Look, showKind = false, why?: string) => (
    <div
      key={look.id}
      className={clsx('agm-look', isOn(look) && 'agm-on', picked.includes(look.id) && 'agm-picked')}
      onClick={(e) => click(e, look)}
      onMouseEnter={() => startPreview(look)}
      onMouseLeave={leaveLook}
      onContextMenu={(e) => lookMenu(e, look)}
    >
      <span
        className="agm-swatch"
        style={swatches[look.id] ? { backgroundImage: `url(${swatches[look.id]})` } : undefined}
      />
      {why ? (
        <span className="agm-suggest-text">
          <span className="agm-look-name">{look.name}</span>
          <span className="agm-suggest-why">{why}</span>
        </span>
      ) : (
        <span className="agm-look-name">{look.name}</span>
      )}
      {showKind && <span className="agm-kind">{t(KIND_LABEL[look.kind])}</span>}
      <button
        className={clsx('agm-star', favourites.includes(look.id) && 'agm-on')}
        onClick={(e) => {
          e.stopPropagation();
          toggleFavourite(look.id);
        }}
        data-tooltip={t(favourites.includes(look.id) ? 'looksUnstar' : 'looksStar')}
      >
        <Star size={12} />
      </button>
    </div>
  );

  const groupHead = (group: Group, count: number, folder = false) => (
    <div
      className={folder ? 'agm-look-folder' : 'agm-sec-head'}
      onClick={() => toggle(group.id)}
      onContextMenu={(e) => groupMenu(e, group)}
    >
      {open.has(group.id) ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
      <span className="min-w-0 truncate">{group.name}</span>
      <span className="agm-count">{count}</span>
    </div>
  );

  const countOf = (g: Group): number => g.looks.length + (g.folders ?? []).reduce((n, f) => n + countOf(f), 0);

  const lutOn = adjustments.lutPath ? luts.find((l) => l.path === adjustments.lutPath) : undefined;

  return (
    <div className="agm-looks">
      <div className="agm-looks-head">
        <div className="agm-looks-row">
          <label className="agm-search">
            <Search size={13} />
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t('looksSearch')} />
            {query && (
              <button onClick={() => setQuery('')} aria-label={t('looksClear')}>
                <X size={12} />
              </button>
            )}
          </label>
          <button className="agm-btn agm-btn-box" onClick={importLooks} data-tooltip={t('looksImportTip')}>
            <Download size={13} />
            {t('looksImport')}
          </button>
          <button className="agm-btn agm-btn-box" onClick={saveMenu} data-tooltip={t('looksSaveTip')}>
            <Plus size={13} />
            {t('looksSave')}
          </button>
          <button className="agm-btn" onClick={moreMenu} data-tooltip={t('looksMore')}>
            <MoreHorizontal size={14} />
          </button>
        </div>
        <div className="agm-chips" role="tablist">
          {CHIPS.map((c) => (
            <button
              key={c.id}
              role="tab"
              aria-selected={chip === c.id}
              className={clsx('agm-chip', chip === c.id && 'agm-on')}
              onClick={() => setChip(c.id)}
            >
              {c.id === 'favourites' && <Star size={11} />}
              {t(c.label)}
            </button>
          ))}
        </div>
        <div className="agm-hint">
          {anyPicked
            ? t('looksPicked').replace('{count}', String(pickedLooks.length))
            : (note ?? t(photo ? 'looksHintPhoto' : 'looksHintGrid'))}
        </div>
      </div>

      <ProviderLooks photo={photo?.path ?? null} ready={!!photo?.isReady} onTakeOff={clearLut} />

      {showSuggested && suggested && (
        <div className={clsx('agm-suggest', regularOff && 'agm-looks-off')} onMouseLeave={stopPreview}>
          <div className="agm-sec-head" onClick={() => (suggested.learning ? toggle('suggested') : undefined)}>
            {suggested.learning && (open.has('suggested') ? <ChevronDown size={12} /> : <ChevronRight size={12} />)}
            <span className="min-w-0 truncate">{t(suggested.learning ? 'looksSuggested' : 'looksSuggestedOff')}</span>
            <span className="agm-sec-tools">
              {suggested.learning ? (
                <span className="agm-count" data-tooltip={t('looksSuggestAbout')}>
                  {t('looksSuggestPicks').replace('{count}', String(suggested.picks))}
                </span>
              ) : (
                <button
                  className="agm-btn"
                  onClick={(e) => {
                    e.stopPropagation();
                    setLearning(true);
                  }}
                >
                  {t('looksSuggestTurnOn')}
                </button>
              )}
              <button className="agm-btn" onClick={suggestMenu} data-tooltip={t('looksSuggestMenu')}>
                <MoreHorizontal size={14} />
              </button>
            </span>
          </div>
          {suggested.learning &&
            open.has('suggested') &&
            (suggestedLooks.length ? (
              suggestedLooks.map(({ look, why }) => row(look, false, whyOf(why)))
            ) : suggested.picks < MIN_PICKS ? (
              <div className="agm-suggest-cold">
                {t('looksSuggestCold')}
                <span className="agm-suggest-why">
                  <span className="agm-suggest-dots">
                    {Array.from({ length: MIN_PICKS }, (_, i) => (
                      <i key={i} className={clsx(i < suggested.picks && 'agm-on')} />
                    ))}
                  </span>
                  {t('looksSuggestColdNote').replace('{count}', String(MIN_PICKS))}
                </span>
              </div>
            ) : (
              <div className="agm-suggest-cold agm-suggest-why">{t('looksSuggestNone')}</div>
            ))}
        </div>
      )}

      {photo && (lutOn || adjustments.lutPath || onPhoto) && (
        <div className="agm-onphoto">
          <div className="agm-sec-head">{t('looksOnPhoto')}</div>
          {adjustments.lutPath === PROVIDER_LOOK_PATH && (
            <OnPhotoLook
              photo={photo.path}
              onStrength={(lutIntensity) => setAdjustments((prev: Adjustments) => ({ ...prev, lutIntensity }))}
              onTakeOff={clearLut}
            />
          )}
          {adjustments.lutPath && adjustments.lutPath !== PROVIDER_LOOK_PATH && (
            <div className="agm-onphoto-item">
              <div className="agm-look agm-on">
                <span
                  className="agm-swatch"
                  style={
                    swatches[adjustments.lutPath]
                      ? { backgroundImage: `url(${swatches[adjustments.lutPath]})` }
                      : undefined
                  }
                />
                <span className="agm-look-name">{(adjustments.lutName ?? '').replace(/\.[^.]+$/, '')}</span>
                <span className="agm-kind">{t('looksKindLut')}</span>
                <button className="agm-btn" onClick={clearLut} data-tooltip={t('looksTakeOff')}>
                  <X size={12} />
                </button>
              </div>
              <Slider
                label={t('looksStrength')}
                min={0}
                max={200}
                step={1}
                defaultValue={100}
                value={adjustments.lutIntensity ?? 100}
                onChange={(e) =>
                  setAdjustments((prev: Adjustments) => ({ ...prev, lutIntensity: Number(e.target.value) }))
                }
              />
            </div>
          )}
          {onPhoto && (
            <div className="agm-onphoto-item">
              <div className="agm-look agm-on">
                <span
                  className="agm-swatch"
                  style={swatches[onPhoto.id] ? { backgroundImage: `url(${swatches[onPhoto.id]})` } : undefined}
                />
                <span className="agm-look-name">{onPhoto.name}</span>
                <span className="agm-kind">{t('looksKindPreset')}</span>
                <button className="agm-btn" onClick={takeOffPreset} data-tooltip={t('looksTakeOff')}>
                  <X size={12} />
                </button>
              </div>
              <Slider
                label={t('looksAmount')}
                min={0}
                max={200}
                step={1}
                defaultValue={100}
                value={onPhoto.amount}
                suffix="%"
                onChange={(e) => changeAmount(Number(e.target.value))}
              />
            </div>
          )}
        </div>
      )}

      <div className={clsx('agm-looks-list', regularOff && 'agm-looks-off')} onMouseLeave={stopPreview}>
        {flatList ? (
          shownFlat.length ? (
            shownFlat.map((look) => row(look, true))
          ) : (
            <div className="agm-hint pt-3">
              {t(chip === 'favourites' && !needle ? 'looksNoFavourites' : 'looksNothing')}
            </div>
          )
        ) : (
          shownSections.map((section) => (
            <div key={section.id} className="agm-look-section">
              {groupHead(section, countOf(section))}
              {open.has(section.id) && (
                <>
                  {(section.folders ?? []).map((folder) => (
                    <div key={folder.id}>
                      {groupHead(folder, folder.looks.length, true)}
                      {open.has(folder.id) && <div className="agm-look-inner">{folder.looks.map((l) => row(l))}</div>}
                    </div>
                  ))}
                  {section.looks.map((l) => row(l))}
                </>
              )}
            </div>
          ))
        )}
      </div>
      <ConfigurePresetModal
        isOpen={modal?.kind === 'save' || modal?.kind === 'configure'}
        initialPreset={modal?.kind === 'configure' ? modal.preset : null}
        onClose={() => setModal(null)}
        onSave={(name, includeMasks, includeCropTransform, presetType) => {
          if (modal?.kind === 'configure') {
            configurePreset(modal.preset.id, name, includeMasks, includeCropTransform, presetType);
          } else {
            addPreset(name, null, includeMasks, includeCropTransform, presetType);
          }
          setModal(null);
        }}
      />
      <CreateFolderModal
        isOpen={modal?.kind === 'newLutFolder'}
        title={t('looksNewLutFolder')}
        onClose={() => setModal(null)}
        onSave={(name) => {
          invoke('create_lut_library', { name })
            .then(refreshLuts)
            .catch((err) => toast.error(String(err)));
          setModal(null);
        }}
      />
      <RenameFolderModal
        isOpen={modal?.kind === 'renameLutFolder'}
        currentName={modal?.kind === 'renameLutFolder' ? modal.name : ''}
        onClose={() => setModal(null)}
        onSave={(name) => {
          if (modal?.kind === 'renameLutFolder') {
            (modal.id === UNCATEGORIZED
              ? renameImported(name)
              : invoke('rename_lut_library', { libraryId: modal.id, name })
            )
              .then(refreshLuts)
              .catch((err) => toast.error(String(err)));
          }
          setModal(null);
        }}
      />
      <CreateFolderModal
        isOpen={modal?.kind === 'newFolder'}
        onClose={() => setModal(null)}
        onSave={(name) => {
          addFolder(name);
          setModal(null);
        }}
      />
      <RenameFolderModal
        isOpen={modal?.kind === 'renameFolder'}
        currentName={modal?.kind === 'renameFolder' ? modal.name : ''}
        onClose={() => setModal(null)}
        onSave={(name) => {
          if (modal?.kind === 'renameFolder') renameItem(modal.id, name);
          setModal(null);
        }}
      />
    </div>
  );
}

function inChip(look: Look, chip: Chip): boolean {
  if (chip === 'presets') return look.kind === 'preset';
  if (chip === 'luts') return look.kind === 'lut';
  return true;
}

/**
 * Swatches for the looks in open groups, on the open photo: LUTs through their
 * generate_lut_previews in one call, presets one at a time through their
 * generate_preset_preview, as today's presets panel does, on the app's
 * defaults. Kept for the photo until another one opens.
 */
function useSwatches(looks: Look[], photoPath: string | null, ready: boolean) {
  const [swatches, setSwatches] = useState<Record<string, string>>({});
  const forPhoto = useRef<string | null>(null);
  const asked = useRef<Set<string>>(new Set());
  const key = looks.map((l) => l.id).join('|');

  useEffect(() => {
    if (forPhoto.current !== photoPath) {
      forPhoto.current = photoPath;
      asked.current = new Set();
      setSwatches((was) => {
        Object.values(was).forEach((url) => url.startsWith('blob:') && URL.revokeObjectURL(url));
        return {};
      });
    }
  }, [photoPath]);

  useEffect(() => {
    if (!photoPath || !ready) return;
    const path = photoPath;
    const luts = looks.filter((l) => l.lut && !asked.current.has(l.id));
    const presets = looks.filter((l) => l.preset && !asked.current.has(l.id));
    luts.forEach((l) => asked.current.add(l.id));
    if (luts.length) {
      invoke<{ path: string; thumb: string | null }[]>('generate_lut_previews', {
        luts: luts.map((l) => ({ path: l.lut?.path, isBuiltIn: l.lut?.isBuiltIn })),
        size: SWATCH_PX,
      })
        .then((results) => {
          if (forPhoto.current !== path) return;
          setSwatches((was) => {
            const next = { ...was };
            results.forEach((r) => r.thumb && (next[r.path] = r.thumb));
            return next;
          });
        })
        .catch((err) => console.warn('Looks: LUT swatches:', err));
    }
    let stopped = false;
    (async () => {
      for (const look of presets) {
        if (stopped || forPhoto.current !== path) return;
        asked.current.add(look.id);
        try {
          const bytes: Uint8Array = await invoke('generate_preset_preview', {
            jsAdjustments: { ...INITIAL_ADJUSTMENTS, ...look.preset },
          });
          if (forPhoto.current !== path) return;
          const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: 'image/jpeg' }));
          setSwatches((was) => ({ ...was, [look.id]: url }));
        } catch {
          asked.current.delete(look.id);
          return;
        }
      }
    })();
    return () => {
      stopped = true;
    };
    // Keyed on which looks, not the array, which is new on every render.
  }, [key, photoPath, ready]);

  return swatches;
}
