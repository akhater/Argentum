/**
 * Tells the user when GPU setup did not finish and the app moved to OpenGL.
 * The decision is `mods/gpu_setup_mark.rs`; it arrives with `frontend_ready`,
 * because setup runs before there is a window to say anything in. Ours.
 */

import i18n from 'i18next';
import { toast } from 'react-toastify';
import { agText } from './locales';

export interface GpuFallback {
  kind: 'thisLaunch' | 'saved';
  backend: string;
}

/** The backend under the name Settings gives it. */
const backendName = (backend: string): string =>
  i18n.t(`settings.processing.backends.${backend}` as never, { defaultValue: backend }) as string;

export function gpuFallbackMessage({ kind, backend }: GpuFallback): string {
  const text = agText(kind === 'saved' ? 'gpuFallbackSaved' : 'gpuFallbackThisLaunch');
  return text.replace('{backend}', backendName(backend));
}

/** It stays until clicked: it explains a change the user did not make. */
export function showGpuFallback(fallback: GpuFallback) {
  toast.warn(gpuFallbackMessage(fallback), { autoClose: false });
}
