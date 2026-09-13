/**
 * The TIFF bit depth chooser. Ours.
 *
 * WHY THERE IS A CHOICE AT ALL
 *
 * Because the two answers are for different jobs, not because one is better. A
 * TIFF you are handing to a client is a delivery and 8 bits is the right size
 * for it; a TIFF you are taking into Photoshop is a master and wants 16. Every
 * other format in the panel is 8-bit by definition, so this appears for TIFF
 * and nothing else.
 *
 * WHY IT IS NOT IN THE EXPORT SETTINGS OBJECT
 *
 * Upstream's own version of this (pull request #1466) puts `tiffBitDepth` on
 * `ExportSettings`, into their presets, and threads it through six of their
 * files down to a fourth parameter on the encoder. That is the pattern
 * `CLAUDE.md` names as the thing that kills a fork - and it would also mean this
 * feature owning a field in a struct upstream edits every release.
 *
 * So the depth is Argentum's own preference, stored beside the profile library
 * in `argentum-processing.json`, exactly as highlight recovery is. Their panel
 * carries one marker line and nothing else. The cost of the next Argentum export
 * control is zero lines of theirs, which is the entire point.
 *
 * WHAT IT DOES NOT SAY
 *
 * It does not claim 16-bit is better looking, because it is not: at the
 * precision this pipeline reaches, the difference is invisible in a photograph
 * and only shows when the file is pushed hard somewhere else afterwards. The
 * labels say what each is *for*, which is the true and useful difference.
 */

import { useCallback, useEffect, useState } from 'react';
import { ag } from './ag';
import { useAgTranslation } from './locales';

/** The depths Argentum can write. Matches `TiffDepth` on the Rust side. */
const DEPTHS = [8, 16] as const;
type Depth = (typeof DEPTHS)[number];

export default function ExportPrecision() {
  const t = useAgTranslation();
  const [depth, setDepth] = useState<Depth | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const value = await ag<number>('tiff_bit_depth');
      // Anything we do not recognise is 16, which is what an export did before
      // this control existed. A preference file from a newer Argentum must not
      // leave an older one with no selection at all.
      setDepth(value === 8 ? 8 : 16);
    } catch {
      setDepth(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const choose = async (next: Depth) => {
    if (depth === null || busy || next === depth) {
      return;
    }
    setBusy(true);
    // Shown at once: the write is one line of JSON, and a control that lags a
    // click reads as broken.
    const previous = depth;
    setDepth(next);
    try {
      await ag('set_tiff_bit_depth', { depth: next });
    } catch {
      setDepth(previous);
    } finally {
      setBusy(false);
    }
  };

  // Their panel renders the marker only while TIFF is the chosen format, so this
  // component existing at all *is* the condition - no prop, and no state of
  // theirs read from in here. Null while the preference loads, so the control
  // does not flicker between depths on the way in.
  if (depth === null) {
    return null;
  }

  return (
    <div className="mt-2">
      <p className="text-xs text-text-secondary mb-1.5">{t('tiffDepthLabel')}</p>
      <div className="grid grid-cols-2 gap-2">
        {DEPTHS.map((value) => (
          <button
            className={`px-2 py-1.5 rounded-md transition-colors disabled:opacity-50 ${
              depth === value ? 'bg-accent' : 'bg-surface hover:bg-card-active'
            }`}
            disabled={busy}
            key={value}
            onClick={() => choose(value)}
            title={t(value === 8 ? 'tiffDepth8Help' : 'tiffDepth16Help')}
            type="button"
          >
            <span className={`text-sm ${depth === value ? 'text-button-text' : 'text-text-secondary'}`}>
              {t(value === 8 ? 'tiffDepth8' : 'tiffDepth16')}
            </span>
          </button>
        ))}
      </div>
      <p className="text-xs text-text-secondary mt-1.5">
        {t(depth === 8 ? 'tiffDepth8Help' : 'tiffDepth16Help')}
      </p>
    </div>
  );
}
