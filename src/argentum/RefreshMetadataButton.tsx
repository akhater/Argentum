/**
 * "Re-read metadata" button. Ours.
 *
 * EXIF is cached in two places that outlive a fix: inside a photo's `.agdata`
 * sidecar, and in a per-folder JSON keyed on the file's mtime and size. Neither
 * is invalidated when the app changes, only when the photo does — which it
 * never does.
 *
 * When lens reading was fixed, already-edited photos kept a frozen snapshot with
 * an empty lens and went on failing while their neighbours worked. It looked
 * intermittent and took a long time to pin on caching. This is the escape hatch,
 * so that never needs diagnosing again.
 *
 * Clears the cache and puts the freshly-read EXIF straight into the store. The
 * first attempt called `window.location.reload()`, which does refresh the data
 * and also throws away the session and returns to the welcome screen — far too
 * much for one field.
 *
 * Edits, ratings and tags are untouched. Only the derived EXIF copy goes.
 */

import { ag } from './ag';
import { useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { useEditorStore } from '../store/useEditorStore';

export default function RefreshMetadataButton() {
  const [busy, setBusy] = useState(false);

  const refresh = async () => {
    const store = useEditorStore.getState() as any;
    const selected = store.selectedImage;
    if (!selected?.path || busy) {
      return;
    }

    setBusy(true);
    try {
      const exif: Record<string, string> = await ag('refresh_image_metadata', {
        path: selected.path,
      });

      // Replace the EXIF in place. Their panel and the lens auto-detect both
      // read it from here, so both pick it up on the next render.
      useEditorStore.setState((state: any) => ({
        selectedImage: { ...state.selectedImage, exif },
      }));
    } catch (err) {
      console.error('Refresh metadata failed:', err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      className="p-1 rounded-md text-text-secondary hover:text-text-primary hover:bg-bg-secondary transition-colors disabled:opacity-50"
      onClick={refresh}
      disabled={busy}
      data-tooltip="Re-read metadata from the file"
    >
      <RefreshCw size={14} className={busy ? 'animate-spin' : ''} />
    </button>
  );
}
