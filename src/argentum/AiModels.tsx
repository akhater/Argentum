/**
 * Settings > AI Models: what has been downloaded, and deleting it. Ours.
 *
 * Every AI feature fetches its model the first time it is used, and a few uses
 * in the models folder holds well over a gigabyte nobody chose to download.
 * This lists what the app knows about (`mods/model_catalog.rs` — adding a model
 * is one entry there and nothing here) with what is on disk, and deletes an
 * entry's files. There is no download button: the feature that needs a model
 * still fetches it itself, next time it is used.
 *
 * Deleting can be refused while Argentum runs — Windows will not delete a file
 * that is open, and the graphics card runtime must stay put once it has been
 * picked for this run. The row then says it goes at the next start, which is
 * what actually happens, rather than pretending it has gone.
 */

import { useCallback, useEffect, useState } from 'react';
import { Trash2 } from 'lucide-react';
import { ag } from './ag';
import { useAgTranslation } from './locales';

type OnDisk = 'downloaded' | 'partial' | 'missing';

interface ModelStatus {
  id: string;
  label: string;
  purpose: string;
  onDisk: OnDisk;
  bytes: number;
  downloadBytes: number | null;
  removingAtNextStart: boolean;
}

interface Listing {
  models: ModelStatus[];
  totalBytes: number;
  otherBytes: number;
}

type Outcome = 'deleted' | 'atNextStart' | 'notThere';

/**
 * Sizes as Windows Explorer shows them, in units of 1024, so the number here
 * matches the one in the folder's properties.
 */
function formatSize(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(1)} GB`;
  }
  if (mb >= 10 || bytes === 0) {
    return `${Math.round(mb)} MB`;
  }
  if (mb >= 0.1) {
    return `${mb.toFixed(1)} MB`;
  }
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export default function AiModels() {
  const t = useAgTranslation();
  const [listing, setListing] = useState<Listing | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<{ id: string; text: string; warn: boolean } | null>(null);

  const refresh = useCallback(async () => {
    try {
      setListing(await ag<Listing>('list_ai_models'));
      setFailed(null);
    } catch (e) {
      setFailed(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const remove = async (id: string) => {
    setNote(null);
    setBusy(id);
    try {
      const outcome = await ag<Outcome>('delete_ai_model', { id });
      if (outcome === 'atNextStart') {
        setNote({ id, text: t('modelsAtNextStart'), warn: true });
      }
    } catch (e) {
      setNote({ id, text: String(e), warn: false });
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const status = (m: ModelStatus) => {
    if (m.removingAtNextStart) {
      return (
        <span className="text-yellow-500">
          {t('modelsWaiting')} · {formatSize(m.bytes)}
        </span>
      );
    }
    if (m.onDisk === 'missing') {
      return m.downloadBytes
        ? `${t('modelsNotDownloaded')} · ${t('modelsDownloadSize').replace('{size}', formatSize(m.downloadBytes))}`
        : t('modelsNotDownloaded');
    }
    const state = m.onDisk === 'partial' ? t('modelsPartial') : t('modelsDownloaded');
    return (
      <>
        <span className="text-accent">{state}</span> · {formatSize(m.bytes)}
      </>
    );
  };

  return (
    <div className="p-6 bg-surface rounded-xl shadow-md max-w-[78ch]">
      <h3 className="text-lg font-semibold text-accent mb-2">{t('modelsTitle')}</h3>
      <p className="text-text-secondary leading-relaxed">{t('modelsDesc')}</p>
      <p className="mt-2 text-sm text-text-secondary leading-relaxed">{t('modelsMaskSet')}</p>

      {failed && <p className="mt-4 text-sm text-red-400">{failed}</p>}

      {listing && (
        <>
          <p className="mt-4 mb-4 font-medium text-text-primary">
            {t('modelsTotal').replace('{size}', formatSize(listing.totalBytes))}
          </p>

          <div className="space-y-2">
            {listing.models.map((m) => (
              <div key={m.id} className="p-3 bg-bg-primary rounded-lg">
                <div className="flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="font-medium text-text-primary">{m.label}</p>
                    <p className="text-xs text-text-secondary leading-relaxed">{m.purpose}</p>
                    <p className="mt-1 text-xs text-text-secondary">{status(m)}</p>
                  </div>
                  {m.onDisk !== 'missing' && !m.removingAtNextStart && (
                    <button
                      onClick={() => remove(m.id)}
                      disabled={busy !== null}
                      className="p-2 text-text-secondary hover:text-red-400 hover:bg-bg-secondary rounded-md transition-colors shrink-0 disabled:opacity-40"
                      data-tooltip={t('modelsDelete')}
                      aria-label={`${t('modelsDelete')}: ${m.label}`}
                    >
                      <Trash2 size={16} />
                    </button>
                  )}
                </div>
                {note?.id === m.id && (
                  <p className={`mt-2 text-xs ${note.warn ? 'text-yellow-500' : 'text-red-400'}`}>
                    {note.text}
                  </p>
                )}
              </div>
            ))}
          </div>

          {listing.otherBytes > 0 && (
            <p className="mt-4 text-xs text-text-secondary leading-relaxed">
              {t('modelsOther').replace('{size}', formatSize(listing.otherBytes))}
            </p>
          )}
        </>
      )}
    </div>
  );
}
