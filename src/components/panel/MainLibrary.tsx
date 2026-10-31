import { useMemo } from 'react';
import { Loader2, Search, SlidersHorizontal } from 'lucide-react';
import CullingView from './library/CullingView';
import { useTranslation } from 'react-i18next';
import {
  AppSettings,
  ImageFile,
  LibraryViewMode,
  Progress,
  ThumbnailSize,
  ThumbnailAspectRatio,
  LibraryDisplayMode,
} from '../ui/AppProperties';
import { GroupBadgeInfo, GroupId } from '../../utils/imageGrouping';
import Text from '../ui/Text';
import { TextColors, TextVariants } from '../../types/typography';
import { useLibraryStore } from '../../store/useLibraryStore';

import LibraryGrid from './library/LibraryGrid';
import ModernEmpty from '../../argentum/modern/ModernEmpty';
import NoPhotosMatch from '../../argentum/modern/NoPhotosMatch';
import { activeFilterNames } from '../../argentum/modern/shownFilters';
import { useAgTranslation } from '../../argentum/locales';

export interface ColumnWidths {
  thumbnail: number;
  name: number;
  date: number;
  rating: number;
  color: number;
  shutter: number;
  aperture: number;
  iso: number;
  focal: number;
}

interface MainLibraryProps {
  activePath: string | null;
  aiModelDownloadStatus: string | null;
  appSettings: AppSettings | null;
  currentFolderPath: string | null;
  groupBadgeInfo: Map<GroupId, GroupBadgeInfo> | null;
  imageList: Array<ImageFile>;
  imageRatings: Record<string, number>;
  indexingProgress: Progress;
  isIndexing: boolean;
  libraryViewMode: LibraryViewMode;
  multiSelectedPaths: Array<string>;
  onClearSelection(): void;
  onContextMenu(event: any, path: string): void;
  onEmptyAreaContextMenu(event: any): void;
  onImageClick(path: string, event: any): void;
  onImageDoubleClick(path: string): void;
  onThumbnailSizeChange(size: ThumbnailSize): void;
  onRequestThumbnails?(paths: string[]): void;
  rootPaths: string[];
  thumbnailAspectRatio: ThumbnailAspectRatio;
  thumbnailSize: ThumbnailSize;
}

/**
 * Argentum: the Modern window's grid. No header (its top bar has the path and
 * the view tools), no start screen (it opens on the folder column), and its
 * own empty view until a folder is picked.
 */
export default function MainLibrary(props: MainLibraryProps) {
  const { t } = useTranslation();

  const libraryDisplayMode = props.appSettings?.libraryDisplayMode || LibraryDisplayMode.Grid;

  const searchCriteria = useLibraryStore((state) => state.searchCriteria);
  const activeFilters = activeFilterNames(
    useLibraryStore((state) => state.filterCriteria),
    useAgTranslation(),
  );

  const translatedThumbnailSizeOptions = useMemo(
    () => [
      { id: ThumbnailSize.Small, label: t('library.thumbnailSize.small'), size: 160 },
      { id: ThumbnailSize.Medium, label: t('library.thumbnailSize.medium'), size: 240 },
      { id: ThumbnailSize.Large, label: t('library.thumbnailSize.large'), size: 320 },
    ],
    [t],
  );

  if (!props.currentFolderPath || !props.rootPaths?.length) {
    return <ModernEmpty />;
  }

  return (
    <div className="relative z-20 flex-1 flex flex-col min-w-0 min-h-0 overflow-visible agm-grid-root">
      {props.imageList.length > 0 ? (
        libraryDisplayMode === LibraryDisplayMode.Cull ? (
          <CullingView {...props} />
        ) : (
          <LibraryGrid
            {...props}
            libraryDisplayMode={libraryDisplayMode}
            thumbnailSizeOptions={translatedThumbnailSizeOptions}
          />
        )
      ) : props.isIndexing || props.aiModelDownloadStatus ? (
        <div className="flex-1 flex flex-col items-center justify-center" onContextMenu={props.onEmptyAreaContextMenu}>
          <Loader2 className="h-12 w-12 text-secondary animate-spin mb-4" />
          <Text variant={TextVariants.heading} color={TextColors.secondary}>
            {props.aiModelDownloadStatus
              ? t('library.status.downloading', { status: props.aiModelDownloadStatus })
              : props.isIndexing && props.indexingProgress.total > 0
                ? t('library.status.indexing', {
                    current: props.indexingProgress.current,
                    total: props.indexingProgress.total,
                  })
                : t('library.status.processing')}
          </Text>
          <Text className="mt-2">{t('library.status.moment')}</Text>
        </div>
      ) : searchCriteria.tags.length > 0 || searchCriteria.text ? (
        <div
          className="flex-1 flex flex-col items-center justify-center text-text-secondary text-center"
          onContextMenu={props.onEmptyAreaContextMenu}
        >
          <Search className="h-12 w-12 text-secondary mb-4" />
          <Text variant={TextVariants.heading} color={TextColors.secondary}>
            {t('library.search.noResults')}
          </Text>
          <Text className="mt-2 max-w-sm">
            {t('library.search.noResultsDesc')}
            {!props.appSettings?.enableAiTagging && t('library.search.noResultsAiHint')}
          </Text>
        </div>
      ) : activeFilters ? (
        <NoPhotosMatch filters={activeFilters} onContextMenu={props.onEmptyAreaContextMenu} />
      ) : (
        <div className="flex-1 flex flex-col items-center justify-center" onContextMenu={props.onEmptyAreaContextMenu}>
          <SlidersHorizontal className="h-12 w-12 mb-4 text-text-secondary" />
          <Text>{t('library.filters.noMatch')}</Text>
        </div>
      )}
    </div>
  );
}
