import React, { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, CheckCircle2, ChevronDown, Loader2, Search, Users, Layers, Crop } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import Button from '../ui/Button';
import Input from '../ui/Input';
import { Invokes, SupportedTypes, ImageFile } from '../ui/AppProperties';
import { INITIAL_ADJUSTMENTS } from '../../utils/adjustments';
import Text from '../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../types/typography';
import Dropdown from '../ui/Dropdown';
import { useUIStore } from '../../store/useUIStore';
import { noteCommunitySave } from '../../argentum/looks/community';
import { revokeBlobUrls } from '../../argentum/objectUrl';

// Argentum: the GitHub mark from simple-icons 16.28.0 (siGithub, CC0-1.0), the one icon used from that package.
const GITHUB_NAME = 'GitHub';
const GITHUB_MARK =
  'M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12';

const DEFAULT_PREVIEW_IMAGE_URL = 'https://raw.githubusercontent.com/CyberTimon/RapidRAW-Presets/main/sample-image.jpg';

interface CommunityPreset {
  name: string;
  creator: string;
  adjustments: Record<string, any>;
  includeMasks?: boolean;
  includeCropTransform?: boolean;
  presetType?: 'tool' | 'style';
}

const containerVariants = {
  hidden: { opacity: 1 },
  visible: {
    opacity: 1,
    transition: {
      staggerChildren: 0.06,
    },
  },
};

const itemVariants = {
  hidden: { y: 20, opacity: 0 },
  visible: {
    y: 0,
    opacity: 1,
  },
};

interface CommunityPageProps {
  onBackToLibrary: () => void;
  supportedTypes: SupportedTypes | null;
  imageList: ImageFile[];
  currentFolderPath: string | null;
}

const shuffleArray = (array: any[]) => {
  const newArray = [...array];
  for (let i = newArray.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [newArray[i], newArray[j]] = [newArray[j], newArray[i]];
  }
  return newArray;
};

const CommunityPage = ({ onBackToLibrary, imageList, currentFolderPath }: CommunityPageProps) => {
  const { t } = useTranslation();
  const [presets, setPresets] = useState<CommunityPreset[]>([]);
  const [previews, setPreviews] = useState<Record<string, string | null>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [previewImagePaths, setPreviewImagePaths] = useState<string[]>([]);
  const [searchTerm, setSearchTerm] = useState('');
  const [sortBy, setSortBy] = useState('name');
  const searchInputRef = useRef<HTMLInputElement>(null);
  const searchFocusRequest = useUIStore((state) => state.searchFocusRequest);
  const lastSearchFocusRequest = useRef(searchFocusRequest);
  const [downloadStatus, setDownloadStatus] = useState<Record<string, 'idle' | 'downloading' | 'success'>>({});
  const [allPreviewsLoaded, setAllPreviewsLoaded] = useState(false);

  const sortMethods = useMemo(() => [{ value: 'name', label: t('library.community.sortMethods.name') }], [t]);

  const previewsRef = useRef(previews);
  previewsRef.current = previews;

  const fetchDefaultPreviewImage = useCallback(async (): Promise<string | null> => {
    try {
      const response = await fetch(DEFAULT_PREVIEW_IMAGE_URL);
      const blob = await response.blob();
      const tempPath: string = await invoke(Invokes.SaveTempFile, {
        bytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
      });
      return tempPath;
    } catch (error) {
      console.error('Failed to fetch default preview image:', error);
      return null;
    }
  }, []);

  useEffect(() => {
    const fetchPresets = async () => {
      setIsLoading(true);
      try {
        const communityPresets: CommunityPreset[] = await invoke(Invokes.FetchCommunityPresets);
        setPresets(communityPresets);
      } catch (error) {
        console.error('Failed to fetch community presets:', error);
      } finally {
        setIsLoading(false);
      }
    };

    fetchPresets();

    return () => {
      revokeBlobUrls(Object.values(previewsRef.current));
    };
  }, []);

  useEffect(() => {
    const setupPreviewImages = async () => {
      revokeBlobUrls(Object.values(previewsRef.current));
      setPreviews({});
      // RapidRAW 9e518220: src/components/panel/CommunityPage.tsx. A cloud-only
      // file (a OneDrive placeholder) would be downloaded to draw a preview.
      const localImages = imageList.filter((image) => !image.is_cloud_placeholder);

      if (!currentFolderPath || localImages.length === 0) {
        const defaultPath = await fetchDefaultPreviewImage();
        if (defaultPath) {
          setPreviewImagePaths([defaultPath]);
        }
        return;
      }

      const shuffled = shuffleArray(localImages);

      if (localImages.length === 1) {
        setPreviewImagePaths([localImages[0].path]);
      } else if (localImages.length >= 2 && localImages.length <= 3) {
        setPreviewImagePaths(shuffled.slice(0, 2).map((img) => img.path));
      } else if (localImages.length >= 4) {
        setPreviewImagePaths(shuffled.slice(0, 4).map((img) => img.path));
      }
    };

    setupPreviewImages();
  }, [imageList, currentFolderPath, fetchDefaultPreviewImage]);

  useEffect(() => {
    if (presets.length === 0 || previewImagePaths.length === 0) {
      return;
    }

    const generateAllPreviews = async () => {
      setAllPreviewsLoaded(false);
      try {
        const previewDataMap: Record<string, number[]> = await invoke(Invokes.GenerateAllCommunityPreviews, {
          imagePaths: previewImagePaths,
          presets: presets.map((p) => ({
            ...p,
            adjustments: { ...INITIAL_ADJUSTMENTS, ...p.adjustments },
          })),
        });

        const newPreviews: Record<string, string | null> = {};
        for (const [presetName, imageData] of Object.entries(previewDataMap)) {
          const blob = new Blob([new Uint8Array(imageData)], { type: 'image/jpeg' });
          newPreviews[presetName] = URL.createObjectURL(blob);
        }

        if (cancelled) {
          revokeBlobUrls(Object.values(newPreviews));
          return;
        }
        revokeBlobUrls(Object.values(previewsRef.current));
        setPreviews(newPreviews);
      } catch (error) {
        console.error(`Failed to generate previews:`, error);
      } finally {
        if (!cancelled) setAllPreviewsLoaded(true);
      }
    };

    let cancelled = false;
    generateAllPreviews();
    return () => {
      cancelled = true;
    };
  }, [presets, previewImagePaths]);

  const handleDownloadPreset = async (preset: CommunityPreset) => {
    setDownloadStatus((prev) => ({ ...prev, [preset.name]: 'downloading' }));
    try {
      if (!preset.adjustments) {
        throw new Error('Preset adjustments are missing.');
      }

      await invoke(Invokes.SaveCommunityPreset, {
        name: preset.name,
        adjustments: preset.adjustments,
        includeMasks: preset.includeMasks,
        includeCropTransform: preset.includeCropTransform,
        presetType: preset.presetType || 'style',
      });
      setDownloadStatus((prev) => ({ ...prev, [preset.name]: 'success' }));
      noteCommunitySave();
    } catch (error) {
      console.error(`Failed to download preset ${preset.name}:`, error);
      setDownloadStatus((prev) => ({ ...prev, [preset.name]: 'idle' }));
    }
  };

  useEffect(() => {
    if (searchFocusRequest === lastSearchFocusRequest.current) return;
    lastSearchFocusRequest.current = searchFocusRequest;
    searchInputRef.current?.focus();
  }, [searchFocusRequest]);

  const filteredAndSortedPresets = useMemo(() => {
    return presets
      .filter((p) => p.name.toLowerCase().includes(searchTerm.toLowerCase()))
      .sort((a, b) => {
        if (sortBy === 'name') {
          return a.name.localeCompare(b.name);
        }
        return 0;
      });
  }, [presets, searchTerm, sortBy]);

  return (
    <div className="flex-1 flex flex-col h-full min-w-0 bg-bg-secondary rounded-lg overflow-hidden p-4">
      <header className="shrink-0 flex items-center justify-between mb-4 flex-wrap gap-4">
        <div className="flex items-center">
          <Button
            className="mr-4 hover:bg-surface text-text-primary rounded-full"
            onClick={onBackToLibrary}
            size="icon"
            variant="ghost"
          >
            <ArrowLeft />
          </Button>
          <div>
            <Text variant={TextVariants.headline} className="flex items-center gap-2">
              <Users /> {t('library.community.headerTitle')}
            </Text>
            <Text>{t('library.community.headerDesc')}</Text>
          </div>
        </div>
      </header>

      <div className="flex justify-between items-center mb-4 flex-wrap gap-4">
        <div className="relative">
          <Input
            ref={searchInputRef}
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            placeholder={t('library.community.searchPlaceholder')}
            className="pl-10 w-64"
          />
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-text-secondary" />
        </div>
        <div className="flex items-center gap-2 text-sm">
          <Text variant={TextVariants.label}>{t('library.community.sortBy')}</Text>
          <Dropdown options={sortMethods} value={sortBy} onChange={(value) => setSortBy(value)} />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto custom-scrollbar pr-2 -mr-2">
        {isLoading ? (
          <Text
            variant={TextVariants.heading}
            color={TextColors.secondary}
            weight={TextWeights.normal}
            className="flex items-center justify-center h-full "
          >
            <Loader2 className="h-8 w-8 animate-spin mr-2" />
            {t('library.community.fetchingPresets')}
          </Text>
        ) : (
          <motion.div
            className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4"
            variants={containerVariants}
            initial="hidden"
            animate="visible"
          >
            <AnimatePresence>
              {filteredAndSortedPresets.map((preset) => {
                const previewUrl = previews[preset.name];
                const status = downloadStatus[preset.name] || 'idle';

                return (
                  <motion.div
                    key={preset.name}
                    layout
                    variants={itemVariants}
                    exit={{ opacity: 0, scale: 0.9 }}
                    className="bg-surface rounded-lg overflow-hidden group border border-border-color flex flex-col"
                  >
                    <div className="relative w-full aspect-square bg-bg-primary flex items-center justify-center">
                      {previewUrl ? (
                        <img
                          src={previewUrl}
                          alt={preset.name}
                          className="w-full h-full object-cover transition-all duration-300 group-hover:blur-xs group-hover:brightness-75"
                        />
                      ) : (
                        <Loader2 className="h-8 w-8 animate-spin text-text-secondary" />
                      )}

                      <div className="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity duration-300">
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => handleDownloadPreset(preset)}
                          disabled={status !== 'idle'}
                          className="shadow-lg"
                        >
                          {status === 'idle' && <>{t('library.community.actionSave')}</>}
                          {status === 'downloading' && (
                            <>
                              <Loader2 size={14} className="mr-2 animate-spin" /> {t('library.community.actionSaving')}
                            </>
                          )}
                          {status === 'success' && (
                            <>
                              <CheckCircle2 size={14} className="mr-2" /> {t('library.community.actionSaved')}
                            </>
                          )}
                        </Button>
                      </div>
                    </div>
                    <div className="p-4 text-center">
                      <Text variant={TextVariants.heading} className="truncate mb-1">
                        {preset.name}
                      </Text>
                      <Text variant={TextVariants.small} className="font-['cursive'] italic">
                        {t('library.community.presetBy', { creator: preset.creator })}
                      </Text>
                    </div>
                  </motion.div>
                );
              })}
            </AnimatePresence>
          </motion.div>
        )}
        <motion.div
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, delay: 0.3 }}
          className="text-center mt-8 py-4"
        >
          <Text>
            {t('library.community.footerHeading')}
            <br />
            <a
              href="https://github.com/CyberTimon/RapidRAW-Presets/issues/new?assignees=&labels=preset-submission&template=preset_submission.md&title=Preset+Submission%3A+%5BYour+Preset+Name%5D"
              target="_blank"
              rel="noopener noreferrer"
              className="text-accent hover:underline inline-flex items-center gap-2"
            >
              <span style={{ display: 'inline-block', width: 14, height: 14 }}>
                <svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="currentColor">
                  <title>{GITHUB_NAME}</title>
                  <path d={GITHUB_MARK} />
                </svg>
              </span>
              {t('library.community.footerLinkText')}
            </a>
          </Text>
        </motion.div>
      </div>
    </div>
  );
};

export default CommunityPage;
