/**
 * Settings > General > AI Models: what has been downloaded, downloading what
 * has not, deleting it, and clearing what no model uses. Ours.
 *
 * Under the Generative AI card, which RapidRAW 1.6.5 moved to General: that card
 * chooses how the AI features run, this one shows what they have downloaded.
 *
 * Every AI feature fetches its model the first time it is used, and a few uses
 * in the models folder holds well over a gigabyte nobody chose to download.
 * This lists what the app knows about (`mods/model_catalog.rs` — adding a model
 * is one entry there and nothing here) with what is on disk. A row that is not
 * all there has a Download button with its size, and shows progress while it
 * downloads (`mods/model_download.rs`, the same files the feature itself would
 * fetch, checked against the same checksums). The layout of a row, its button
 * and its progress line follow Shoebox's Settings > AI models
 * (src/components/ModelsTab.tsx @ 3c82753).
 *
 * Deleting can be refused while Argentum runs — Windows will not delete a file
 * that is open, and the graphics card runtime must stay put once it has been
 * picked for this run. The row then says it goes at the next start, which is
 * what actually happens, rather than pretending it has gone.
 *
 * The "other files" line is what no model claims: an unfinished download, a
 * feature's temporary file. Clear shows that list with its size first and
 * deletes only what it showed.
 */

import { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { Download, Trash2 } from 'lucide-react';
import { ag } from './ag';
import { useAgTranslation } from './locales';
import Text from '../components/ui/Text';
import { TextColors, TextVariants } from '../types/typography';
import { formatSize } from './formatSize';
import {
  AG_EVENTS,
  type AgEventPayloads,
  type Leftover,
  type Listing,
  type ModelStatus,
  type Progress,
} from './ag.generated';

/** What mods/model_download.rs ends a stopped download with. */
const STOPPED = 'Download stopped.';

type Tone = 'ok' | 'warn' | 'error' | 'muted';

const TONE: Record<Tone, string> = {
  ok: 'text-accent',
  warn: 'text-yellow-500',
  error: 'text-red-400',
  muted: 'text-text-secondary',
};

const BUTTON = 'px-2.5 py-1 rounded-md text-xs font-medium transition-colors shrink-0 disabled:opacity-40';

export default function AiModels() {
  const t = useAgTranslation();
  const [listing, setListing] = useState<Listing | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const [note, setNote] = useState<{ id: string; text: string; tone: Tone } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await ag('list_ai_models');
      setListing(next);
      // A download that began before this page opened, or in the editor.
      setProgress((current) => current ?? next.downloading);
      setFailed(null);
    } catch (e) {
      setFailed(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<AgEventPayloads[typeof AG_EVENTS.AG_MODEL_DOWNLOAD]>(
      AG_EVENTS.AG_MODEL_DOWNLOAD,
      (event) => {
        const p = event.payload;
        if (p.done >= p.total) {
          setProgress(null);
          refresh();
        } else {
          setProgress(p);
        }
      },
    );
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [refresh]);

  const download = async (m: ModelStatus) => {
    setNote(null);
    setBusy(m.id);
    setProgress({ id: m.id, done: 0, total: m.downloadBytes });
    try {
      const fetched = await ag('download_ai_model', { id: m.id });
      setNote({ id: m.id, text: fetched > 0 ? t('modelsFetched') : t('modelsAlreadyHere'), tone: 'ok' });
    } catch (e) {
      const text = String(e);
      setNote({ id: m.id, text, tone: text === STOPPED ? 'muted' : 'error' });
    } finally {
      setBusy(null);
      setProgress(null);
      await refresh();
    }
  };

  const remove = async (id: string) => {
    setNote(null);
    setBusy(id);
    try {
      const outcome = await ag('delete_ai_model', { id });
      if (outcome === 'atNextStart') {
        setNote({ id, text: t('modelsAtNextStart'), tone: 'warn' });
      }
    } catch (e) {
      setNote({ id, text: String(e), tone: 'error' });
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const clear = async (other: Leftover[]) => {
    setConfirmingClear(false);
    setNote(null);
    setBusy('other');
    try {
      const cleared = await ag('clear_ai_model_leftovers', { names: other.map((o) => o.name) });
      const done = t('modelsCleared').replace('{size}', formatSize(cleared.freedBytes));
      setNote(
        cleared.left.length
          ? {
              id: 'other',
              text: `${done} ${t('modelsClearLeft').replace('{names}', cleared.left.join(', '))}`,
              tone: 'warn',
            }
          : { id: 'other', text: done, tone: 'ok' },
      );
    } catch (e) {
      setNote({ id: 'other', text: String(e), tone: 'error' });
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
    // The Download button beside it gives the size still to fetch.
    if (m.onDisk === 'missing') return t('modelsNotDownloaded');
    if (m.onDisk === 'partial') {
      return (
        <>
          <span className="text-yellow-500">{t('modelsPartial')}</span> · {formatSize(m.bytes)}
        </>
      );
    }
    return (
      <>
        <span className="text-accent">{t('modelsDownloaded')}</span> · {formatSize(m.bytes)}
      </>
    );
  };

  const anyDownload = progress !== null || busy !== null;

  return (
    <div className="p-6 bg-surface rounded-xl shadow-md">
      {/* Their card title, so it reads as one of the cards on the page. */}
      <Text variant={TextVariants.title} color={TextColors.accent} className="mb-8">
        {t('modelsTitle')}
      </Text>
      <p className="text-text-secondary leading-relaxed">{t('modelsDesc')}</p>
      <p className="mt-2 text-sm text-text-secondary leading-relaxed">{t('modelsMaskSet')}</p>

      {failed && <p className="mt-4 text-sm text-red-400">{failed}</p>}

      {listing && (
        <>
          <p className="mt-4 mb-4 font-medium text-text-primary">
            {t('modelsTotal').replace('{size}', formatSize(listing.totalBytes))}
          </p>

          <div className="space-y-2">
            {listing.models.map((m) => {
              const downloading = progress?.id === m.id ? progress : null;
              const canDownload = m.onDisk !== 'downloaded' && !m.removingAtNextStart;
              const canDelete = m.onDisk !== 'missing' && !m.removingAtNextStart;
              return (
                <div key={m.id} className="p-3 bg-bg-primary rounded-lg">
                  <div className="flex items-center justify-between gap-3">
                    <div className="min-w-0">
                      <p className="font-medium text-text-primary">{m.label}</p>
                      <p className="text-xs text-text-secondary leading-relaxed">{m.purpose}</p>
                      {!downloading && <p className="mt-1 text-xs text-text-secondary">{status(m)}</p>}
                    </div>
                    <div className="flex items-center gap-1 shrink-0">
                      {downloading ? (
                        <button
                          onClick={() => ag('stop_ai_model_download')}
                          className={`${BUTTON} text-text-primary border border-border-color hover:bg-card-active`}
                        >
                          {t('modelsStop')}
                        </button>
                      ) : (
                        canDownload && (
                          <button
                            onClick={() => download(m)}
                            disabled={anyDownload}
                            className={`${BUTTON} flex items-center gap-1.5 bg-accent text-button-text`}
                          >
                            <Download size={14} />
                            {t('modelsDownload').replace('{size}', formatSize(m.downloadBytes))}
                          </button>
                        )
                      )}
                      {canDelete && !downloading && (
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
                  </div>
                  {downloading && (
                    <div className="mt-2 flex items-center gap-3">
                      <div className="h-1.5 flex-1 rounded-full bg-bg-secondary overflow-hidden">
                        <div
                          className="h-full bg-accent transition-[width]"
                          style={{
                            width: `${downloading.total ? (100 * downloading.done) / downloading.total : 0}%`,
                          }}
                        />
                      </div>
                      <span className="text-xs text-text-secondary tabular-nums shrink-0">
                        {t('modelsProgress')
                          .replace('{done}', formatSize(downloading.done))
                          .replace('{total}', formatSize(downloading.total))}
                      </span>
                    </div>
                  )}
                  {note?.id === m.id && <p className={`mt-2 text-xs ${TONE[note.tone]}`}>{note.text}</p>}
                </div>
              );
            })}
          </div>

          {listing.otherBytes > 0 && (
            <div className="mt-4">
              <div className="flex items-center justify-between gap-3">
                <p className="text-xs text-text-secondary leading-relaxed">
                  {t('modelsOther').replace('{size}', formatSize(listing.otherBytes))}
                </p>
                {!confirmingClear && (
                  <button
                    onClick={() => setConfirmingClear(true)}
                    disabled={anyDownload}
                    className={`${BUTTON} text-text-primary border border-border-color hover:bg-card-active`}
                  >
                    {t('modelsClear')}
                  </button>
                )}
              </div>
              {confirmingClear && (
                <div className="mt-2 p-3 bg-bg-primary rounded-lg">
                  <p className="text-xs text-text-primary leading-relaxed">
                    {t('modelsClearAsk').replace('{size}', formatSize(listing.otherBytes))}
                  </p>
                  <ul className="mt-2 space-y-0.5 text-xs text-text-secondary">
                    {listing.other.map((o) => (
                      <li key={o.name} className="flex justify-between gap-3">
                        <span className="truncate font-mono">{o.name}</span>
                        <span className="tabular-nums shrink-0">{formatSize(o.bytes)}</span>
                      </li>
                    ))}
                  </ul>
                  <div className="mt-3 flex justify-end gap-2">
                    <button
                      onClick={() => setConfirmingClear(false)}
                      className={`${BUTTON} text-text-primary border border-border-color hover:bg-card-active`}
                    >
                      {t('modelsCancel')}
                    </button>
                    <button
                      onClick={() => clear(listing.other)}
                      disabled={anyDownload}
                      className={`${BUTTON} bg-red-500/80 hover:bg-red-500 text-white`}
                    >
                      {t('modelsClearConfirm')}
                    </button>
                  </div>
                </div>
              )}
            </div>
          )}
          {note?.id === 'other' && <p className={`mt-2 text-xs ${TONE[note.tone]}`}>{note.text}</p>}
        </>
      )}
    </div>
  );
}
