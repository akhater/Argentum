import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Loader2, Save, Sparkles, X, ZoomIn, ZoomOut } from 'lucide-react';
import { ag } from './ag';
import { SUPER_RESOLUTION_OPEN_EVENT } from './superResolution';
import { useAgTranslation } from './locales';
import { useProcessStore } from '../store/useProcessStore';

interface PreviewPayload {
  result: string;
  original: string;
}

interface ProgressPayload {
  completed: number;
  total: number;
  message: string;
}

interface SuperResolutionModalProps {
  onLibraryRefresh: () => Promise<void>;
  onImageSelect: (path: string) => void;
}

export default function SuperResolutionModal({ onLibraryRefresh, onImageSelect }: SuperResolutionModalProps) {
  const agT = useAgTranslation();
  const modelStatus = useProcessStore((state) => state.aiModelDownloadStatus);
  const [paths, setPaths] = useState<string[]>([]);
  const [scale, setScale] = useState(2);
  const [preview, setPreview] = useState<PreviewPayload | null>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<string | null>(null);
  const [progressPercent, setProgressPercent] = useState<number | null>(null);
  const [split, setSplit] = useState(50);
  const [zoom, setZoom] = useState(1);

  const close = () => {
    if (isProcessing) return;
    setPaths([]);
    setScale(2);
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
      setScale(2);
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

  const runPreview = async () => {
    if (paths.length === 0 || isProcessing) return;

    setIsProcessing(true);
    setPreview(null);
    setError(null);
    setProgress(agT('superResolutionPreparing').replace('{scale}', String(scale)));
    setProgressPercent(null);
    try {
      const result = await ag<PreviewPayload>('super_resolution_preview', { path: paths[0], scale });
      setPreview(result);
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
    const unlisten = listen<ProgressPayload | string>('super-resolution-progress', (event) => {
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

  const saveOne = async () => {
    setIsProcessing(true);
    setError(null);
    setProgress(agT('superResolutionSaving'));
    setProgressPercent(null);
    try {
      const savedPath = await ag<string>('save_super_resolution', { path: paths[0] });
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
      await ag<string[]>('batch_super_resolution', { paths, scale });
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

  const chooseScale = (nextScale: number) => {
    if (isBusy) return;
    setScale(nextScale);
    setPreview(null);
    setError(null);
    setProgress(null);
  };

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
          {preview ? (
            <div className="relative flex h-full min-h-[320px] items-center justify-center overflow-hidden rounded-lg">
              <div className="relative h-full w-full" style={{ transform: `scale(${zoom})` }}>
                <img
                  src={preview.result}
                  alt={agT('superResolutionResult').replace('{scale}', String(scale))}
                  className="absolute inset-0 h-full w-full object-contain"
                />
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
                {agT('superResolutionResult').replace('{scale}', String(scale))}
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
              <span>{progress || agT('superResolutionChooseScale')}</span>
              {modelStatus && <span className="text-xs">{modelStatus}</span>}
              {isBusy && (
                <div className="h-1.5 w-64 overflow-hidden rounded-full bg-surface" aria-label="Super resolution progress">
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
                : agT('superResolutionScale').replace('{scale}', String(scale))}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex items-center gap-1 rounded-md border border-border-color p-0.5">
              <span className="px-1.5 text-xs text-text-secondary">{agT('superResolutionScaleLabel')}</span>
              {[2, 4].map((option) => (
                <button
                  className={`rounded px-2 py-1 text-xs ${
                    scale === option ? 'bg-accent text-button-text' : 'text-text-secondary hover:bg-surface'
                  }`}
                  disabled={isBusy}
                  key={option}
                  onClick={() => chooseScale(option)}
                  type="button"
                >
                  {option}×
                </button>
              ))}
            </div>
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
                disabled={isBusy}
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
