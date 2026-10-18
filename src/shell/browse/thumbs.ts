// Thumbnails the page holds, as object URLs the grid and the filmstrip draw:
// within a budget in bytes, the least recently wanted going first. The app
// keeps its own copies in memory and on disk (src-tauri/src/mods/browse/
// cache.rs); this is only what is on, or near, the screen.

/** Thumbnails held by the page, in bytes of image: some 1,900 at THUMB_PX. */
export const THUMBS_BUDGET_BYTES = 48 * 1024 * 1024;

export interface ThumbUrls {
  make(blob: Blob): string;
  revoke(url: string): void;
}

const objectUrls: ThumbUrls = {
  make: (blob) => URL.createObjectURL(blob),
  revoke: (url) => URL.revokeObjectURL(url),
};

export class ThumbCache {
  /** By path, the least recently wanted first: a Map keeps insertion order. */
  private held = new Map<string, { url: string; bytes: number }>();
  private bytes = 0;

  constructor(
    private readonly budgetBytes = THUMBS_BUDGET_BYTES,
    private readonly urls: ThumbUrls = objectUrls,
  ) {}

  url(path: string): string | undefined {
    return this.held.get(path)?.url;
  }

  has(path: string): boolean {
    return this.held.has(path);
  }

  /** Keep `blob` as `path`'s thumbnail, letting the least recently wanted go until it fits. */
  put(path: string, blob: Blob): void {
    this.remove(path);
    while (this.bytes + blob.size > this.budgetBytes && this.held.size > 0) {
      this.remove(this.held.keys().next().value!);
    }
    this.held.set(path, { url: this.urls.make(blob), bytes: blob.size });
    this.bytes += blob.size;
  }

  /** These were wanted just now: they go last. */
  touch(paths: readonly string[]): void {
    for (const p of paths) {
      const entry = this.held.get(p);
      if (!entry) continue;
      this.held.delete(p);
      this.held.set(p, entry);
    }
  }

  get size(): { count: number; bytes: number } {
    return { count: this.held.size, bytes: this.bytes };
  }

  private remove(path: string): void {
    const entry = this.held.get(path);
    if (!entry) return;
    this.held.delete(path);
    this.bytes -= entry.bytes;
    this.urls.revoke(entry.url);
  }
}
