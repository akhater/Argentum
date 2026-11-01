/**
 * The Modern window's top bar, which is also its title bar. Ours.
 *
 * Left to right: folder history, the path (click it or press Alt+D to type or
 * paste one), then what the view in the centre needs (the grid's sort, size
 * and view options, or the photo's undo, Before, Clipping, RGB and zoom), the
 * switch between the photo and the grid, the panel toggles, the Settings gear
 * and the window controls. It is the one region that never hides, so the gear
 * and the toggles are always there.
 *
 * Every control drives today's own state: the library store's sort and
 * filters, the app's view settings, the editor's history and Before, today's
 * zoom. Nothing here keeps state of its own but the path being typed.
 */

import { type ChangeEvent, type KeyboardEvent, useEffect, useRef, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';
import {
  ArrowLeft,
  ArrowRight,
  ChevronRight,
  Eye,
  Image as ImageIcon,
  LayoutGrid,
  Redo2,
  Settings,
  Undo2,
} from 'lucide-react';
import clsx from 'clsx';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useEditorStore } from '../../store/useEditorStore';
import { useSettingsStore } from '../../store/useSettingsStore';
import { useUIStore } from '../../store/useUIStore';
import { useEditorActions } from '../../hooks/useEditorActions';
import {
  EditedStatus,
  ExifOverlay,
  LibraryViewMode,
  RawStatus,
  SortDirection,
  ThumbnailAspectRatio,
  ThumbnailSize,
} from '../../components/ui/AppProperties';
import RgbReadoutButton from '../RgbReadoutButton';
import { useAgTranslation, type AgKey } from '../locales';
import VersionNote from './VersionNote';
import { crumbs } from './folders';
import { Menu, MenuHead, MenuItem, MenuSep } from './Menu';
import { SHOW_CHOICES, TYPE_CHOICES, activeFilterNames } from './shownFilters';
import WindowControls, { MacLights, usePlatform } from './WindowControls';
import { fileNameOf } from '../paths';

const PanelLeft = () => (
  <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16" />
  </svg>
);
const PanelBottom = () => (
  <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 15h18" />
  </svg>
);
const PanelRight = () => (
  <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M15 4v16" />
  </svg>
);
const ClipIcon = () => (
  <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6}>
    <circle cx="12" cy="12" r="8" />
    <path d="M12 4a8 8 0 0 1 0 16z" fill="currentColor" />
  </svg>
);

const SORTS: { key: string; label: AgKey }[] = [
  { key: 'name', label: 'mwSortName' },
  { key: 'date_taken', label: 'mwSortTaken' },
  { key: 'date', label: 'mwSortModified' },
  { key: 'edited', label: 'mwSortEdited' },
  { key: 'iso', label: 'mwSortIso' },
  { key: 'focal_length', label: 'mwSortFocal' },
  { key: 'aperture', label: 'mwSortAperture' },
  { key: 'shutter_speed', label: 'mwSortShutter' },
];

/** Today's three thumbnail sizes, as the size slider's three steps. */
const SIZES = [ThumbnailSize.Small, ThumbnailSize.Medium, ThumbnailSize.Large];

const ZOOMS = [0.5, 1, 2, 4];

interface TopBarProps {
  onBack(): void;
  onForward(): void;
  onOpenPath(path: string): void;
  onOpenPhoto(path: string): void;
  onBackToLibrary(): void;
  onZoom(zoom: number, fitToWindow?: boolean): void;
  thumbnailSize: ThumbnailSize;
  setThumbnailSize(size: ThumbnailSize): void;
  thumbnailAspectRatio: ThumbnailAspectRatio;
  setThumbnailAspectRatio(ratio: ThumbnailAspectRatio): void;
  libraryViewMode: LibraryViewMode;
  setLibraryViewMode(mode: LibraryViewMode): void;
}

export default function TopBar(props: TopBarProps) {
  const t = useAgTranslation();
  const os = usePlatform();
  const appSettings = useSettingsStore((s) => s.appSettings);
  const { activeView, isSettingsOpen, uiVisibility, setUI } = useUIStore(
    useShallow((s) => ({
      activeView: s.activeView,
      isSettingsOpen: s.isSettingsOpen,
      uiVisibility: s.uiVisibility,
      setUI: s.setUI,
    })),
  );
  const { currentFolderPath, libraryActivePath, navHistory, navIndex } = useLibraryStore(
    useShallow((s) => ({
      currentFolderPath: s.currentFolderPath,
      libraryActivePath: s.libraryActivePath,
      navHistory: s.navHistory,
      navIndex: s.navIndex,
    })),
  );
  const selectedPath = useEditorStore((s) => s.selectedImage?.path);
  const editing = activeView === 'editor' && !!selectedPath;

  const [typing, setTyping] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  // Alt+D types a path, as in Explorer.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && e.code === 'KeyD') {
        e.preventDefault();
        setTyping(currentFolderPath ?? '');
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [currentFolderPath]);

  useEffect(() => {
    if (typing !== null) input.current?.select();
  }, [typing !== null]);

  const pathKey = (e: KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === 'Enter' && typing?.trim()) {
      props.onOpenPath(typing.trim());
      setTyping(null);
    } else if (e.key === 'Escape') {
      setTyping(null);
    }
  };

  const fileName = selectedPath ? fileNameOf(selectedPath) : undefined;
  const folderCrumbs = currentFolderPath && !currentFolderPath.startsWith('Album: ') ? crumbs(currentFolderPath) : [];
  const toggle = (key: 'leftPanel' | 'filmstrip' | 'rightPanel') =>
    setUI((s) => ({ uiVisibility: { ...s.uiVisibility, [key]: !s.uiVisibility[key] } }));
  const canBack = navIndex > 0;
  const canForward = navIndex < navHistory.length - 1;
  const photoToOpen = activeView === 'library' && currentFolderPath ? libraryActivePath : null;
  const switchTip = t(editing ? 'mwToGrid' : currentFolderPath ? 'mwOpenPhoto' : 'mwOpenFolderFirst');

  return (
    <div className="agm-top" data-tauri-drag-region={appSettings?.decorations ? undefined : 'true'}>
      <div className="agm-top-left" data-tauri-drag-region>
        <MacLights os={appSettings?.decorations ? '' : os} />
        <button className="agm-btn" disabled={!canBack} onClick={props.onBack} data-tooltip={t('mwBack')}>
          <ArrowLeft size={16} />
        </button>
        <button className="agm-btn" disabled={!canForward} onClick={props.onForward} data-tooltip={t('mwForward')}>
          <ArrowRight size={16} />
        </button>
        {typing !== null ? (
          <input
            ref={input}
            className="agm-path-input"
            value={typing}
            spellCheck={false}
            onChange={(e: ChangeEvent<HTMLInputElement>) => setTyping(e.target.value)}
            onKeyDown={pathKey}
            onBlur={() => setTyping(null)}
          />
        ) : (
          <div
            className="agm-crumbs"
            data-tooltip={t('mwPathTip')}
            onClick={(e) => {
              if (e.target === e.currentTarget) setTyping(currentFolderPath ?? '');
            }}
          >
            {folderCrumbs.length === 0 && !isSettingsOpen && (
              <span className="agm-crumbs-hint" onClick={() => setTyping('')}>
                {t('mwChooseFolder')}
              </span>
            )}
            {folderCrumbs.map((c, i) => {
              const last = i === folderCrumbs.length - 1 && !editing && !isSettingsOpen;
              return (
                <span key={c.path} className="flex items-center min-w-0">
                  {i > 0 && <ChevronRight size={12} className="agm-crumb-sep" />}
                  <span className={clsx('agm-crumb', last && 'agm-last')} onClick={() => props.onOpenPath(c.path)}>
                    {c.name}
                  </span>
                </span>
              );
            })}
            {editing && fileName && !isSettingsOpen && (
              <>
                <ChevronRight size={12} className="agm-crumb-sep" />
                <span className="agm-crumb agm-last">{fileName}</span>
              </>
            )}
            {isSettingsOpen && (
              <>
                {folderCrumbs.length > 0 && <ChevronRight size={12} className="agm-crumb-sep" />}
                <span className="agm-crumb agm-last">{t('mwSettingsCrumb')}</span>
              </>
            )}
          </div>
        )}
        <div className="agm-drag" data-tauri-drag-region />
      </div>

      {!isSettingsOpen && (editing ? <PhotoTools onZoom={props.onZoom} /> : <GridTools {...props} />)}

      <div className="agm-win">
        {/* A press takes no focus, so Space and Enter stay the window's shortcuts after a click. */}
        {!isSettingsOpen && (
          <>
            <button
              className="agm-btn"
              disabled={!editing && !photoToOpen}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => (photoToOpen ? props.onOpenPhoto(photoToOpen) : props.onBackToLibrary())}
              aria-label={switchTip}
              data-tooltip={switchTip}
            >
              {editing ? <LayoutGrid size={16} /> : <ImageIcon size={16} />}
            </button>
            <div className="agm-sep" />
          </>
        )}
        <button
          className={clsx('agm-btn', uiVisibility.leftPanel ? 'agm-on' : 'agm-off')}
          onClick={() => toggle('leftPanel')}
          data-tooltip={t('mwLeft')}
        >
          <PanelLeft />
        </button>
        <button
          className={clsx('agm-btn', uiVisibility.filmstrip ? 'agm-on' : 'agm-off')}
          onClick={() => toggle('filmstrip')}
          data-tooltip={t('mwFilm')}
        >
          <PanelBottom />
        </button>
        <button
          className={clsx('agm-btn', uiVisibility.rightPanel ? 'agm-on' : 'agm-off')}
          onClick={() => toggle('rightPanel')}
          data-tooltip={t('mwRight')}
        >
          <PanelRight />
        </button>
        <VersionNote />
        <button
          className={clsx('agm-btn agm-gear', isSettingsOpen && 'agm-active')}
          onClick={() => setUI({ isSettingsOpen: !isSettingsOpen })}
          data-tooltip={t('mwGear')}
        >
          <Settings size={16} />
        </button>
        {!appSettings?.decorations && <WindowControls os={os} />}
      </div>
    </div>
  );
}

/** The grid's tools: sort, thumbnail size, and the View menu. */
function GridTools(p: TopBarProps) {
  const t = useAgTranslation();
  const { appSettings, handleSettingsChange } = useSettingsStore(
    useShallow((s) => ({ appSettings: s.appSettings, handleSettingsChange: s.handleSettingsChange })),
  );
  const { sortCriteria, setSortCriteria, filterCriteria, setFilterCriteria } = useLibraryStore(
    useShallow((s) => ({
      sortCriteria: s.sortCriteria,
      setSortCriteria: s.setSortCriteria,
      filterCriteria: s.filterCriteria,
      setFilterCriteria: s.setFilterCriteria,
    })),
  );
  const sortLabel = SORTS.find((s) => s.key === sortCriteria.key)?.label ?? 'mwSortName';
  const filters = activeFilterNames(filterCriteria, t);
  const grouping = appSettings?.grouping ?? 'off';
  const exif = appSettings?.exifOverlay ?? ExifOverlay.Off;
  const setting = (change: object) => appSettings && handleSettingsChange({ ...appSettings, ...change });

  return (
    <div className="agm-context">
      <Menu label={`${t('mwSort')}: ${t(sortLabel)}`}>
        {(close) => (
          <>
            {SORTS.map((s) => (
              <MenuItem
                key={s.key}
                checked={sortCriteria.key === s.key}
                onPick={() => {
                  setSortCriteria({ key: s.key });
                  close();
                }}
              >
                {t(s.label)}
              </MenuItem>
            ))}
            <MenuSep />
            <MenuItem
              checked={sortCriteria.order === SortDirection.Ascending}
              onPick={() => setSortCriteria({ order: SortDirection.Ascending })}
            >
              {t('mwAscending')}
            </MenuItem>
            <MenuItem
              checked={sortCriteria.order === SortDirection.Descending}
              onPick={() => setSortCriteria({ order: SortDirection.Descending })}
            >
              {t('mwDescending')}
            </MenuItem>
          </>
        )}
      </Menu>
      <input
        type="range"
        className="agm-size"
        min={0}
        max={SIZES.length - 1}
        step={1}
        value={Math.max(0, SIZES.indexOf(p.thumbnailSize))}
        onChange={(e) => p.setThumbnailSize(SIZES[Number(e.target.value)])}
        data-tooltip={t('mwSize')}
      />
      <Menu label={filters ? `${t('mwView')}: ${filters}` : t('mwView')}>
        {() => (
          <>
            <MenuItem
              checked={grouping !== 'off'}
              onPick={() => setting({ grouping: grouping === 'off' ? 'raw' : 'off' })}
            >
              {t('mwRawJpeg')}
            </MenuItem>
            {grouping !== 'off' && (
              <>
                <MenuItem checked={grouping === 'raw'} onPick={() => setting({ grouping: 'raw' })}>
                  <span className="pl-4">{t('mwPreferRaw')}</span>
                </MenuItem>
                <MenuItem checked={grouping === 'jpeg'} onPick={() => setting({ grouping: 'jpeg' })}>
                  <span className="pl-4">{t('mwPreferJpeg')}</span>
                </MenuItem>
              </>
            )}
            <MenuItem
              checked={p.libraryViewMode === LibraryViewMode.Recursive}
              onPick={() =>
                p.setLibraryViewMode(
                  p.libraryViewMode === LibraryViewMode.Recursive ? LibraryViewMode.Flat : LibraryViewMode.Recursive,
                )
              }
            >
              {t('mwSubfolders')}
            </MenuItem>
            <MenuSep />
            <MenuHead>{t('mwExif')}</MenuHead>
            {(
              [
                [ExifOverlay.Off, 'mwExifOff'],
                [ExifOverlay.Hover, 'mwExifHover'],
                [ExifOverlay.Always, 'mwExifAlways'],
              ] as const
            ).map(([value, label]) => (
              <MenuItem key={value} checked={exif === value} onPick={() => setting({ exifOverlay: value })}>
                {t(label)}
              </MenuItem>
            ))}
            <MenuHead>{t('mwShape')}</MenuHead>
            {(
              [
                [ThumbnailAspectRatio.Contain, 'mwShapeOriginal'],
                [ThumbnailAspectRatio.Cover, 'mwShapeSquare'],
                [ThumbnailAspectRatio.Justified, 'mwShapeJustified'],
              ] as const
            ).map(([value, label]) => (
              <MenuItem
                key={value}
                checked={p.thumbnailAspectRatio === value}
                onPick={() => p.setThumbnailAspectRatio(value)}
              >
                {t(label)}
              </MenuItem>
            ))}
            <MenuHead>{t('mwShow')}</MenuHead>
            {SHOW_CHOICES.map(([value, label]) => (
              <MenuItem
                key={value}
                checked={(filterCriteria.editedStatus ?? EditedStatus.All) === value}
                onPick={() => setFilterCriteria({ editedStatus: value })}
              >
                {t(label)}
              </MenuItem>
            ))}
            <MenuHead>{t('mwTypes')}</MenuHead>
            {TYPE_CHOICES.map(([value, label]) => (
              <MenuItem
                key={value}
                checked={(filterCriteria.rawStatus ?? RawStatus.All) === value}
                onPick={() => setFilterCriteria({ rawStatus: value })}
              >
                {t(label)}
              </MenuItem>
            ))}
          </>
        )}
      </Menu>
    </div>
  );
}

/** The photo's view tools: what used to sit in the toolbar over the photo. */
function PhotoTools({ onZoom }: { onZoom(zoom: number, fitToWindow?: boolean): void }) {
  const t = useAgTranslation();
  const { history, historyIndex, undo, redo, showOriginal, showClipping } = useEditorStore(
    useShallow((s) => ({
      history: s.history,
      historyIndex: s.historyIndex,
      undo: s.undo,
      redo: s.redo,
      showOriginal: s.showOriginal,
      showClipping: Number(s.adjustments.showClipping) || 0,
    })),
  );
  const { toggleShowOriginal } = useEditorActions();

  // A view, not an edit: set straight on the photo's settings, with no step in the history.
  const toggleClipping = () =>
    useEditorStore.setState((s) => ({
      // Today's cycle reads it as a number (off, all, R, G, B); true is all.
      adjustments: { ...s.adjustments, showClipping: !((Number(s.adjustments.showClipping) || 0) > 0) },
    }));

  return (
    <div className="agm-context">
      {/* data-bench-id: bench/replay.js clicks undo here, as it did on their toolbar's button. */}
      <button
        className="agm-btn"
        disabled={historyIndex <= 0}
        onClick={undo}
        data-tooltip={t('mwUndo')}
        data-bench-id="undo"
      >
        <Undo2 size={16} />
      </button>
      <button
        className="agm-btn"
        disabled={historyIndex >= history.length - 1}
        onClick={redo}
        data-tooltip={t('mwRedo')}
      >
        <Redo2 size={16} />
      </button>
      <div className="agm-sep" />
      <button className={clsx('agm-btn', showOriginal && 'agm-on')} onClick={toggleShowOriginal}>
        <Eye size={15} />
        {t('mwBefore')}
        <kbd>B</kbd>
      </button>
      <button className={clsx('agm-btn', showClipping > 0 && 'agm-on')} onClick={toggleClipping}>
        <ClipIcon />
        {t('mwClipping')}
      </button>
      <RgbReadoutButton />
      <div className="agm-sep" />
      <Menu label={t('mwZoom')}>
        {(close) => (
          <>
            <MenuItem
              onPick={() => {
                onZoom(0, true);
                close();
              }}
              shortcut="Ctrl+0"
            >
              {t('mwFit')}
            </MenuItem>
            {ZOOMS.map((z) => (
              <MenuItem
                key={z}
                onPick={() => {
                  onZoom(z);
                  close();
                }}
                shortcut={z === 1 ? 'Ctrl+1' : undefined}
              >
                {`${z * 100} %`}
              </MenuItem>
            ))}
          </>
        )}
      </Menu>
    </div>
  );
}
