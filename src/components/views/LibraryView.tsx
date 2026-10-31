import { useShallow } from 'zustand/react/shallow';

import CommunityPage from '../panel/CommunityPage';
import MainLibrary from '../panel/MainLibrary';
import { leaveCommunity } from '../../argentum/looks/community';

import { useUIStore } from '../../store/useUIStore';
import { useLibraryStore } from '../../store/useLibraryStore';
import { useProcessStore } from '../../store/useProcessStore';
import { useSettingsStore } from '../../store/useSettingsStore';

import { ImageFile, LibraryViewMode, ThumbnailAspectRatio, ThumbnailSize } from '../ui/AppProperties';
import { GroupBadgeInfo, GroupId } from '../../utils/imageGrouping';

interface LibraryViewProps {
  sortedImageList: ImageFile[];
  groupBadgeInfo: Map<GroupId, GroupBadgeInfo> | null;
  thumbnailSize: ThumbnailSize;
  thumbnailAspectRatio: ThumbnailAspectRatio;
  libraryViewMode: LibraryViewMode;
  setThumbnailSize: (size: ThumbnailSize) => void;
  handleClearSelection: () => void;
  handleLibraryImageSingleClick: (...args: any) => void;
  handleImageSelect: (...args: any) => void;
  handleThumbnailContextMenu: (...args: any) => void;
  handleMainLibraryContextMenu: (...args: any) => void;
  requestThumbnails: any;
}

export default function LibraryView({
  sortedImageList,
  groupBadgeInfo,
  thumbnailSize,
  thumbnailAspectRatio,
  libraryViewMode,
  setThumbnailSize,
  handleClearSelection,
  handleLibraryImageSingleClick,
  handleImageSelect,
  handleThumbnailContextMenu,
  handleMainLibraryContextMenu,
  requestThumbnails,
}: LibraryViewProps) {
  const { activeView } = useUIStore(
    useShallow((state) => ({
      activeView: state.activeView,
    })),
  );

  const { rootPaths, currentFolderPath, libraryActivePath, multiSelectedPaths, imageRatings } = useLibraryStore(
    useShallow((state) => ({
      rootPaths: state.rootPaths,
      currentFolderPath: state.currentFolderPath,
      libraryActivePath: state.libraryActivePath,
      multiSelectedPaths: state.multiSelectedPaths,
      imageRatings: state.imageRatings,
    })),
  );

  const { appSettings, supportedTypes } = useSettingsStore(
    useShallow((state) => ({
      appSettings: state.appSettings,
      supportedTypes: state.supportedTypes,
    })),
  );

  const { aiModelDownloadStatus, indexingProgress, isIndexing } = useProcessStore(
    useShallow((state) => ({
      aiModelDownloadStatus: state.aiModelDownloadStatus,
      indexingProgress: state.indexingProgress,
      isIndexing: state.isIndexing,
    })),
  );

  return (
    <div className="flex flex-row grow h-full min-h-0">
      <div className="flex-1 flex flex-col min-w-0 gap-2">
        {activeView === 'community' ? (
          <CommunityPage
            onBackToLibrary={leaveCommunity}
            supportedTypes={supportedTypes}
            imageList={sortedImageList}
            currentFolderPath={currentFolderPath}
          />
        ) : (
          <MainLibrary
            activePath={libraryActivePath}
            aiModelDownloadStatus={aiModelDownloadStatus}
            appSettings={appSettings}
            currentFolderPath={currentFolderPath}
            groupBadgeInfo={groupBadgeInfo}
            imageList={sortedImageList}
            imageRatings={imageRatings}
            indexingProgress={indexingProgress}
            isIndexing={isIndexing}
            libraryViewMode={libraryViewMode}
            multiSelectedPaths={multiSelectedPaths}
            onClearSelection={handleClearSelection}
            onContextMenu={handleThumbnailContextMenu}
            onEmptyAreaContextMenu={handleMainLibraryContextMenu}
            onImageClick={handleLibraryImageSingleClick}
            onImageDoubleClick={handleImageSelect}
            onThumbnailSizeChange={setThumbnailSize}
            onRequestThumbnails={requestThumbnails}
            rootPaths={rootPaths}
            thumbnailAspectRatio={thumbnailAspectRatio}
            thumbnailSize={thumbnailSize}
          />
        )}
      </div>
    </div>
  );
}
