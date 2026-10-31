import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'react-toastify';
import { useTranslation } from 'react-i18next';
import { useLibraryStore } from '../store/useLibraryStore';
import { useEditorStore } from '../store/useEditorStore';
import { useUIStore } from '../store/useUIStore';
import { useProcessStore } from '../store/useProcessStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { ImageFlag, Invokes } from '../components/ui/AppProperties';
import { activePathAfterDelete, isInFolder, pathAfterFolderRename, shownAfterRename } from '../argentum/fileOps';
import { ag } from '../argentum/ag';
import { useModernFolders } from '../argentum/modern/folders';
import { VIRTUAL_COPY, isVirtualCopyPath, physicalPathOf } from '../argentum/paths';

export function useFileOperations(
  refreshImageList: () => Promise<void>,
  refreshAllFolderTrees: () => Promise<void>,
  handleImageSelect: (path: string, openInEditor?: boolean) => void,
  handleBackToLibrary: () => void,
  sortedImageList: any[],
) {
  const { t } = useTranslation();

  const getParentDir = (filePath: string): string => {
    const separator = filePath.includes('/') ? '/' : '\\';
    const lastSeparatorIndex = filePath.lastIndexOf(separator);
    if (lastSeparatorIndex === -1) return '';
    return filePath.substring(0, lastSeparatorIndex);
  };

  const executeDelete = useCallback(
    async (pathsToDelete: Array<string>, options = { includeAssociated: false }) => {
      if (!pathsToDelete || pathsToDelete.length === 0) return;

      const { libraryActivePath, setLibrary } = useLibraryStore.getState();
      const { selectedImage } = useEditorStore.getState();
      const { activeView } = useUIStore.getState(); // <-- Check active view

      // Determine the active path based on the current view
      const activePath = selectedImage && activeView === 'editor' ? selectedImage.path : libraryActivePath;
      const nextImagePath = activePath
        ? activePathAfterDelete(
            sortedImageList.map((img) => img.path),
            activePath,
            pathsToDelete,
          )
        : null;

      try {
        const command = options.includeAssociated ? 'delete_files_with_associated' : 'delete_files_from_disk';
        await invoke(command, { paths: pathsToDelete });
        await refreshImageList();
        await refreshAllFolderTrees();

        if (selectedImage && activeView === 'editor') {
          const physicalPath = physicalPathOf(selectedImage.path);
          const isFileBeingEditedDeleted = pathsToDelete.some((p) => p === selectedImage.path || p === physicalPath);

          if (isFileBeingEditedDeleted) {
            if (nextImagePath) {
              handleImageSelect(nextImagePath);
            } else {
              handleBackToLibrary();
            }
          }
        } else {
          if (nextImagePath) {
            setLibrary({ multiSelectedPaths: [nextImagePath], libraryActivePath: nextImagePath });
          } else {
            setLibrary({ multiSelectedPaths: [], libraryActivePath: null });
          }

          if (selectedImage) {
            const physicalPath = physicalPathOf(selectedImage.path);
            if (pathsToDelete.some((p) => p === selectedImage.path || p === physicalPath)) {
              useEditorStore.getState().setEditor({ selectedImage: null });
            }
          }
        }
      } catch (err) {
        console.error('Failed to delete files:', err);
        toast.error(`Failed to delete files: ${err}`);
      }
    },
    [refreshImageList, refreshAllFolderTrees, handleBackToLibrary, sortedImageList, handleImageSelect],
  );

  const handleDeleteSelected = useCallback(() => {
    const { multiSelectedPaths, imageList } = useLibraryStore.getState();
    const { setUI } = useUIStore.getState();

    const pathsToDelete = multiSelectedPaths;
    if (pathsToDelete.length === 0) {
      return;
    }

    const isSingle = pathsToDelete.length === 1;

    const selectionHasVirtualCopies =
      isSingle &&
      !isVirtualCopyPath(pathsToDelete[0]) &&
      imageList.some((image) => image.path.startsWith(`${pathsToDelete[0]}${VIRTUAL_COPY}`));

    let modalTitle = 'Confirm Delete';
    let modalMessage = '';
    let confirmText = 'Delete';

    if (selectionHasVirtualCopies) {
      modalTitle = 'Delete Image and All Virtual Copies?';
      modalMessage = `Are you sure you want to permanently delete this image and all of its virtual copies? This action cannot be undone.`;
      confirmText = 'Delete All';
    } else if (isSingle) {
      modalMessage = `Are you sure you want to permanently delete this image? This action cannot be undone. Right-click for more options (e.g., deleting associated files).`;
      confirmText = 'Delete Selected Only';
    } else {
      modalMessage = `Are you sure you want to permanently delete these ${pathsToDelete.length} images? This action cannot be undone. Right-click for more options (e.g., deleting associated files).`;
      confirmText = 'Delete Selected Only';
    }

    setUI({
      confirmModalState: {
        confirmText,
        confirmVariant: 'destructive',
        isOpen: true,
        message: modalMessage,
        onConfirm: () => executeDelete(pathsToDelete, { includeAssociated: false }),
        title: modalTitle,
      },
    });
  }, [executeDelete]);

  const handleDeleteRejected = useCallback(() => {
    const { imageList } = useLibraryStore.getState();
    const rejectedPaths = imageList.filter((image) => image.flag === ImageFlag.Reject).map((image) => image.path);

    if (rejectedPaths.length === 0) {
      toast.info(t('library.reject.noneToDelete'));
      return;
    }

    const rejectedSet = new Set(rejectedPaths);
    const affectedCopyCount = imageList.filter(
      (image) =>
        isVirtualCopyPath(image.path) && !rejectedSet.has(image.path) && rejectedSet.has(physicalPathOf(image.path)),
    ).length;
    const deleteMessage = t('library.reject.deleteMessage', { count: rejectedPaths.length });

    useUIStore.getState().setUI({
      confirmModalState: {
        confirmText: t('library.reject.deleteConfirm'),
        confirmVariant: 'destructive',
        isOpen: true,
        message:
          affectedCopyCount > 0
            ? `${deleteMessage}\n\n${t('library.reject.virtualCopiesWarning', { count: affectedCopyCount })}`
            : deleteMessage,
        onConfirm: () => executeDelete(rejectedPaths, { includeAssociated: false }),
        title: t('library.reject.deleteTitle'),
      },
    });
  }, [executeDelete, t]);

  const handleCreateFolder = useCallback(
    async (folderName: string) => {
      const { folderActionTarget } = useUIStore.getState();

      if (folderName && folderName.trim() !== '' && folderActionTarget) {
        try {
          await invoke(Invokes.CreateFolder, { path: `${folderActionTarget}/${folderName.trim()}` });
          await refreshAllFolderTrees();
        } catch (err) {
          toast.error(`Failed to create folder: ${err}`);
        }
      }
    },
    [refreshAllFolderTrees],
  );

  const handleRenameFolder = useCallback(
    async (newName: string) => {
      const { folderActionTarget } = useUIStore.getState();
      const { rootPaths, currentFolderPath, setLibrary } = useLibraryStore.getState();
      const { appSettings, handleSettingsChange } = useSettingsStore.getState();

      if (newName && newName.trim() !== '' && folderActionTarget) {
        try {
          const oldPath = folderActionTarget;
          const trimmedNewName = newName.trim();

          await invoke(Invokes.RenameFolder, { path: oldPath, newName: trimmedNewName });

          const parentDir = getParentDir(oldPath);
          const separator = oldPath.includes('/') ? '/' : '\\';
          const newPath = parentDir ? `${parentDir}${separator}${trimmedNewName}` : trimmedNewName;

          // Argentum: the Recent list follows the folder (mods/recent_folders.rs), before
          // the folder on show changes and is added to it under its new name.
          try {
            useModernFolders.setState({ recent: await ag('rename_recent_folder', { oldPath, newPath }) });
          } catch (err) {
            console.warn('Recent folders not renamed:', err);
          }

          const newAppSettings = { ...appSettings } as any;
          let settingsChanged = false;
          const isInRenamedFolder = (p: string) => isInFolder(p, oldPath);
          const renamePath = (p: string) => pathAfterFolderRename(p, oldPath, newPath);

          if (rootPaths.some(isInRenamedFolder)) {
            const newRoots = rootPaths.map(renamePath);
            setLibrary({ rootPaths: newRoots });
            newAppSettings.rootFolders = newRoots;
            settingsChanged = true;
          }
          if (currentFolderPath && isInRenamedFolder(currentFolderPath)) {
            setLibrary({ currentFolderPath: renamePath(currentFolderPath) });
          }

          const currentPins = appSettings?.pinnedFolders || [];
          if (currentPins.some(isInRenamedFolder)) {
            const newPins = currentPins.map(renamePath).sort((a: string, b: string) => a.localeCompare(b));
            newAppSettings.pinnedFolders = newPins;
            settingsChanged = true;
          }

          if (settingsChanged) {
            handleSettingsChange(newAppSettings);
          }

          await refreshAllFolderTrees();
        } catch (err) {
          toast.error(`Failed to rename folder: ${err}`);
        }
      }
    },
    [refreshAllFolderTrees],
  );

  const handleSaveRename = useCallback(
    async (nameTemplate: string) => {
      const { renameTargetPaths, activeView, setUI } = useUIStore.getState();
      const { selectedImage } = useEditorStore.getState();
      const { libraryActivePath, setLibrary } = useLibraryStore.getState();

      if (renameTargetPaths.length > 0 && nameTemplate) {
        try {
          const newPaths: Array<string> = await invoke(Invokes.RenameFiles, {
            nameTemplate,
            paths: renameTargetPaths,
          });

          await refreshImageList();

          if (selectedImage && renameTargetPaths.includes(selectedImage.path)) {
            const shown = shownAfterRename(
              newPaths[renameTargetPaths.indexOf(selectedImage.path)],
              activeView === 'editor',
            );
            if (shown === 'backToLibrary') {
              handleBackToLibrary();
            } else if (shown !== 'nothing') {
              handleImageSelect(shown.select, shown.openInEditor);
            }
          }

          if (libraryActivePath && renameTargetPaths.includes(libraryActivePath)) {
            const oldPathIndex = renameTargetPaths.indexOf(libraryActivePath);
            if (newPaths[oldPathIndex]) {
              setLibrary({ libraryActivePath: newPaths[oldPathIndex] });
            } else {
              setLibrary({ libraryActivePath: null });
            }
          }

          setLibrary({ multiSelectedPaths: newPaths });
        } catch (err) {
          toast.error(`Failed to rename files: ${err}`);
        }
      }
      setUI({ renameTargetPaths: [] });
    },
    [refreshImageList, handleImageSelect, handleBackToLibrary],
  );

  const handleRenameFiles = useCallback((paths: Array<string>) => {
    if (paths && paths.length > 0) {
      useUIStore.getState().setUI({ renameTargetPaths: paths, isRenameFileModalOpen: true });
    }
  }, []);

  const handlePasteFiles = useCallback(
    async (mode = 'copy') => {
      const { copiedFilePaths, setProcess } = useProcessStore.getState();
      const { currentFolderPath, setLibrary } = useLibraryStore.getState();

      if (copiedFilePaths.length === 0 || !currentFolderPath) return;

      try {
        if (mode === 'copy') {
          await invoke(Invokes.CopyFiles, { sourcePaths: copiedFilePaths, destinationFolder: currentFolderPath });
        } else {
          await invoke(Invokes.MoveFiles, { sourcePaths: copiedFilePaths, destinationFolder: currentFolderPath });
          setProcess({ copiedFilePaths: [] });
          setLibrary({ multiSelectedPaths: [] });
          await refreshAllFolderTrees();
        }
        await refreshImageList();
      } catch (err) {
        toast.error(`Failed to ${mode} files: ${err}`);
      }
    },
    [refreshImageList, refreshAllFolderTrees],
  );

  return {
    executeDelete,
    handleDeleteSelected,
    handleDeleteRejected,
    handleCreateFolder,
    handleRenameFolder,
    handleSaveRename,
    handleRenameFiles,
    handlePasteFiles,
  };
}
