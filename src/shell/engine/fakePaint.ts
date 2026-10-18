// Pixels for the contract fake in a browser: one worker, one job at a time. A
// job that arrives while another is painting waits; a newer one replaces it,
// and the replaced one answers null, so only the latest is painted.

import type { FakePainter, PaintJob } from './fake';
import type { PaintMessage, PaintReply } from './fakePaint.worker';

const num = (v: unknown, fallback: number) => (typeof v === 'number' ? v : fallback);

function messageFor(job: PaintJob): PaintMessage {
  const tools = job.edit.tools;
  const wb = tools.ag_exposure_wb?.settings ?? {};
  const sharpen = tools.rt_capture_sharpen?.settings ?? {};
  const gain = Math.pow(2, num(wb.exposure, 0));
  const radius = sharpen.autoRadius === false ? num(sharpen.radius, 0.75) / 0.75 : 1;
  let seed = 0;
  for (const c of job.path) seed = (seed * 33 + c.charCodeAt(0)) % 9973;
  return {
    seed,
    photo: { width: job.width, height: job.height },
    view: job.view,
    gains: [gain * num(wb.red, 1), gain * num(wb.green, 1), gain * num(wb.blue, 1)],
    recover: tools.dt_highlights?.settings.recover !== false,
    sharpen: sharpen.capture === false ? 0 : (num(sharpen.captureAmount, 100) / 100) * radius,
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
