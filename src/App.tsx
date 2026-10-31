import Argentum from './argentum/Argentum';
import { ModernWindow, refreshModernTrees, startModernLibrary } from './argentum/modern';
import { useLightPalette } from './argentum/layout/editorLayout';
import { type PointerEvent as ReactPointerEvent, useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCloudStore } from './store/useCloudStore';
import { ToastContainer, toast, Slide } from 'react-toastify';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  Modifier,
  MeasuringStrategy,
  pointerWithin,
} from '@dnd-kit/core';
import clsx from 'clsx';

import FolderTree from './components/panel/right/FolderTree';
import ExportPanel from './components/panel/right/ExportPanel';
import GlobalTooltip from './components/ui/GlobalTooltip';
import AppModals from './components/modals/AppModals';

import MetadataPanel from './components/panel/right/MetadataPanel';
import CropPanel from './components/panel/right/CropPanel';
import AIPanel from './components/panel/right/AIPanel';
import TetheringPanel from './components/panel/right/TetheringPanel';

import { ContextMenuProvider } from './context/ContextMenuContext';
import { useSettingsStore } from './store/useSettingsStore';
import { DEFAULT_BOTTOM_PANEL_HEIGHT, DEFAULT_PANEL_WIDTH, useUIStore } from './store/useUIStore';
import { useLibraryStore } from './store/useLibraryStore';
import { useEditorStore } from './store/useEditorStore';
import { useProcessStore } from './store/useProcessStore';
import { useShallow } from 'zustand/react/shallow';

import { useThumbnails } from './hooks/useThumbnails';
import { useKeyboardShortcuts } from './hooks/useKeyboardShortcuts';
import { useTauriListeners } from './hooks/useTauriListeners';
import { useFileOperations } from './hooks/useFileOperations';
import { useAppContextMenus } from './hooks/useAppContextMenus';
import { useSortedLibrary } from './hooks/useSortedLibrary';
import { useAppNavigation } from './hooks/useAppNavigation';
import { useExternalEditSession } from './hooks/useExternalEditSession';
import ExternalEditBar from './components/ui/ExternalEditBar';
import { Status } from './components/ui/ExportImportProperties';

import { useEditorActions } from './hooks/useEditorActions';
import { useLibraryActions } from './hooks/useLibraryActions';
import { useProductivityActions } from './hooks/useProductivityActions';

import { useAppInitialization } from './hooks/useAppInitialization';
import './i18n';

import {
  Invokes,
  ImageFile,
  LibraryViewMode,
  Panel,
  ThumbnailSize,
  ThumbnailAspectRatio,
} from './components/ui/AppProperties';

import ImageProcessingManager from './components/managers/ImageProcessingManager';
import ImageLoaderManager from './components/managers/ImageLoaderManager';

const insertChildrenIntoTree = (node: any, targetPath: string, newChildren: any[]): any => {
  if (!node) return null;

  if (node.path === targetPath) {
    const mergedChildren = newChildren.map((newChild: any) => {
      const existingChild = node.children?.find((c: any) => c.path === newChild.path);
      if (existingChild && existingChild.children && existingChild.children.length > 0) {
        return { ...newChild, children: existingChild.children };
      }
      return newChild;
    });
    return { ...node, children: mergedChildren };
  }

  if (node.children && node.children.length > 0) {
    return {
      ...node,
      children: node.children.map((child: any) => insertChildrenIntoTree(child, targetPath, newChildren)),
    };
  }

  return node;
};

const imageDragModifier: Modifier = ({ active, activatorEvent, activeNodeRect, transform }) => {
  if (active?.data?.current?.type === 'library-image' && activatorEvent && activeNodeRect) {
    const event = activatorEvent as any;
    const startX = event.clientX ?? event.touches?.[0]?.clientX ?? 0;
    const startY = event.clientY ?? event.touches?.[0]?.clientY ?? 0;

    if (startX === 0 && startY === 0) return transform;

    const offsetX = startX - activeNodeRect.left - 48;
    const offsetY = startY - activeNodeRect.top - 48;

    return {
      ...transform,
      x: transform.x + offsetX,
      y: transform.y + offsetY,
    };
  }
  return transform;
};

function ImageDragOverlayNode({ activeItem }: { activeItem: { path: string; paths: string[] } }) {
  const url = useProcessStore.getState().thumbnails[activeItem.path];
  const count = activeItem.paths.length;

  return (
    <div className="w-24 h-24 rounded-lg shadow-2xl border-2 border-accent relative bg-surface overflow-hidden flex items-center justify-center">
      {url && <img src={url} className="w-full h-full object-cover" />}
      {count > 1 && (
        <div className="absolute top-1 right-1 bg-accent text-button-text text-xs font-bold px-2 py-0.5 rounded-full shadow-md z-10">
          {count}
        </div>
      )}
    </div>
  );
}

function App() {
  const [activeImageDragItem, setActiveImageDragItem] = useState<{ path: string; paths: string[] } | null>(null);

  const { appSettings, osPlatform, handleSettingsChange } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      osPlatform: state.osPlatform,
      handleSettingsChange: state.handleSettingsChange,
    })),
  );

  const { activeView, isFullScreen, isWindowFullScreen, isInstantTransition, activePanel, setUI } = useUIStore(
    useShallow((state) => ({
      activeView: state.activeView,
      isFullScreen: state.isFullScreen,
      isWindowFullScreen: state.isWindowFullScreen,
      isInstantTransition: state.isInstantTransition,
      activePanel: state.activePanel,
      setUI: state.setUI,
    })),
  );

  const { rootPaths, currentFolderPath, expandedFolders, multiSelectedPaths, setLibrary } = useLibraryStore(
    useShallow((state) => ({
      rootPaths: state.rootPaths,
      currentFolderPath: state.currentFolderPath,
      expandedFolders: state.expandedFolders,
      multiSelectedPaths: state.multiSelectedPaths,
      setLibrary: state.setLibrary,
    })),
  );

  const { selectedImage, activeMaskContainerId, activeAiPatchContainerId, hasRenderedFirstFrame, setEditor } =
    useEditorStore(
      useShallow((state) => ({
        selectedImage: state.selectedImage,
        activeMaskContainerId: state.activeMaskContainerId,
        activeAiPatchContainerId: state.activeAiPatchContainerId,
        hasRenderedFirstFrame: state.hasRenderedFirstFrame,
        setEditor: state.setEditor,
      })),
    );

  const { exportState, setExportState } = useProcessStore(
    useShallow((state) => ({
      exportState: state.exportState,
      setExportState: state.setExportState,
    })),
  );

  const selectedImagePathRef = useRef<string | null>(null);
  useEffect(() => {
    selectedImagePathRef.current = selectedImage?.path ?? null;
  }, [selectedImage?.path]);

  const prevAdjustmentsRef = useRef<any>(null);

  const isBackendReadyRef = useRef(true);
  const previewJobIdRef = useRef<number>(0);
  const latestRenderedJobIdRef = useRef<number>(0);
  const currentResRef = useRef<number>(1280);
  const cachedEditStateRef = useRef<any | null>(null);

  const [libraryViewMode, setLibraryViewMode] = useState<LibraryViewMode>(LibraryViewMode.Flat);
  const [isResizing, setIsResizing] = useState(false);
  const [thumbnailSize, setThumbnailSize] = useState(ThumbnailSize.Medium);
  const [thumbnailAspectRatio, setThumbnailAspectRatio] = useState(ThumbnailAspectRatio.Contain);

  const { requestThumbnails, clearThumbnailQueue, markGenerated } = useThumbnails();

  const transformWrapperRef = useRef<any>(null);
  const preloadedDataRef = useRef<{
    trees?: Promise<any>;
    images?: Promise<ImageFile[]>;
    rootPaths?: string[];
    currentPath?: string;
  }>({});

  useAppInitialization({
    preloadedDataRef,
    thumbnailSize,
    setThumbnailSize,
    thumbnailAspectRatio,
    setThumbnailAspectRatio,
    libraryViewMode,
    setLibraryViewMode,
  });

  const { handleZoomChange } = useEditorActions();

  const navigationRefs = useMemo(
    () => ({
      transformWrapperRef,
      preloadedDataRef,
      cachedEditStateRef,
      selectedImagePathRef,
      isBackendReadyRef,
      latestRenderedJobIdRef,
      previewJobIdRef,
      currentResRef,
      prevAdjustmentsRef,
    }),
    [],
  );

  const {
    handleGoHome,
    handleBackToLibrary,
    handleImageSelect,
    handleSelectSubfolder,
    handleSelectAlbum,
    handleOpenFolder,
    handleNavBack,
    handleNavForward,
  } = useAppNavigation({
    clearThumbnailQueue,
    refs: navigationRefs,
  });

  const {
    externalEditSession,
    isFinishing: isExternalEditFinishing,
    finishExternalEdit,
  } = useExternalEditSession(handleImageSelect);

  const {
    handleRate,
    handleClearSelection,
    handleLibraryImageSingleClick,
    handleImageClick,
    handleSetFlag,
    handleTogglePinFolder,
    handleCreateAlbumItem,
    handleRenameAlbumItem,
  } = useLibraryActions(handleImageSelect);

  // Argentum: the folder tree's roots are the disks. It starts on them with no
  // start screen, and its refresh leaves their photos uncounted, which would
  // read every folder on a disk.
  const refreshAllFolderTrees = refreshModernTrees;
  const handleModernStart = useCallback(
    () => startModernLibrary(preloadedDataRef, handleSelectSubfolder),
    [handleSelectSubfolder],
  );

  useEffect(() => {
    const { setImageSelectHandler } = useUIStore.getState();
    setImageSelectHandler(handleImageSelect);
    return () => setImageSelectHandler(null);
  }, [handleImageSelect]);

  const { displayList: sortedImageList, badges: groupBadgeInfo } = useSortedLibrary();

  const handleLibraryRefresh = useCallback(async () => {
    if (currentFolderPath) {
      if (currentFolderPath.startsWith('Album: ')) {
        const { activeAlbumId, albumTree } = useLibraryStore.getState();
        if (activeAlbumId) {
          const findObj = (nodes: any[]): any => {
            for (const n of nodes) {
              if (n.id === activeAlbumId) return n;
              if (n.type === 'group') {
                const f = findObj(n.children);
                if (f) return f;
              }
            }
            return null;
          };
          const album = findObj(albumTree);
          if (album) await handleSelectAlbum(album.id, album.name, album.images, true);
        }
      } else {
        await handleSelectSubfolder(currentFolderPath, false, undefined, false, true);
      }
    }
  }, [currentFolderPath, handleSelectSubfolder, handleSelectAlbum]);

  const {
    executeDelete,
    handleDeleteSelected,
    handleDeleteRejected,
    handleCreateFolder,
    handleRenameFolder,
    handleSaveRename,
    handleRenameFiles,
    handlePasteFiles,
  } = useFileOperations(
    handleLibraryRefresh,
    refreshAllFolderTrees,
    handleImageSelect,
    handleBackToLibrary,
    sortedImageList,
  );

  const {
    handleStartPanorama,
    handleSavePanorama,
    handleStartHdr,
    handleSaveHdr,
    handleApplyDenoise,
    handleBatchDenoise,
    handleSaveDenoisedImage,
    handleSaveCollage,
    handleStartFocusStack,
    handleSaveFocusStack,
  } = useProductivityActions(handleLibraryRefresh);

  const {
    handleEditorContextMenu,
    handleThumbnailContextMenu,
    handleFolderTreeContextMenu,
    handleAlbumTreeContextMenu,
    handleMainLibraryContextMenu,
  } = useAppContextMenus({
    handleImageSelect,
    handleBackToLibrary,
    handleLibraryRefresh,
    handleRenameFiles,
    refreshAllFolderTrees,
    refreshImageList: handleLibraryRefresh,
    executeDelete,
    handleDeleteRejected,
    handleTogglePinFolder,
  });

  useTauriListeners({
    refreshImageList: handleLibraryRefresh,
    markGenerated,
  });

  useKeyboardShortcuts({
    sortedImageList,
    handleBackToLibrary,
    handleDeleteSelected,
    handleDeleteRejected,
    handleGoHome,
    handleImageSelect,
    handlePasteFiles,
    handleZoomChange,
  });

  useEffect(() => {
    const handleGlobalContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };
    window.addEventListener('contextmenu', handleGlobalContextMenu);
    return () => window.removeEventListener('contextmenu', handleGlobalContextMenu);
  }, []);

  // Argentum: toasts follow the Modern palette, not the Classic theme Settings no longer sets.
  const isLightTheme = useLightPalette();

  useEffect(() => {
    if (
      (activePanel !== Panel.Masks || !activeMaskContainerId) &&
      (activePanel !== Panel.Ai || !activeAiPatchContainerId)
    ) {
      setEditor({ isMaskControlHovered: false });
    }
  }, [activePanel, activeMaskContainerId, activeAiPatchContainerId, setEditor]);

  useEffect(() => {
    setEditor({
      isWbPickerActive: false,
      isStraightenActive: false,
      isGuidedPerspectiveActive: false,
      activeMaskId: null,
      activeAiSubMaskId: null,
    });
  }, [activePanel, activeView, setEditor]);

  useEffect(() => {
    const unlisten = listen('ai-connector-status-update', (event: any) => {
      setEditor({ isAIConnectorConnected: event.payload.connected });
    });
    invoke(Invokes.CheckAIConnectorStatus);
    const interval = setInterval(() => invoke(Invokes.CheckAIConnectorStatus), 10000);
    return () => {
      clearInterval(interval);
      unlisten.then((f) => f());
    };
  }, [setEditor]);

  const createResizeHandler = (stateKey: string, startSize: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    setIsResizing(true);

    const pointerId = e.pointerId;
    const target = e.currentTarget;
    const startX = e.clientX;
    const startY = e.clientY;

    const previousTouchAction = document.documentElement.style.touchAction;
    const previousUserSelect = document.documentElement.style.userSelect;

    target.setPointerCapture?.(pointerId);
    document.documentElement.style.touchAction = 'none';
    document.documentElement.style.userSelect = 'none';

    const doDrag = (moveEvent: PointerEvent) => {
      if (moveEvent.pointerId !== pointerId) return;
      moveEvent.preventDefault();

      if (stateKey === 'left') {
        let w = startSize + (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, leftPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            leftPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, leftPanel: true },
          }));
        }
      } else if (stateKey === 'right') {
        let w = startSize - (moveEvent.clientX - startX);
        if (w < 200) {
          setUI((state) => ({ uiVisibility: { ...state.uiVisibility, rightPanel: false } }));
        } else {
          w = Math.min(w, 600);
          setUI((state) => ({
            rightPanelWidth: Math.round(w),
            uiVisibility: { ...state.uiVisibility, rightPanel: true },
          }));
        }
      } else if (stateKey === 'bottom') {
        const newHeight = startSize - (moveEvent.clientY - startY);
        if (newHeight < 100) {
          setUI((state) => ({
            uiVisibility: { ...state.uiVisibility, filmstrip: false },
          }));
        } else {
          setUI((state) => ({
            bottomPanelHeight: Math.round(Math.min(newHeight, 400)),
            uiVisibility: { ...state.uiVisibility, filmstrip: true },
          }));
        }
      }
    };

    const stopDrag = (upEvent: PointerEvent) => {
      if (upEvent.pointerId !== pointerId) return;
      if (target.hasPointerCapture?.(pointerId)) target.releasePointerCapture(pointerId);

      document.documentElement.style.cursor = '';
      document.documentElement.style.touchAction = previousTouchAction;
      document.documentElement.style.userSelect = previousUserSelect;

      window.removeEventListener('pointermove', doDrag);
      window.removeEventListener('pointerup', stopDrag);
      window.removeEventListener('pointercancel', stopDrag);
      setIsResizing(false);
    };

    document.documentElement.style.cursor = stateKey === 'bottom' ? 'row-resize' : 'col-resize';

    window.addEventListener('pointermove', doDrag, { passive: false });
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
  };

  const createResizeResetHandler = (stateKey: string) => () => {
    if (stateKey === 'left') {
      setUI((state) => ({
        leftPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, leftPanel: true },
      }));
    } else if (stateKey === 'right') {
      setUI((state) => ({
        rightPanelWidth: DEFAULT_PANEL_WIDTH,
        uiVisibility: { ...state.uiVisibility, rightPanel: true },
      }));
    } else if (stateKey === 'bottom') {
      setUI((state) => ({
        bottomPanelHeight: DEFAULT_BOTTOM_PANEL_HEIGHT,
        uiVisibility: { ...state.uiVisibility, filmstrip: true },
      }));
    }
  };

  useEffect(() => {
    const appWindow = getCurrentWindow();
    const checkFullscreen = async () => {
      setUI({ isWindowFullScreen: await appWindow.isFullscreen() });
    };
    checkFullscreen();
    const unlistenPromise = appWindow.onResized(checkFullscreen);
    return () => {
      unlistenPromise.then((unlisten: any) => unlisten());
    };
  }, [setUI]);

  const handleToggleFolder = useCallback(
    async (path: string) => {
      const isExpanding = !expandedFolders.has(path);
      setLibrary((state) => {
        const newSet = new Set(state.expandedFolders);
        if (isExpanding) {
          newSet.add(path);
        } else {
          newSet.delete(path);
        }
        return { expandedFolders: newSet };
      });
      if (!isExpanding) return;
      try {
        // Argentum: no counts here. The roots are whole disks, and this count reads every
        // folder under them before the tree shows; each row counts its own (folderCounts.ts).
        const newChildren: any[] = await invoke(Invokes.GetFolderChildren, {
          path,
          showImageCounts: false,
        });
        setLibrary((state) => ({
          folderTrees: state.folderTrees.map((t: any) => insertChildrenIntoTree(t, path, newChildren)),
        }));
        setLibrary((state) => ({
          pinnedFolderTrees: state.pinnedFolderTrees.map((tree) => insertChildrenIntoTree(tree, path, newChildren)),
        }));
      } catch (err) {
        toast.error(`Failed to load folder: ${err}`);
      }
    },
    [expandedFolders, setLibrary],
  );

  const renderAppPanel = useCallback(
    (panelId: Panel) => {
      switch (panelId) {
        case Panel.FolderTree:
          return (
            <FolderTree
              isResizing={isResizing}
              onContextMenu={handleFolderTreeContextMenu}
              onAlbumContextMenu={handleAlbumTreeContextMenu}
              onSelectAlbum={(id, name, images, skipHistory) => handleSelectAlbum(id, name, images, false, skipHistory)}
              onFolderSelect={(path, skipHistory) =>
                handleSelectSubfolder(path, false, undefined, true, false, skipHistory)
              }
              onToggleFolder={handleToggleFolder}
              onOpenFolder={handleOpenFolder}
              onNavBack={handleNavBack}
              onNavForward={handleNavForward}
              style={{ width: '100%', height: '100%' }}
              isInstantTransition={isInstantTransition}
            />
          );
        case Panel.Export:
          return (
            <ExportPanel
              exportState={exportState}
              multiSelectedPaths={multiSelectedPaths}
              selectedImage={selectedImage}
              setExportState={setExportState}
              appSettings={appSettings}
              onSettingsChange={handleSettingsChange}
              rootPaths={rootPaths}
              isVisible={true}
              libraryContext
            />
          );
        case Panel.Metadata:
          return <MetadataPanel />;
        case Panel.Crop:
          return <CropPanel />;
        case Panel.Ai:
          return <AIPanel />;
        case Panel.Tethering:
          return <TetheringPanel onLibraryRefresh={handleLibraryRefresh} onImageSelect={handleImageSelect} />;
        default:
          return null;
      }
    },
    [
      isResizing,
      handleFolderTreeContextMenu,
      handleAlbumTreeContextMenu,
      handleSelectAlbum,
      handleSelectSubfolder,
      handleToggleFolder,
      handleOpenFolder,
      handleNavBack,
      handleNavForward,
      setUI,
      isInstantTransition,
      exportState,
      multiSelectedPaths,
      selectedImage,
      setExportState,
      appSettings,
      handleSettingsChange,
      rootPaths,
    ],
  );

  const isWgpuActive =
    activeView === 'editor' &&
    appSettings?.useWgpuRenderer !== false &&
    selectedImage?.isReady &&
    hasRenderedFirstFrame;
  const useMacWindowShell = osPlatform === 'macos' && !appSettings?.decorations && !isWindowFullScreen && !isFullScreen;

  const libraryViewProps = {
    sortedImageList,
    groupBadgeInfo,
    thumbnailSize,
    thumbnailAspectRatio,
    libraryViewMode,
    setThumbnailSize,
    setThumbnailAspectRatio,
    setLibraryViewMode,
    handleClearSelection,
    handleLibraryImageSingleClick,
    handleImageSelect,
    handleThumbnailContextMenu,
    handleMainLibraryContextMenu,
    requestThumbnails,
  };

  const layoutSensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 5 } }));
  const handleDragStart = (e: any) => {
    if (e.active.data.current?.type === 'library-image') {
      const path = e.active.data.current.path;
      const multiSelected = useLibraryStore.getState().multiSelectedPaths;
      const paths = multiSelected.includes(path) ? multiSelected : [path];
      setActiveImageDragItem({ path, paths });
    }
  };

  const handleDragEnd = (e: any) => {
    setActiveImageDragItem(null);
    const { active, over } = e;

    if (active.data.current?.type === 'library-image' && over?.data.current?.type === 'folder') {
      const targetFolder = over.data.current.path;
      const sourcePaths = activeImageDragItem?.paths || [active.data.current.path];

      invoke(Invokes.MoveFiles, { sourcePaths, destinationFolder: targetFolder })
        .then(() => {
          refreshAllFolderTrees();
          handleLibraryRefresh();
          useLibraryStore.getState().setLibrary({ multiSelectedPaths: [] });
        })
        .catch((err) => {
          toast.error(`Failed to move files: ${err}`);
        });
    }

    if (active.data.current?.type === 'library-image' && over?.data.current?.type === 'album') {
      const targetAlbumId = over.data.current.id;
      const sourcePaths = activeImageDragItem?.paths || [active.data.current.path];

      invoke(Invokes.AddToAlbum, { albumId: targetAlbumId, paths: sourcePaths })
        .then(() => invoke(Invokes.GetAlbums))
        .then((updatedTree: any) => {
          useLibraryStore.getState().setLibrary({ albumTree: updatedTree, multiSelectedPaths: [] });
          handleLibraryRefresh();
        })
        .catch((err) => {
          toast.error(`Failed to add to album: ${err}`);
        });
    }
  };

  return (
    <>
      <Argentum onImageSelect={handleImageSelect} onLibraryRefresh={handleLibraryRefresh} />
      <ImageProcessingManager
        transformWrapperRef={transformWrapperRef}
        prevAdjustmentsRef={prevAdjustmentsRef}
        previewJobIdRef={previewJobIdRef}
        latestRenderedJobIdRef={latestRenderedJobIdRef}
        currentResRef={currentResRef}
      />
      <ImageLoaderManager cachedEditStateRef={cachedEditStateRef} />
      <div
        className={clsx(
          'flex flex-col h-screen font-sans text-text-primary overflow-hidden select-none',
          useMacWindowShell && 'macos-window-shell',
          isWgpuActive ? 'bg-transparent' : 'bg-bg-primary',
        )}
      >
        <div className="flex-1 flex flex-col min-h-0">
          <DndContext
            sensors={layoutSensors}
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
            collisionDetection={pointerWithin}
            measuring={{
              droppable: {
                strategy: MeasuringStrategy.Always,
              },
            }}
          >
            {/* Argentum: the Modern window is the whole app at every size (argentum/modern). */}
            <ModernWindow
              gpu={appSettings?.useWgpuRenderer !== false}
              renderAppPanel={renderAppPanel}
              createResizeHandler={createResizeHandler}
              createResizeResetHandler={createResizeResetHandler}
              onStart={handleModernStart}
              onSelectFolder={(path) => handleSelectSubfolder(path, false, undefined, true)}
              onNavBack={handleNavBack}
              onNavForward={handleNavForward}
              transformWrapperRef={transformWrapperRef}
              onBackToLibrary={handleBackToLibrary}
              onEditorContextMenu={handleEditorContextMenu}
              onOpenInEditor={handleImageClick}
              onOpenPhoto={handleImageSelect}
              onZoom={handleZoomChange}
              libraryProps={libraryViewProps}
              sortedImageList={sortedImageList}
              thumbnailAspectRatio={thumbnailAspectRatio}
              onSelectInGrid={handleLibraryImageSingleClick}
              onClearSelection={handleClearSelection}
              onThumbnailContextMenu={handleThumbnailContextMenu}
              onEmptyAreaContextMenu={handleMainLibraryContextMenu}
              onRequestThumbnails={requestThumbnails}
              settingsProps={{
                appSettings,
                onLibraryRefresh: handleLibraryRefresh,
                onSettingsChange: handleSettingsChange,
                rootPaths,
              }}
              externalEditBar={
                selectedImage && externalEditSession ? (
                  <ExternalEditBar
                    session={externalEditSession}
                    isFinishing={isExternalEditFinishing}
                    errorMessage={exportState.status === Status.Error ? exportState.errorMessage : ''}
                    onDone={finishExternalEdit}
                  />
                ) : undefined
              }
            />
            <DragOverlay modifiers={activeImageDragItem ? [imageDragModifier] : undefined} dropAnimation={null}>
              {activeImageDragItem ? <ImageDragOverlayNode activeItem={activeImageDragItem} /> : null}
            </DragOverlay>
          </DndContext>
        </div>
        <AppModals
          handleImageSelect={handleImageSelect}
          handleSavePanorama={handleSavePanorama}
          handleStartPanorama={handleStartPanorama}
          handleStartFocusStack={handleStartFocusStack}
          handleSaveFocusStack={handleSaveFocusStack}
          handleSaveHdr={handleSaveHdr}
          handleStartHdr={handleStartHdr}
          refreshImageList={handleLibraryRefresh}
          handleApplyDenoise={handleApplyDenoise}
          handleBatchDenoise={handleBatchDenoise}
          handleSaveDenoisedImage={handleSaveDenoisedImage}
          handleCreateFolder={handleCreateFolder}
          handleRenameFolder={handleRenameFolder}
          handleSaveRename={handleSaveRename}
          handleSetFlag={handleSetFlag}
          handleRate={handleRate}
          executeDelete={executeDelete}
          handleSaveCollage={handleSaveCollage}
          handleCreateAlbumItem={handleCreateAlbumItem}
          handleRenameAlbumItem={handleRenameAlbumItem}
        />
        <ToastContainer
          position="bottom-right"
          autoClose={5000}
          hideProgressBar={false}
          newestOnTop
          closeOnClick
          rtl={false}
          pauseOnFocusLoss
          draggable={false}
          pauseOnHover
          theme={isLightTheme ? 'light' : 'dark'}
          transition={Slide}
          toastClassName={() =>
            clsx(
              'relative flex min-h-16 p-4 rounded-lg justify-between overflow-hidden cursor-pointer mb-4',
              'bg-surface! text-text-primary! border! border-border-color! shadow-2xl! max-w-[420px]!',
            )
          }
        />
      </div>
    </>
  );
}

const AppWrapper = () => {
  useEffect(() => {
    useCloudStore.getState().initAuth();
  }, []);

  return (
    <ContextMenuProvider>
      <App />
      <GlobalTooltip />
    </ContextMenuProvider>
  );
};

export default AppWrapper;
