import { Sparkles } from 'lucide-react';
import { useEditorStore } from '../store/useEditorStore';
import { useLibraryStore } from '../store/useLibraryStore';
import { useSettingsStore } from '../store/useSettingsStore';
import { useAgTranslation } from './locales';
import { openSuperResolution } from './superResolution';

export default function SuperResolutionButton() {
  const agT = useAgTranslation();
  const selectedImage = useEditorStore((state) => state.selectedImage);
  const selectedPaths = useLibraryStore((state) => state.multiSelectedPaths);
  const paths = selectedPaths.length > 0 ? selectedPaths : selectedImage ? [selectedImage.path] : [];
  // RapidRAW 1.6.5's AI-Free mode hides every AI control, theirs through this
  // same setting. Enlarging is AI, so it goes with them.
  const isAiFree = useSettingsStore((s) => s.appSettings?.aiProvider === 'ai-free');

  if (!selectedImage?.isReady || isAiFree) return null;

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
