import { create } from 'zustand';
import {
  ImageFile,
  Panel,
  UiVisibility,
  CullingSuggestions,
  WorkspaceState,
  CollapsibleSectionsState,
} from '../components/ui/AppProperties';
import { useEditorStore } from './useEditorStore';
import { columnOf } from '../argentum/modern/panels';

interface ConfirmModalState {
  confirmText?: string;
  confirmVariant?: string;
  isOpen: boolean;
  message?: string;
  onConfirm?(): void;
  title?: string;
}

interface CollageModalState {
  isOpen: boolean;
  sourceImages: Array<Pick<ImageFile, 'path'>>;
}

interface PanoramaModalState {
  error: string | null;
  finalImageBase64: string | null;
  isOpen: boolean;
  isProcessing: boolean;
  progressMessage: string | null;
  stitchingSourcePaths: Array<string>;
}

interface FocusStackModalState {
  error: string | null;
  finalImageBase64: string | null;
  depthMapBase64: string | null;
  isOpen: boolean;
  isProcessing: boolean;
  progressMessage: string | null;
  sourcePaths: Array<string>;
}

interface HdrModalState {
  error: string | null;
  finalImageBase64: string | null;
  isOpen: boolean;
  isProcessing: boolean;
  progressMessage: string | null;
  stitchingSourcePaths: Array<string>;
}

interface DenoiseModalState {
  isOpen: boolean;
  isProcessing: boolean;
  previewBase64: string | null;
  originalBase64?: string | null;
  error: string | null;
  targetPaths: string[];
  progressMessage: string | null;
  isRaw: boolean;
}

interface NegativeConversionModalState {
  isOpen: boolean;
  targetPaths: Array<string>;
}

interface CullingModalState {
  isOpen: boolean;
  suggestions: CullingSuggestions | null;
  progress: { current: number; total: number; stage: string } | null;
  error: string | null;
  pathsToCull: Array<string>;
}

export const DEFAULT_PANEL_WIDTH = 350;
export const DEFAULT_BOTTOM_PANEL_HEIGHT = 144;

/** The column widths in a saved workspace; what the Classic layout also saved is left behind here. */
export function readWorkspace(saved: Partial<WorkspaceState> | undefined): WorkspaceState {
  return {
    leftPanelWidth: saved?.leftPanelWidth || DEFAULT_PANEL_WIDTH,
    rightPanelWidth: saved?.rightPanelWidth || DEFAULT_PANEL_WIDTH,
  };
}

export interface UIState {
  activeView: string;
  isFullScreen: boolean;
  isWindowFullScreen: boolean;
  isInstantTransition: boolean;
  uiVisibility: UiVisibility;
  isSettingsOpen: boolean;

  leftPanelWidth: number;
  rightPanelWidth: number;
  bottomPanelHeight: number;

  /** Argentum: whether this machine can tether a camera, asked once at start-up (useAppInitialization.ts). */
  isTetheringSupported: boolean;

  activePanel: Panel | null;
  collapsibleSectionsState: CollapsibleSectionsState;

  isCreateFolderModalOpen: boolean;
  isRenameFolderModalOpen: boolean;
  isRenameFileModalOpen: boolean;
  renameTargetPaths: Array<string>;
  isCopyPasteSettingsModalOpen: boolean;
  folderActionTarget: string | null;

  isCreateAlbumModalOpen: boolean;
  isCreateAlbumGroupModalOpen: boolean;
  isRenameAlbumModalOpen: boolean;
  albumActionTarget: string | null;

  confirmModalState: ConfirmModalState;
  panoramaModalState: PanoramaModalState;
  focusStackModalState: FocusStackModalState;
  hdrModalState: HdrModalState;
  negativeModalState: NegativeConversionModalState;
  denoiseModalState: DenoiseModalState;
  cullingModalState: CullingModalState;
  collageModalState: CollageModalState;

  setUI: (updater: Partial<UIState> | ((state: UIState) => Partial<UIState>)) => void;
  setPanel: (panel: Panel | null) => void;
  customEscapeHandler: (() => void) | null;
  setCustomEscapeHandler: (handler: (() => void) | null) => void;
  imageSelectHandler: ((path: string, openInEditor?: boolean) => void) | null;
  setImageSelectHandler: (handler: ((path: string, openInEditor?: boolean) => void) | null) => void;
  searchFocusRequest: number;
  requestSearchFocus: () => void;
  toggleFullScreen: () => void;
  resetWorkspaceLayout: () => WorkspaceState;
}

export const useUIStore = create<UIState>((set, get) => ({
  activeView: 'library',
  isFullScreen: false,
  isWindowFullScreen: false,
  isInstantTransition: false,
  uiVisibility: { filmstrip: true, leftPanel: true, rightPanel: true },
  isSettingsOpen: false,

  leftPanelWidth: DEFAULT_PANEL_WIDTH,
  rightPanelWidth: DEFAULT_PANEL_WIDTH,
  bottomPanelHeight: DEFAULT_BOTTOM_PANEL_HEIGHT,
  isTetheringSupported: false,

  activePanel: Panel.Adjustments,
  collapsibleSectionsState: { basic: true, color: false, curves: true, details: false, effects: false },

  isCreateFolderModalOpen: false,
  isRenameFolderModalOpen: false,
  isRenameFileModalOpen: false,
  renameTargetPaths: [],
  isCopyPasteSettingsModalOpen: false,
  folderActionTarget: null,
  isCreateAlbumModalOpen: false,
  isCreateAlbumGroupModalOpen: false,
  isRenameAlbumModalOpen: false,
  albumActionTarget: null,

  confirmModalState: { isOpen: false },
  panoramaModalState: {
    error: null,
    finalImageBase64: null,
    isOpen: false,
    isProcessing: false,
    progressMessage: '',
    stitchingSourcePaths: [],
  },
  focusStackModalState: {
    error: null,
    finalImageBase64: null,
    depthMapBase64: null,
    isOpen: false,
    isProcessing: false,
    progressMessage: '',
    sourcePaths: [],
  },
  hdrModalState: {
    error: null,
    finalImageBase64: null,
    isOpen: false,
    isProcessing: false,
    progressMessage: '',
    stitchingSourcePaths: [],
  },
  negativeModalState: { isOpen: false, targetPaths: [] },
  denoiseModalState: {
    isOpen: false,
    isProcessing: false,
    previewBase64: null,
    error: null,
    targetPaths: [],
    progressMessage: null,
    isRaw: false,
  },
  cullingModalState: { isOpen: false, suggestions: null, progress: null, error: null, pathsToCull: [] },
  collageModalState: { isOpen: false, sourceImages: [] },

  setUI: (updater) => set((state) => (typeof updater === 'function' ? updater(state) : updater)),

  // Argentum: a panel opens the column the Modern window shows it in (argentum/modern/panels.ts);
  // a right panel tab is also chosen there.
  setPanel: (panel) =>
    set((state) => {
      if (!panel) return state;
      const column = columnOf(panel);
      const shown = column === 'left' ? 'leftPanel' : 'rightPanel';
      const width = column === 'left' ? 'leftPanelWidth' : 'rightPanelWidth';
      const updates: Partial<UIState> = column === 'right' ? { activePanel: panel } : {};
      if (!state.uiVisibility[shown]) {
        updates.uiVisibility = { ...state.uiVisibility, [shown]: true };
        if (state[width] < DEFAULT_PANEL_WIDTH) updates[width] = DEFAULT_PANEL_WIDTH;
      }
      return updates;
    }),

  toggleFullScreen: () => {
    const { isFullScreen } = get();
    const { zoom, selectedImage } = useEditorStore.getState();

    const isNotFitToScreen = Math.abs(zoom - 1) > 0.01;
    set({ isInstantTransition: isNotFitToScreen });

    if (isFullScreen) {
      set({ isFullScreen: false });
    } else {
      if (!selectedImage) return;
      set({ isFullScreen: true });
    }

    if (isNotFitToScreen) {
      setTimeout(() => set({ isInstantTransition: false }), 100);
    }
  },

  resetWorkspaceLayout: () => {
    const defaultWorkspace = readWorkspace(undefined);
    set({
      ...defaultWorkspace,
      uiVisibility: { filmstrip: true, leftPanel: true, rightPanel: true },
      activePanel: Panel.Adjustments,
    });
    return defaultWorkspace;
  },

  customEscapeHandler: null,
  setCustomEscapeHandler: (handler) => set({ customEscapeHandler: handler }),
  imageSelectHandler: null,
  setImageSelectHandler: (handler) => set({ imageSelectHandler: handler }),
  searchFocusRequest: 0,
  requestSearchFocus: () => set((state) => ({ searchFocusRequest: state.searchFocusRequest + 1 })),
}));
