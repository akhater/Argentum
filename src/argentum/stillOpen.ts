import { useEditorStore } from '../store/useEditorStore';

/**
 * The edit as it is now, when `path` is still the open photo; null once another photo (or none) is open.
 * Called after every await that applies a result to "the open photo", so a late answer for photo A is
 * never written into photo B.
 */
export function stillOpen(path: string | undefined) {
  const { selectedImage, adjustments, setEditor } = useEditorStore.getState();
  return selectedImage?.path === path ? { adjustments, setEditor } : null;
}
