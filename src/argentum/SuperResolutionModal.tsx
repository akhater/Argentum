import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Eye, EyeOff, Loader2, Save, Sparkles, X, ZoomIn, ZoomOut } from 'lucide-react';
import { ag } from './ag';
import { SUPER_RESOLUTION_OPEN_EVENT } from './superResolution';
import { useAgTranslation } from './locales';
import { useProcessStore } from '../store/useProcessStore';
import { AG_EVENTS, type PreviewPayload } from './ag.generated';

interface ProgressPayload {
  completed: number;
  total: number;
  message: string;
}

interface SuperResolutionModalProps {
  onLibraryRefresh: () => Promise<void>;
  onImageSelect: (path: string) => void;
  /** The request that loaded this window (its open event passed before it mounted). */
  initialPaths?: string[];
}

/** Restore's Mask slider: 60 is the tight end, the default (restore.rs). */
const DEFAULT_MASK = 60;

/**
 * One window, two jobs, one file: Restore undoes lens softness at the photo's
 * own size, Enlarge makes it 2x or 4x bigger. Either, or both - Restore first.
 * Scale 1 is "no enlargement".
 */
export default function SuperResolutionModal({
  onLibraryRefresh,
  onImageSelect,
  initialPaths,
}: SuperResolutionModalProps) {
  const agT = useAgTranslation();
  const modelStatus = useProcessStore((state) => state.aiModelDownloadStatus);
  const [paths, setPaths] = useState<string[]>(initialPaths ?? []);
  const [scale, setScale] = useState(2);
  const [restore, setRestore] = useState(false);
  const [mask, setMask] = useState(DEFAULT_MASK);
  const [preview, setPreview] = useState<PreviewPayload | null>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<string | null>(null);
  const [progressPercent, setProgressPercent] = useState<number | null>(null);
  const [split, setSplit] = useState(50);
  const [zoom, setZoom] = useState(1);
  // The mask view: white where Restore applies. It comes from the photo, not
  // the model, so it shows before Enhance and follows the slider.
  const [showMask, setShowMask] = useState(false);
  const [maskUrl, setMaskUrl] = useState<string | null>(null);
  const [maskBusy, setMaskBusy] = useState(false);
  // The Mask the shown result was blended with; moving the slider re-blends.
  const [previewMask, setPreviewMask] = useState<number | null>(null);

  const reset = () => {
    setScale(2);
    setRestore(false);
    setMask(DEFAULT_MASK);
    setShowMask(false);
    setMaskUrl(null);
    setPreviewMask(null);
  };

  const close = () => {
    if (isProcessing) return;
    // Restore keeps the model's output for a second Enhance; let it go.
    ag('discard_super_resolution').catch(() => {});
    setPaths([]);
    reset();
    setPreview(null);
    setError(null);
    setProgress(null);
    setProgressPercent(null);
    setSplit(50);
    setZoom(1);
  };

  useEffect(() => {
    const handleOpen = (event: Event) => {
      const nextPaths = (event as CustomEvent<{ paths: string[] }>).detail?.paths ?? [];
      if (nextPaths.length === 0) return;
      setPaths(nextPaths);
      reset();
      setPreview(null);
      setError(null);
      setProgress(null);
      setProgressPercent(null);
      setSplit(50);
      setZoom(1);
    };
    window.addEventListener(SUPER_RESOLUTION_OPEN_EVENT, handleOpen);
    return () => window.removeEventListener(SUPER_RESOLUTION_OPEN_EVENT, handleOpen);
  }, []);

  const restoreArg = restore ? mask : null;
  const nothingChosen = !restore && scale === 1;

  const runPreview = async () => {
    if (paths.length === 0 || isProcessing || nothingChosen) return;

    setIsProcessing(true);
    setPreview(null);
    setShowMask(false);
    setError(null);
    setProgress(agT('superResolutionPreparing'));
    setProgressPercent(null);
    try {
      const result = await ag('super_resolution_preview', {
        path: paths[0],
        scale,
        restore: restoreArg,
      });
      setPreview(result);
      setPreviewMask(restore ? mask : null);
      setShowMask(false);
      setProgress(null);
      setProgressPercent(100);
    } catch (reason) {
      setError(String(reason));
      setProgress(null);
      setProgressPercent(null);
    } finally {
      setIsProcessing(false);
    }
  };

  useEffect(() => {
    if (paths.length === 0) return;
    let active = true;
    const unlisten = listen<ProgressPayload | string>(AG_EVENTS.SUPER_RESOLUTION_PROGRESS, (event) => {
      if (!active) return;
      if (typeof event.payload === 'string') {
        setProgress(event.payload);
        setProgressPercent(null);
        return;
      }
      setProgress(event.payload.message);
      setProgressPercent(
        event.payload.total > 0 ? Math.min(100, (event.payload.completed / event.payload.total) * 100) : null,
      );
    });
    return () => {
      active = false;
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, [paths.length]);

  // Redraw the mask as the slider moves; the backend keeps the photo decoded.
  useEffect(() => {
    if (paths.length === 0 || !restore || !showMask) return;
    let active = true;
    const timer = window.setTimeout(async () => {
      setMaskBusy(true);
      try {
        const url = await ag('restore_mask', { path: paths[0], mask });
        if (active) setMaskUrl(url);
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        if (active) setMaskBusy(false);
      }
    }, 120);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [paths, restore, showMask, mask]);

  // Restore on its own: a new Mask re-blends the kept model output, no rerun.
  useEffect(() => {
    if (!preview || !restore || scale !== 1 || previewMask === null || mask === previewMask) return;
    let active = true;
    const timer = window.setTimeout(async () => {
      try {
        const url = await ag('restore_reblend', { mask });
        if (!active) return;
        setPreview((shown) => (shown ? { ...shown, result: url } : shown));
        setPreviewMask(mask);
      } catch (reason) {
        if (active) setError(String(reason));
      }
    }, 150);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [mask, preview, restore, scale, previewMask]);

  const saveOne = async () => {
    setIsProcessing(true);
    setError(null);
    setProgress(agT('superResolutionSaving'));
    setProgressPercent(null);
    try {
      const savedPath = await ag('save_super_resolution', { path: paths[0] });
      await onLibraryRefresh();
      close();
      onImageSelect(savedPath);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setIsProcessing(false);
      setProgress(null);
    }
  };

  const saveBatch = async () => {
    setIsProcessing(true);
    setError(null);
    setProgress(agT('superResolutionBatching'));
    setProgressPercent(null);
    try {
      await ag('batch_super_resolution', { paths, scale, restore: restoreArg });
      await onLibraryRefresh();
      close();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setIsProcessing(false);
      setProgress(null);
    }
  };

  if (paths.length === 0) return null;

  const isBusy = isProcessing || Boolean(modelStatus);

  // Any change asks for a new Enhance. Restore's model output is kept by the
  // backend, so a new mask or scale does not run it again.
  const changed = () => {
    setPreview(null);
    setError(null);
    setProgress(null);
  };

  const chooseScale = (nextScale: number) => {
    if (isBusy) return;
    setScale(nextScale);
    changed();
  };

  // Restore on its own keeps its result and re-blends it (the effect above);
  // with an enlargement after it, a new Mask needs a new Enhance.
  const chooseMask = (nextMask: number) => {
    setMask(nextMask);
    if (scale !== 1) changed();
  };

  const resultLabel = restore
    ? scale > 1
      ? agT('restoreResultEnlarged').replace('{scale}', String(scale))
      : agT('restoreResult')
    : agT('superResolutionResult').replace('{scale}', String(scale));
  const recipeLabel = restore
    ? scale > 1
      ? agT('recipeRestoreEnlarge').replace('{scale}', String(scale))
      : agT('recipeRestore')
    : agT('recipeEnlarge').replace('{scale}', String(scale));
  const maskShown = restore && showMask && maskUrl !== null;

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-6 backdrop-blur-sm">
      <div className="flex max-h-[92vh] w-full max-w-6xl flex-col overflow-hidden rounded-xl border border-border-color bg-bg-primary shadow-2xl">
        <div className="flex items-center justify-between border-b border-border-color px-5 py-3">
          <div className="flex items-center gap-2">
            <Sparkles size={18} className="text-accent" />
            <div>
              <h2 className="text-base font-semibold text-text-primary">{agT('superResolutionTitle')}</h2>
              <p className="text-xs text-text-secondary">{agT('superResolutionSubtitle')}</p>
            </div>
          </div>
          <button
            className="rounded-md p-1.5 text-text-secondary hover:bg-surface hover:text-text-primary"
            onClick={close}
          >
            <X size={18} />
          </button>
        </div>

        <div className="relative min-h-[320px] flex-1 overflow-hidden bg-[#111] p-4">
          {maskShown ? (
            <div className="relative flex h-[60vh] min-h-[320px] items-center justify-center overflow-hidden rounded-lg">
              <div className="absolute inset-0" style={{ transform: `scale(${zoom})` }}>
                <img
                  src={maskUrl ?? undefined}
                  alt={agT('restoreMaskView')}
                  className="absolute inset-0 h-full w-full object-contain"
                />
              </div>
              <div className="absolute left-3 top-3 flex items-center gap-2 rounded bg-black/70 px-2 py-1 text-xs text-white">
                {maskBusy && <Loader2 className="animate-spin" size={12} />}
                {agT('restoreMaskView')}
              </div>
            </div>
          ) : preview ? (
            <div className="relative flex h-[60vh] min-h-[320px] items-center justify-center overflow-hidden rounded-lg">
              {/* Absolutely filled, not h-full: a percentage of a parent with only a
                  min-height resolves to auto, and with both images absolute the box
                  collapsed to nothing - the labels and slider showed, the photos did not. */}
              <div className="absolute inset-0" style={{ transform: `scale(${zoom})` }}>
                <img src={preview.result} alt={resultLabel} className="absolute inset-0 h-full w-full object-contain" />
                <img
                  src={preview.original}
                  alt={agT('superResolutionOriginal')}
                  className="absolute inset-0 h-full w-full object-contain"
                  style={{ clipPath: `inset(0 ${100 - split}% 0 0)` }}
                />
                <div className="absolute inset-y-0 w-0.5 bg-white shadow-lg" style={{ left: `${split}%` }} />
              </div>
              <div className="absolute left-3 top-3 rounded bg-black/60 px-2 py-1 text-xs text-white">
                {agT('superResolutionOriginal')}
              </div>
              <div className="absolute right-3 top-3 rounded bg-accent/90 px-2 py-1 text-xs text-button-text">
                {resultLabel}
              </div>
              <input
                aria-label={agT('superResolutionCompare')}
                className="absolute inset-x-4 bottom-4 accent-accent"
                max={100}
                min={0}
                onChange={(event) => setSplit(Number(event.target.value))}
                type="range"
                value={split}
              />
            </div>
          ) : (
            <div className="flex min-h-[320px] flex-col items-center justify-center gap-3 text-text-secondary">
              {isBusy ? <Loader2 className="animate-spin text-accent" size={28} /> : <Sparkles size={28} />}
              <span>{progress || agT('restoreChoose')}</span>
              {modelStatus && <span className="text-xs">{modelStatus}</span>}
              {isBusy && (
                <div
                  className="h-1.5 w-64 overflow-hidden rounded-full bg-surface"
                  aria-label="Super resolution progress"
                >
                  <div
                    className={`h-full rounded-full bg-accent ${progressPercent === null ? 'animate-pulse' : ''}`}
                    style={{ width: `${progressPercent ?? 35}%` }}
                  />
                </div>
              )}
            </div>
          )}
        </div>

        {error && (
          <div className="border-t border-red-500/30 bg-red-500/10 px-5 py-3 text-sm text-red-300">{error}</div>
        )}

        <div className="flex flex-wrap items-center gap-x-6 gap-y-2 border-t border-border-color px-5 py-3">
          <label className="flex items-center gap-2 text-sm text-text-primary" title={agT('restoreHelp')}>
            <input
              checked={restore}
              className="accent-accent"
              disabled={isBusy}
              onChange={(event) => {
                setRestore(event.target.checked);
                // Show what it will touch straight away: that is the decision.
                setShowMask(event.target.checked);
                // Restore on its own is the usual job, and keeps Capture
                // sharpening live on the file; a 2x got saved by accident when
                // Enlarge stayed at its default. 2x and 4x are a click away.
                if (event.target.checked) setScale(1);
                // Nothing left to do: enlarging is what the window did before.
                if (!event.target.checked && scale === 1) setScale(2);
                changed();
              }}
              type="checkbox"
            />
            {agT('restoreLabel')}
          </label>
          {restore && (
            <div className="flex items-center gap-2 text-xs text-text-secondary" title={agT('restoreMaskHelp')}>
              <button
                aria-label={agT('restoreShowMask')}
                aria-pressed={showMask}
                className={`rounded p-1 hover:bg-surface ${showMask ? 'text-accent' : 'hover:text-text-primary'}`}
                data-tooltip={agT('restoreShowMask')}
                disabled={isBusy}
                onClick={() => setShowMask((on) => !on)}
                type="button"
              >
                {showMask ? <Eye size={15} /> : <EyeOff size={15} />}
              </button>
              <span>{agT('restoreMask')}</span>
              <input
                aria-label={agT('restoreMask')}
                className="w-40 accent-accent"
                disabled={isBusy}
                max={100}
                min={0}
                onChange={(event) => chooseMask(Number(event.target.value))}
                type="range"
                value={mask}
              />
              <span className="w-7 text-right tabular-nums">{mask}</span>
              {mask !== DEFAULT_MASK && (
                <button
                  className="rounded px-1.5 py-0.5 hover:bg-surface hover:text-text-primary"
                  disabled={isBusy}
                  onClick={() => chooseMask(DEFAULT_MASK)}
                  type="button"
                >
                  {agT('restoreMaskReset')}
                </button>
              )}
            </div>
          )}
          <div className="ml-auto flex items-center gap-1 rounded-md border border-border-color p-0.5">
            <span className="px-1.5 text-xs text-text-secondary">{agT('superResolutionScaleLabel')}</span>
            {[1, 2, 4].map((option) => (
              <button
                className={`rounded px-2 py-1 text-xs disabled:opacity-40 ${
                  scale === option ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-surface'
                }`}
                disabled={isBusy || (option === 1 && !restore)}
                key={option}
                onClick={() => chooseScale(option)}
                title={option === 1 && !restore ? agT('enlargeOffNeedsRestore') : undefined}
                type="button"
              >
                {option === 1 ? agT('enlargeOff') : `${option}×`}
              </button>
            ))}
          </div>
        </div>

        <div className="flex items-center justify-between border-t border-border-color px-5 py-3">
          <div className="flex items-center gap-2 text-text-secondary">
            <button
              aria-label={agT('superResolutionZoomOut')}
              className="rounded p-1.5 hover:bg-surface disabled:opacity-40"
              disabled={!preview}
              onClick={() => setZoom((value) => Math.max(0.5, value - 0.25))}
            >
              <ZoomOut size={16} />
            </button>
            <span className="min-w-12 text-center text-xs">{Math.round(zoom * 100)}%</span>
            <button
              aria-label={agT('superResolutionZoomIn')}
              className="rounded p-1.5 hover:bg-surface disabled:opacity-40"
              disabled={!preview}
              onClick={() => setZoom((value) => Math.min(2, value + 0.25))}
            >
              <ZoomIn size={16} />
            </button>
            <span className="ml-2 text-xs">
              {paths.length > 1
                ? agT('superResolutionBatchCount').replace('{count}', String(paths.length))
                : recipeLabel}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              className="rounded-md px-3 py-2 text-sm text-text-secondary hover:bg-surface"
              disabled={isBusy}
              onClick={close}
              type="button"
            >
              {agT('superResolutionCancel')}
            </button>
            {!preview ? (
              <button
                className="flex items-center gap-2 rounded-md bg-accent px-3 py-2 text-sm font-medium text-button-text disabled:opacity-50"
                disabled={isBusy || nothingChosen}
                onClick={runPreview}
                type="button"
              >
                {isBusy ? <Loader2 className="animate-spin" size={15} /> : <Sparkles size={15} />}
                {agT('superResolutionEnhance')}
              </button>
            ) : (
              <>
                {paths.length > 1 && (
                  <button
                    className="flex items-center gap-2 rounded-md bg-accent px-3 py-2 text-sm font-medium text-button-text disabled:opacity-50"
                    disabled={!preview || isBusy}
                    onClick={saveBatch}
                    type="button"
                  >
                    {isBusy ? <Loader2 className="animate-spin" size={15} /> : <Save size={15} />}
                    {agT('superResolutionApplyBatch')}
                  </button>
                )}
                <button
                  className="flex items-center gap-2 rounded-md bg-accent px-3 py-2 text-sm font-medium text-button-text disabled:opacity-50"
                  disabled={!preview || isBusy}
                  onClick={saveOne}
                  type="button"
                >
                  {isBusy ? <Loader2 className="animate-spin" size={15} /> : <Save size={15} />}
                  {agT('superResolutionSave')}
                </button>
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
