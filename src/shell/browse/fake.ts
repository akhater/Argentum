// A test double of the app's browsing: the same BrowseClient as the real one,
// for the shell's tests and for a plain browser. Its disk is made up: two
// drives, dated folders, a folder of 1,200 photos and one of a card. It lists
// in parts and latest-wins as the app does, makes thumbnails in the order
// wanted, a few at a time, and fails one photo in a hundred as a broken file
// would. It remembers favourites and recent folders until the page closes.

import { THUMB_PX, type BrowseError, type BrowseEvent, type Folder, type Photo } from './browse.generated';
import type { BrowseClient } from './client';

const PICTURES = 'C:\\Users\\you\\Pictures';
const CARD = 'D:\\DCIM\\100CANON';

/** The made-up disk: each folder's photos by name. Folders not listed hold none. */
const PHOTO_NAMES: Record<string, string[]> = {
  [`${PICTURES}\\2026\\2026-10-09`]: [
    'IMG_0412.CR3',
    'IMG_0415.CR3',
    'IMG_0421.CR3',
    'DSC_1180.NEF',
    'DSC_1184.NEF',
    '_R6A2207.CR3',
    '_R6A2213.CR3',
    'P1040562.RW2',
  ],
  [`${PICTURES}\\2026\\2026-09-26`]: numbered('IMG_', 2200, 36, '.CR3'),
  [`${PICTURES}\\Big shoot`]: numbered('_R6A', 1, 1200, '.CR3'),
  [`${PICTURES}\\Scans`]: ['scan_01.jpg', 'scan_02.jpg', 'scan_03.tif', 'poster.png'],
  [CARD]: numbered('IMG_', 9001, 40, '.CR3'),
  [`${PICTURES}\\_inbox`]: numbered('IMG_', 1, 12, '.CR3'),
};

const FOLDERS = ['C:\\Users', 'C:\\Users\\you', PICTURES, `${PICTURES}\\2026`, ...Object.keys(PHOTO_NAMES), 'D:\\DCIM'];

function numbered(prefix: string, from: number, count: number, ext: string): string[] {
  return Array.from({ length: count }, (_, i) => `${prefix}${String(from + i).padStart(4, '0')}${ext}`);
}

const nameOf = (path: string) => path.split('\\').pop() ?? path;
const parentOf = (path: string) => path.slice(0, path.lastIndexOf('\\')) || path;

function hash(text: string): number {
  let h = 0;
  for (const c of text) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h;
}

function childrenOf(path: string): Folder[] {
  const dir = path.endsWith('\\') ? path.slice(0, -1) : path;
  const kids = FOLDERS.filter((f) => parentOf(f) === dir);
  // In no order, as a disk gives them: the interface sorts.
  return kids.map((f) => ({ name: nameOf(f), path: f, hasFolders: FOLDERS.some((g) => parentOf(g) === f) }));
}

const notFound = (path: string): BrowseError => ({ kind: 'notFound', message: `${path}: not found` });

function photosOf(folder: string): Photo[] {
  const names = PHOTO_NAMES[folder] ?? [];
  // A shoot an evening long: a photo every 20 seconds, the names in order.
  const start = Date.UTC(2026, 9, 9, 17) - (hash(folder) % 30) * 86_400_000;
  return names.map((name, i) => ({
    name,
    path: `${folder}\\${name}`,
    bytes: 24_000_000 + (hash(name) % 9_000_000),
    modifiedMs: start + i * 20_000 + (hash(name) % 7_000),
    edited: hash(name) % 5 === 0,
  }));
}

/** A broken file: one in a hundred has no thumbnail. */
const isBroken = (path: string) => hash(path) % 100 === 7;

export interface FakeBrowseOptions {
  /** Milliseconds a thumbnail takes to make. 0 in tests. */
  thumbMs?: number;
  /** Thumbnails made at once. */
  workers?: number;
  /** Photos in one listed part, as the app's PART_PHOTOS. */
  part?: number;
  /** Makes a thumbnail's image. Without one, a few bytes stand in for it. */
  paint?: (path: string) => Promise<Blob>;
}

export interface FakeBrowse extends BrowseClient {
  /** Every thumbnail asked to be made, in the order they were begun. */
  readonly made: string[];
}

export function createFakeBrowse(options: FakeBrowseOptions = {}): FakeBrowse {
  const thumbMs = options.thumbMs ?? 0;
  const workers = options.workers ?? 4;
  const part = options.part ?? 256;
  const listeners = new Set<(event: BrowseEvent) => void>();
  const thumbs = new Map<string, Blob>();
  const made: string[] = [];
  let favourites = [`${PICTURES}\\2026\\2026-10-09`];
  let recent = [`${PICTURES}\\2026\\2026-10-09`, `${PICTURES}\\Big shoot`, CARD];
  let latestListing = 0;
  let waiting: string[] = [];
  const working = new Set<string>();

  const emit = (event: BrowseEvent) => listeners.forEach((l) => l(event));
  const later = (ms = 0) => new Promise<void>((resolve) => setTimeout(resolve, ms));
  const paint = options.paint ?? (async (path: string) => new Blob([path], { type: 'image/jpeg' }));

  async function work() {
    for (let path = waiting.shift(); path !== undefined; path = waiting.shift()) {
      working.add(path);
      made.push(path);
      await later(thumbMs);
      if (isBroken(path)) {
        emit({
          event: 'noThumb',
          path,
          error: { kind: 'noPreview', message: `${nameOf(path)}: no picture inside it` },
        });
      } else {
        thumbs.set(path, await paint(path));
        emit({ event: 'thumb', path });
      }
      working.delete(path);
    }
  }
  let running = 0;
  const startWorkers = () => {
    while (running < workers && waiting.length > 0) {
      running++;
      void work().finally(() => running--);
    }
  };

  return {
    made,

    async places() {
      await later();
      return {
        drives: [
          { name: 'C:', path: 'C:\\', hasFolders: true },
          { name: 'D:', path: 'D:\\', hasFolders: true },
        ],
        standard: [{ name: 'Pictures', path: PICTURES, hasFolders: true }],
        favourites: [...favourites],
        recent: [...recent],
      };
    },

    async folders(path) {
      await later();
      if (!/^[A-Z]:\\$/.test(path) && !FOLDERS.includes(path)) throw notFound(path);
      return childrenOf(path);
    },

    async list(path) {
      await later();
      if (!/^[A-Z]:\\$/.test(path) && !FOLDERS.includes(path)) throw notFound(path);
      const listing = ++latestListing;
      const photos = photosOf(path);
      void (async () => {
        for (let at = 0; ; at += part) {
          await later();
          if (listing !== latestListing) return;
          const done = at + part >= photos.length;
          emit({ event: 'listed', listing, photos: photos.slice(at, at + part), done });
          if (done) return;
        }
      })();
      return listing;
    },

    async want(paths) {
      waiting = [...new Set(paths)].filter((p) => !working.has(p));
      startWorkers();
    },

    async thumb(path) {
      return thumbs.get(path) ?? null;
    },

    async setFavourite(path, on) {
      favourites = favourites.filter((f) => f !== path);
      if (on) favourites.push(path);
      return [...favourites];
    },

    async addRecent(path) {
      recent = [path, ...recent.filter((f) => f !== path)].slice(0, 12);
      return [...recent];
    },

    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/** Different pictures the fake's thumbnails are drawn from. */
const LOOKS = 24;
const painted = new Map<number, Promise<Blob>>();

/**
 * A thumbnail for a plain browser, as a camera's preview might look: one of
 * `LOOKS` pictures, each painted once. The page then pays for a thumbnail
 * what it pays inside the app, where they are made elsewhere.
 */
export function paintThumb(path: string): Promise<Blob> {
  const look = hash(path) % LOOKS;
  let blob = painted.get(look);
  // 137 degrees apart, so neighbours differ.
  if (!blob) painted.set(look, (blob = paintLook(look * 137)));
  return blob;
}

function paintLook(h: number): Promise<Blob> {
  const portrait = h % 5 === 0;
  const [w, hgt] = portrait ? [Math.round((THUMB_PX * 2) / 3), THUMB_PX] : [THUMB_PX, Math.round((THUMB_PX * 2) / 3)];
  const canvas = new OffscreenCanvas(w, hgt);
  const ctx = canvas.getContext('2d')!;
  const hue = h % 360;
  const sky = ctx.createLinearGradient(0, 0, 0, hgt);
  sky.addColorStop(0, `hsl(${hue} 45% 62%)`);
  sky.addColorStop(1, `hsl(${(hue + 40) % 360} 35% 28%)`);
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, w, hgt);
  ctx.fillStyle = `hsl(${(hue + 180) % 360} 30% 22%)`;
  ctx.beginPath();
  ctx.moveTo(0, hgt);
  for (let x = 0; x <= w; x += w / 8) ctx.lineTo(x, hgt * (0.55 + 0.2 * Math.sin((x / w) * 6 + (h % 7))));
  ctx.lineTo(w, hgt);
  ctx.fill();
  ctx.fillStyle = `hsl(${(hue + 60) % 360} 80% 85% / 0.9)`;
  ctx.beginPath();
  ctx.arc(w * (0.2 + (h % 60) / 100), hgt * 0.3, hgt * 0.08, 0, Math.PI * 2);
  ctx.fill();
  return canvas.convertToBlob({ type: 'image/jpeg', quality: 0.8 });
}
