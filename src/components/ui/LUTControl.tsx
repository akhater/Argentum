import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronDown, ChevronRight, ImageOff, Plus, Upload, X, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { AnimatePresence, motion } from 'framer-motion';
import { useContextMenu } from '../../context/ContextMenuContext';
import { toast } from 'react-toastify';
import Slider from './Slider';
import { useEditorStore } from '../../store/useEditorStore';
import { useSettingsStore } from '../../store/useSettingsStore';

interface LutEntry {
  name: string;
  path: string;
  isBuiltIn: boolean;
  libraryId: string | null;
}

interface LutLibrary {
  id: string;
  name: string;
}

interface LutPreview {
  path: string;
  thumb: string | null;
}

interface LUTControlProps {
  lutPath: string | null;
  lutName: string | null;
  lutIntensity: number;
  onLutSelect: (path: string, isBuiltIn: boolean) => void;
  onLutHover?: (path: string | null, isBuiltIn?: boolean) => void;
  onIntensityChange: (intensity: number) => void;
  onClear: () => void;
  onDragStateChange?: (isDragging: boolean) => void;
}

const PREVIEW_SIZE = 112;
const SUPPORTED_EXTENSIONS = ['cube', '3dl', 'png', 'jpg', 'jpeg', 'tiff'];
const UNCATEGORIZED_LIBRARY_ID = 'uncategorized';

export default function LUTControl({
  lutPath,
  lutName,
  lutIntensity,
  onLutSelect,
  onLutHover,
  onIntensityChange,
  onClear,
  onDragStateChange,
}: LUTControlProps) {
  const { t } = useTranslation();
  const { showContextMenu } = useContextMenu();
  const selectedImagePath = useEditorStore((state) => state.selectedImage?.path ?? null);
  const isImageReady = useEditorStore((state) => state.selectedImage?.isReady ?? false);

  const [isExpanded, setIsExpanded] = useState(false);
  const [entries, setEntries] = useState<LutEntry[]>([]);
  const [libraries, setLibraries] = useState<LutLibrary[]>([]);
  const [selectedLibraryId, setSelectedLibraryId] = useState(UNCATEGORIZED_LIBRARY_ID);
  const [collapsedLibraries, setCollapsedLibraries] = useState<Record<string, boolean>>({});
  const [previews, setPreviews] = useState<Record<string, string | null>>({});
  const [isLoadingPreviews, setIsLoadingPreviews] = useState(false);
  const previewCache = useRef<Map<string, Record<string, string | null>>>(new Map());

  const handleContextMenu = (event: React.MouseEvent, entry: LutEntry) => {
    if (entry.isBuiltIn) return;

    event.preventDefault();
    event.stopPropagation();

    const moveOptions = libraries
      .filter((library) => library.id !== entry.libraryId)
      .map((library) => ({
        label: library.name,
        onClick: async () => {
          try {
            const updatedList = await invoke<LutEntry[]>('set_lut_library', {
              path: entry.path,
              libraryId: library.id,
            });
            setEntries(updatedList);
            previewCache.current.clear();
          } catch (err) {
            console.error('Failed to move LUT:', err);
            toast.error(String(err));
          }
        },
      }));

    showContextMenu(event.clientX, event.clientY, [
      ...(moveOptions.length > 0
        ? [
            {
              label: t('ui.lut.moveToLibrary', 'Move to library'),
              submenu: moveOptions,
            },
          ]
        : []),
      {
        label: t('ui.lut.removeLut'),
        icon: Trash2,
        isDestructive: true,
        onClick: async () => {
          const confirmed = window.confirm(
            t(
              'ui.lut.removeConfirm',
              'Remove this LUT from the library? Existing edits that use it may stop rendering if the file is deleted.',
            ),
          );
          if (!confirmed) return;
          try {
            const updatedList = await invoke<LutEntry[]>('remove_lut', { path: entry.path });
            setEntries(updatedList);
            setPreviews((prev) => {
              const next = { ...prev };
              delete next[entry.path];
              return next;
            });
            previewCache.current.clear();
            if (entry.path === lutPath) {
              onClear();
            }
          } catch (err) {
            console.error('Failed to remove LUT:', err);
            toast.error(String(err));
          }
        },
      },
    ]);
  };

  const refreshList = useCallback(async () => {
    try {
      const libraryList = await invoke<LutLibrary[]>('list_lut_libraries');
      const list = await invoke<LutEntry[]>('list_luts');
      setLibraries(libraryList);
      setSelectedLibraryId((current) =>
        libraryList.some((library) => library.id === current) ? current : UNCATEGORIZED_LIBRARY_ID,
      );
      setEntries(list);
    } catch (err) {
      console.error('Failed to list LUTs:', err);
    }
  }, []);

  const handleCreateLibrary = async () => {
    const name = window.prompt(t('ui.lut.newLibraryPrompt', 'Name for the new LUT library'))?.trim();
    if (!name) return;

    try {
      const library = await invoke<LutLibrary>('create_lut_library', { name });
      setLibraries((current) => [...current, library]);
      setSelectedLibraryId(library.id);
      setCollapsedLibraries((current) => ({ ...current, [library.id]: false }));
    } catch (err) {
      console.error('Failed to create LUT library:', err);
      toast.error(String(err));
    }
  };

  const handleLibraryContextMenu = (event: React.MouseEvent, library: LutLibrary) => {
    if (library.id === UNCATEGORIZED_LIBRARY_ID) return;
    event.preventDefault();
    event.stopPropagation();

    showContextMenu(event.clientX, event.clientY, [
      {
        label: t('ui.lut.renameLibrary', 'Rename library'),
        onClick: async () => {
          const name = window.prompt(t('ui.lut.renameLibraryPrompt', 'New name for this LUT library'), library.name)?.trim();
          if (!name) return;
          try {
            const updatedLibraries = await invoke<LutLibrary[]>('rename_lut_library', {
              libraryId: library.id,
              name,
            });
            setLibraries(updatedLibraries);
          } catch (err) {
            console.error('Failed to rename LUT library:', err);
            toast.error(String(err));
          }
        },
      },
      {
        label: t('ui.lut.deleteLibrary', 'Delete library'),
        isDestructive: true,
        onClick: async () => {
          try {
            const updatedLibraries = await invoke<LutLibrary[]>('delete_lut_library', { libraryId: library.id });
            const updatedEntries = await invoke<LutEntry[]>('list_luts');
            setLibraries(updatedLibraries);
            setEntries(updatedEntries);
            setSelectedLibraryId((current) => (current === library.id ? UNCATEGORIZED_LIBRARY_ID : current));
          } catch (err) {
            console.error('Failed to delete LUT library:', err);
            toast.error(String(err));
          }
        },
      },
    ]);
  };

  useEffect(() => {
    refreshList();
  }, [refreshList]);

  useEffect(() => {
    if (!isExpanded || !selectedImagePath || !isImageReady || entries.length === 0) {
      return;
    }
    const cacheKey = `${selectedImagePath}|${entries
      .map((entry) => `${entry.path}:${entry.isBuiltIn ? 1 : 0}`)
      .join(',')}`;
    const cached = previewCache.current.get(cacheKey);
    if (cached) {
      setPreviews(cached);
      return;
    }

    let isActive = true;
    setIsLoadingPreviews(true);
    invoke<LutPreview[]>('generate_lut_previews', {
      luts: entries.map((entry) => ({ path: entry.path, isBuiltIn: entry.isBuiltIn })),
      size: PREVIEW_SIZE,
    })
      .then((results) => {
        if (!isActive) return;
        const map: Record<string, string | null> = {};
        results.forEach((result) => {
          map[result.path] = result.thumb;
        });
        previewCache.current.set(cacheKey, map);
        setPreviews(map);
      })
      .catch((err) => console.error('Failed to generate LUT previews:', err))
      .finally(() => {
        if (isActive) setIsLoadingPreviews(false);
      });
    return () => {
      isActive = false;
    };
  }, [isExpanded, selectedImagePath, isImageReady, entries]);

  const handleImport = async (targetLibraryId = selectedLibraryId) => {
    try {
      const { osPlatform } = useSettingsStore.getState();
      const isAndroid = osPlatform === 'android';

      const selected = await open({
        multiple: true,
        filters: isAndroid
          ? []
          : [
              {
                name: t('ui.lut.filterLabel'),
                extensions: [...SUPPORTED_EXTENSIONS, ...SUPPORTED_EXTENSIONS.map((ext) => ext.toUpperCase())],
              },
            ],
      });
      const sourcePaths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      if (sourcePaths.length === 0) return;

      let validPaths = sourcePaths;
      if (isAndroid) {
        const resolvedNames = await Promise.all(
          sourcePaths.map(async (path) => {
            try {
              return await invoke<string>('resolve_android_content_uri_name', { uriStr: path });
            } catch (e) {
              console.error('Failed to resolve Android URI:', e);
              return path;
            }
          }),
        );
        const allowedExtensions = new Set(SUPPORTED_EXTENSIONS);
        validPaths = sourcePaths.filter((_, index) => {
          const resolvedName = resolvedNames[index];
          const ext = resolvedName.split('.').pop()?.toLowerCase() || '';
          if (!allowedExtensions.has(ext)) {
            console.warn(`Skipping unsupported file: ${resolvedName}`);
            return false;
          }
          return true;
        });
        if (validPaths.length === 0) {
          toast.error(t('ui.lut.importFailed'));
          return;
        }
      }

      const list = await invoke<LutEntry[]>('import_luts', {
        sourcePaths: validPaths,
        libraryId: targetLibraryId,
      });
      previewCache.current.clear();
      setEntries(list);
      setPreviews({});
    } catch (err) {
      console.error('Failed to import LUTs:', err);
      toast.error(t('ui.lut.importFailed'));
    }
  };

  const handleSwatchClick = (entry: LutEntry) => {
    onLutHover?.(null);
    if (entry.path === lutPath) {
      onClear();
    } else {
      onLutSelect(entry.path, entry.isBuiltIn);
    }
  };

  const builtInLuts = entries.filter((e) => e.isBuiltIn);
  const customLuts = entries.filter((e) => !e.isBuiltIn);
  const visibleLibraries =
    libraries.length > 0 ? libraries : [{ id: UNCATEGORIZED_LIBRARY_ID, name: 'Uncategorized' }];

  const renderSwatch = (entry: LutEntry) => {
    const thumb = previews[entry.path];
    const isSelected = entry.path === lutPath;
    return (
      <button
        key={entry.path}
        onMouseEnter={() => onLutHover?.(entry.path, entry.isBuiltIn)}
        onMouseLeave={() => onLutHover?.(null)}
        onClick={() => handleSwatchClick(entry)}
        onContextMenu={entry.isBuiltIn ? undefined : (e) => handleContextMenu(e, entry)}
        className={`relative aspect-square rounded-md overflow-hidden bg-bg-tertiary border-2 transition-colors ${
          isSelected ? 'border-accent' : 'border-transparent hover:border-surface'
        }`}
        data-tooltip={entry.name}
      >
        {isLoadingPreviews && thumb === undefined ? (
          <div className="w-full h-full animate-pulse bg-surface" />
        ) : thumb ? (
          <img src={thumb} alt={entry.name} className="w-full h-full object-cover" draggable={false} />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-text-secondary">
            <ImageOff size={18} />
          </div>
        )}
        <span className="absolute inset-x-0 bottom-0 px-1 py-0.5 text-[10px] text-white bg-black/60 truncate text-left backdrop-blur-xs">
          {entry.name}
        </span>
      </button>
    );
  };

  const renderLibrarySection = (library: LutLibrary) => {
    const libraryLuts = customLuts.filter(
      (entry) => (entry.libraryId || UNCATEGORIZED_LIBRARY_ID) === library.id,
    );
    const isCollapsed = collapsedLibraries[library.id] ?? false;

    return (
      <div key={library.id} className="space-y-2">
        <div
          className="flex items-center gap-1"
          onContextMenu={(event) => handleLibraryContextMenu(event, library)}
        >
          <button
            onClick={() =>
              setCollapsedLibraries((current) => ({ ...current, [library.id]: !isCollapsed }))
            }
            className="flex min-w-0 flex-1 items-center gap-1 text-left text-sm font-medium text-text-secondary hover:text-text-primary transition-colors"
          >
            {isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
            <span className="truncate">{library.name}</span>
            <span className="text-xs text-text-secondary/70">{libraryLuts.length}</span>
          </button>
          <button
            onClick={() => handleImport(library.id)}
            className="p-1 text-text-secondary hover:text-accent transition-colors"
            data-tooltip={t('ui.lut.import')}
          >
            <Upload size={13} />
          </button>
        </div>

        {!isCollapsed &&
          (libraryLuts.length > 0 ? (
            <div className="grid grid-cols-3 gap-2 pl-1">{libraryLuts.map(renderSwatch)}</div>
          ) : (
            <button
              onClick={() => handleImport(library.id)}
              className="w-full rounded-md border border-dashed border-text-secondary/20 py-3 text-xs text-text-secondary hover:border-text-secondary/40 hover:text-text-primary transition-colors"
            >
              {t('ui.lut.empty')}
            </button>
          ))}
      </div>
    );
  };

  return (
    <div className="mb-2">
      <div className="flex justify-between items-center">
        <span className="text-sm font-medium text-text-secondary select-none">{t('ui.lut.label')}</span>
        <div className="flex items-center gap-1">
          {lutName && (
            <button
              onClick={onClear}
              className="flex items-center justify-center p-0.5 rounded-full bg-bg-tertiary hover:bg-surface text-text-secondary hover:text-text-primary transition-colors"
              data-tooltip={t('ui.lut.clearLut')}
            >
              <X size={14} />
            </button>
          )}
          <button
            onClick={() => setIsExpanded((value) => !value)}
            className="flex items-center gap-1 text-sm text-text-secondary select-none cursor-pointer hover:text-accent transition-colors"
            data-tooltip={lutName || t('ui.lut.selectLutFile')}
          >
            <span className="truncate max-w-35 text-right">{lutName || t('ui.lut.select')}</span>
            <ChevronDown size={16} className={`transition-transform duration-200 ${isExpanded ? 'rotate-180' : ''}`} />
          </button>
        </div>
      </div>

      <AnimatePresence initial={false}>
        {lutName && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2, ease: 'easeInOut' }}
            className="overflow-hidden"
          >
            <div className="mt-3">
              <Slider
                label={t('ui.lut.intensity')}
                min={0}
                max={100}
                step={1}
                value={lutIntensity}
                defaultValue={100}
                onChange={(e) => onIntensityChange(Number(e.target.value))}
                onDragStateChange={onDragStateChange}
                fillOrigin="min"
              />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <AnimatePresence initial={false}>
        {isExpanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.25, ease: 'easeInOut' }}
            className="overflow-hidden"
          >
            <div className={`${lutName ? '' : 'mt-3'} pb-1 space-y-4`}>
              {builtInLuts.length > 0 && (
                <div>
                  <span className="text-sm font-medium text-text-secondary select-none block mb-2">
                    {t('ui.lut.filmEmulations')}
                  </span>
                  <div className="grid grid-cols-3 gap-2">{builtInLuts.map(renderSwatch)}</div>
                </div>
              )}

              <div>
                <div className="flex items-center justify-between mb-2">
                  <span className="text-sm font-medium text-text-secondary select-none">{t('ui.lut.customLuts')}</span>
                  <button
                    onClick={handleCreateLibrary}
                    className="text-xs text-text-secondary hover:text-accent flex items-center gap-1 transition-colors"
                    data-tooltip={t('ui.lut.newLibrary', 'New library')}
                  >
                    <Plus size={12} />
                    {t('ui.lut.newLibrary', 'New library')}
                  </button>
                </div>

                <div className="flex items-center gap-2 mb-3">
                  <select
                    value={selectedLibraryId}
                    onChange={(event) => setSelectedLibraryId(event.target.value)}
                    className="min-w-0 flex-1 rounded-md bg-bg-tertiary border border-surface px-2 py-1.5 text-xs text-text-primary focus:border-accent focus:outline-none"
                    aria-label={t('ui.lut.importLibrary', 'Import into library')}
                  >
                    {visibleLibraries.map((library) => (
                      <option key={library.id} value={library.id}>
                        {t('ui.lut.importInto', 'Import into')}: {library.name}
                      </option>
                    ))}
                  </select>
                  <button
                    onClick={() => handleImport()}
                    className="flex items-center gap-1 rounded-md bg-bg-tertiary px-2 py-1.5 text-xs text-text-secondary hover:text-accent transition-colors"
                  >
                    <Upload size={12} />
                    {t('ui.lut.import')}
                  </button>
                </div>

                <div className="space-y-4">{visibleLibraries.map(renderLibrarySection)}</div>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
