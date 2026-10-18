// Pixels for the contract fake in a browser: one worker, one job at a time. A
// job that arrives while another is painting waits; a newer one replaces it,
// and the replaced one answers null, so only the latest is painted.

import type { EditDocument } from '../contract';
import { valueOf } from '../edit';
import { FAKE_TOOLS, type FakePainter, type PaintJob } from './fake';
import type { PaintMessage, PaintReply } from './fakePaint.worker';

const specOf = (toolId: string, key: string) =>
  FAKE_TOOLS.find((t) => t.id === toolId)?.settings.find((s) => s.key === key);

/** A number setting of the edit, or its descriptor's default. */
function amount(edit: EditDocument, toolId: string, key: string): number {
  const spec = specOf(toolId, key);
  return spec ? Number(valueOf(edit, toolId, spec)) : 0;
}

/** Whether a bool setting of the edit is on, or its descriptor's default. */
function on(edit: EditDocument, toolId: string, key: string): boolean {
  const spec = specOf(toolId, key);
  return spec ? valueOf(edit, toolId, spec) === true : false;
}

function messageFor(job: PaintJob): PaintMessage {
  const { edit } = job;
  const gain = Math.pow(2, amount(edit, 'ag_exposure_wb', 'exposure'));
  // Sharpening as a share of its full amount, stronger for a radius above the default.
  const radius = on(edit, 'rt_capture_sharpen', 'autoRadius')
    ? 1
    : amount(edit, 'rt_capture_sharpen', 'radius') / Number(specOf('rt_capture_sharpen', 'radius')?.default ?? 1);
  const share =
    amount(edit, 'rt_capture_sharpen', 'captureAmount') / (specOf('rt_capture_sharpen', 'captureAmount')?.max ?? 1);
  let seed = 0;
  for (const c of job.path) seed = (seed * 33 + c.charCodeAt(0)) % 9973;
  return {
    seed,
    photo: { width: job.width, height: job.height },
    view: job.view,
    gains: [
      gain * amount(edit, 'ag_exposure_wb', 'red'),
      gain * amount(edit, 'ag_exposure_wb', 'green'),
      gain * amount(edit, 'ag_exposure_wb', 'blue'),
    ],
    recover: on(edit, 'dt_highlights', 'recover'),
    sharpen: on(edit, 'rt_capture_sharpen', 'capture') ? share * radius : 0,
  };
}

type Result = Awaited<ReturnType<FakePainter>>;

export function createWorkerPainter(): FakePainter {
  const worker = new Worker(new URL('./fakePaint.worker.ts', import.meta.url), { type: 'module' });
  let busy: ((result: Result) => void) | null = null;
  let waiting: { job: PaintJob; resolve: (r: Result) => void } | null = null;

  const start = (job: PaintJob, resolve: (r: Result) => void) => {
    busy = resolve;
    worker.postMessage(messageFor(job));
  };

  worker.onmessage = (event: MessageEvent<PaintReply>) => {
    const { width, height, data, bins } = event.data;
    const done = busy;
    busy = null;
    done?.({ pixels: { width, height, data }, bins });
    if (waiting) {
      const next = waiting;
      waiting = null;
      start(next.job, next.resolve);
    }
  };

  return (job) =>
    new Promise((resolve) => {
      if (!busy) return start(job, resolve);
      waiting?.resolve(null);
      waiting = { job, resolve };
    });
}
