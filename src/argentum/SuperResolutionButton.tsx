import { Sparkles } from 'lucide-react';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useAgTranslation } from './locales';
import { openSuperResolution } from './superResolution';

export default function SuperResolutionButton() {
  const agT = useAgTranslation();
  const selectedImage = useEditorStore((state) => state.selectedImage);
  const selectedPaths = useLibraryStore((state) => state.multiSelectedPaths);
  const paths = selectedPaths.length > 0 ? selectedPaths : selectedImage ? [selectedImage.path] : [];

  if (!selectedImage?.isReady) return null;

  return (
    <button
      aria-label={agT('superResolutionToolbar')}
      className="flex h-7 w-7 items-center justify-center rounded text-text-secondary transition-colors hover:bg-surface hover:text-accent"
      data-tooltip={agT('superResolutionToolbar')}
      onClick={() => openSuperResolution(paths)}
      type="button"
    >
      <Sparkles size={15} />
    </button>
  );
}
