import { lazy, useEffect, useState } from 'react';
import { LazyDialog } from '../components/modals/LazyDialog';
import { SUPER_RESOLUTION_OPEN_EVENT } from './superResolution';

const SuperResolutionModal = lazy(() => import('./SuperResolutionModal'));

/**
 * The Super Resolution window opens by an event, so something must be listening
 * before its code exists. This is that listener: it holds the paths of the first
 * request, loads the window on it, and hands them over (the window listens for
 * later requests itself, and the event that woke this one has already passed).
 */
export default function LazySuperResolutionModal({
  onLibraryRefresh,
  onImageSelect,
}: {
  onLibraryRefresh: () => Promise<void>;
  onImageSelect: (path: string) => void;
}) {
  const [firstPaths, setFirstPaths] = useState<string[] | null>(null);

  useEffect(() => {
    const handleOpen = (event: Event) => {
      const paths = (event as CustomEvent<{ paths: string[] }>).detail?.paths ?? [];
      if (paths.length > 0) setFirstPaths((current) => current ?? paths);
    };
    window.addEventListener(SUPER_RESOLUTION_OPEN_EVENT, handleOpen);
    return () => window.removeEventListener(SUPER_RESOLUTION_OPEN_EVENT, handleOpen);
  }, []);

  return (
    <LazyDialog isOpen={firstPaths !== null} onClose={() => setFirstPaths(null)}>
      <SuperResolutionModal
        initialPaths={firstPaths ?? undefined}
        onImageSelect={onImageSelect}
        onLibraryRefresh={onLibraryRefresh}
      />
    </LazyDialog>
  );
}
