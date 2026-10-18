// Browse against the browse fake and the app's own fixtures: the calls the
// real client makes (browse/wire.fixtures.json, also read by the Rust side's
// test in src-tauri/src/mods/browse/mod.rs), listing in parts and latest
// wins, thumbnails the most wanted first, what is remembered, and the way
// into Edit and back.

import { describe, expect, it } from 'vitest';

import type { BrowseClient } from '../browse/client';
import fixtures from '../browse/browse.fixtures.json';
import type { BrowseEvent, ListingId, Photo } from '../browse/browse.generated';
import { sortPhotos } from '../browse/browseSlice';
import { createFakeBrowse, type FakeBrowse } from '../browse/fake';
import { BROWSE_EVENT, createTauriBrowse } from '../browse/tauri';
import { ThumbCache } from '../browse/thumbs';
import wire from '../browse/wire.fixtures.json';
import { createFakeEngine } from '../engine/fake';
import type { Transport } from '../engine/tauri';
import { ancestorsOf, folderName, folderOf, samePath } from '../paths';
import { createShellStore } from '../store';
import { settle } from './helpers';

const PICTURES = 'C:\\Users\\you\\Pictures';
const BIG = `${PICTURES}\\Big shoot`;
const DAY = `${PICTURES}\\2026\\2026-10-09`;

/** Let the fake work until `done` holds. */
async function until(done: () => boolean) {
  for (let i = 0; i < 500 && !done(); i++) await settle();
  await settle();
}

async function browsing(browse: BrowseClient = createFakeBrowse()) {
  const engine = createFakeEngine({ autoFrames: false });
  const store = createShellStore(engine, { browse, decode: async () => null });
  await store.getState().init();
  await store.getState().initBrowse();
  return { engine, store, s: store.getState };
}

describe('the real browse client', () => {
  it('makes the calls the app reads, with the arguments it reads them with', async () => {
    const calls: [string, Record<string, unknown>][] = [];
    const transport: Transport = {
      async ag<T>(name: string, args: Record<string, unknown> = {}) {
        calls.push([name, args]);
        return (name === 'browse_thumb' ? new Uint8Array([0xff, 0xd8]).buffer : null) as T;
      },
      listen: async () => () => undefined,
      sharedBuffers: () => () => undefined,
    };
    const browse = createTauriBrowse(transport);
    const c = wire.calls;
    await browse.places();
    await browse.folders(c.browse_folders.path);
    await browse.list(c.browse_list.path);
    await browse.want(c.browse_want.paths);
    const thumb = await browse.thumb(c.browse_thumb.path);
    await browse.setFavourite(c.browse_favourite.path, c.browse_favourite.on);
    await browse.addRecent(c.browse_add_recent.path);
    expect(Object.fromEntries(calls)).toEqual(wire.calls);
    expect(thumb?.type).toBe('image/jpeg');
    expect(thumb?.size).toBe(2);
  });

  it('hears the app’s events as the contract writes them', async () => {
    let handler: ((payload: unknown) => void) | null = null;
    const transport: Transport = {
      ag: async () => null as never,
      listen: async (event, h) => {
        expect(event).toBe(BROWSE_EVENT);
        handler = h as (payload: unknown) => void;
        return () => undefined;
      },
      sharedBuffers: () => () => undefined,
    };
    const seen: BrowseEvent[] = [];
    createTauriBrowse(transport).subscribe((e) => seen.push(e));
    await settle();
    for (const e of fixtures.events) handler!(e);
    expect(seen).toEqual(fixtures.events);
  });
});

describe('browsing', () => {
  it('starts at the folders remembered, with no folder and no photo open', async () => {
    const { s } = await browsing();
    expect(s().mode).toBe('browse');
    expect(s().folder).toBeNull();
    expect(s().path).toBeNull();
    expect(s().places?.favourites).toEqual([DAY]);
    expect(s().places?.recent[0]).toBe(DAY);
  });

  it('lists a big folder in parts, sorted as people count, the filmstrip the same', async () => {
    const { s } = await browsing();
    const asked = s().browseFolder(BIG);
    expect(s().listed).toBe(false);
    await asked;
    await settle();
    expect(s().listed).toBe(true);
    expect(s().folderPhotos).toHaveLength(1200);
    expect(s().folderPhotos[0].name).toBe('_R6A0001.CR3');
    expect(s().photos).toEqual(s().folderPhotos.map((p) => p.path));
    // Remembered as the latest folder opened.
    expect(s().places?.recent[0]).toBe(BIG);
  });

  it('shows only the folder asked for last', async () => {
    const { s } = await browsing();
    void s().browseFolder(BIG);
    await s().browseFolder(DAY);
    await settle();
    expect(s().folder).toBe(DAY);
    expect(s().folderPhotos).toHaveLength(8);
    expect(s().folderPhotos.every((p) => p.path.startsWith(DAY))).toBe(true);
  });

  it('keeps photos the app sends before it answers with the listing’s id', async () => {
    const listeners = new Set<(e: BrowseEvent) => void>();
    const photo = (name: string): Photo => ({
      name,
      path: `C:\\x\\${name}`,
      bytes: 1,
      modifiedMs: 0,
      edited: false,
    });
    const fake = createFakeBrowse();
    const early: BrowseClient = {
      ...fake,
      subscribe: (l) => (listeners.add(l), () => listeners.delete(l)),
      async list() {
        const listing: ListingId = 7;
        // An older listing's last part, then this one's, all before the answer.
        listeners.forEach((l) => l({ event: 'listed', listing: 6, photos: [photo('old.CR3')], done: true }));
        listeners.forEach((l) => l({ event: 'listed', listing, photos: [photo('b.CR3'), photo('a.CR3')], done: true }));
        return listing;
      },
    };
    const { s } = await browsing(early);
    await s().browseFolder('C:\\x');
    expect(s().folderPhotos.map((p) => p.name)).toEqual(['a.CR3', 'b.CR3']);
    expect(s().listed).toBe(true);
  });

  it('says why a folder cannot be shown', async () => {
    const { s } = await browsing();
    await s().browseFolder('C:\\gone');
    expect(s().browseError?.kind).toBe('notFound');
    expect(s().listed).toBe(true);
    expect(s().folderPhotos).toEqual([]);
  });

  it('sorts by date, oldest first, names breaking ties', () => {
    const p = (name: string, modifiedMs: number): Photo => ({ name, path: name, bytes: 1, modifiedMs, edited: false });
    const photos = [p('IMG_10', 5), p('IMG_2', 5), p('IMG_1', 9)];
    expect(sortPhotos(photos, 'name').map((x) => x.name)).toEqual(['IMG_1', 'IMG_2', 'IMG_10']);
    expect(sortPhotos(photos, 'date').map((x) => x.name)).toEqual(['IMG_2', 'IMG_10', 'IMG_1']);
  });

  it('opens the tree down to a folder opened from the recent ones', async () => {
    const { s } = await browsing();
    await s().browseFolder(DAY);
    await settle();
    expect(s().expanded[PICTURES]).toBe(true);
    expect(s().expanded[`${PICTURES}\\2026`]).toBe(true);
    expect(s().subfolders[`${PICTURES}\\2026`]?.map((f) => f.name)).toEqual(['2026-09-26', '2026-10-09']);
    // Under the deepest place that holds it, not every drive.
    expect(s().expanded['C:\\']).toBeUndefined();
    await s().toggleExpanded(PICTURES);
    expect(s().expanded[PICTURES]).toBe(false);
  });

  it('pins and unpins a favourite', async () => {
    const { s } = await browsing();
    await s().setFavourite(BIG, true);
    expect(s().places?.favourites).toEqual([DAY, BIG]);
    await s().setFavourite(DAY, false);
    expect(s().places?.favourites).toEqual([BIG]);
  });
});

describe('thumbnails', () => {
  const slow = () => createFakeBrowse({ thumbMs: 5, workers: 1 });

  it('are made the most wanted first, each new list replacing the last', async () => {
    const browse: FakeBrowse = slow();
    const { s } = await browsing(browse);
    await s().browseFolder(BIG);
    await settle();
    const paths = s().folderPhotos.map((p) => p.path);
    s().wantThumbs(paths.slice(0, 4));
    // The user scrolls before the first is done.
    s().wantThumbs(paths.slice(600, 602));
    await new Promise((r) => setTimeout(r, 60));
    expect(browse.made).toEqual([paths[0], paths[600], paths[601]]);
    expect(s().thumbUrl(paths[600])).toBeDefined();
    expect(s().thumbUrl(paths[1])).toBeUndefined();
  });

  it('are not asked for again once held, nor when none can be made', async () => {
    const browse: FakeBrowse = createFakeBrowse();
    const { s } = await browsing(browse);
    await s().browseFolder(BIG);
    await settle();
    const paths = s().folderPhotos.map((p) => p.path);
    s().wantThumbs(paths.slice(0, 200));
    await until(() => paths.slice(0, 200).every((p) => s().thumbUrl(p) !== undefined || s().thumbFailed(p)));
    const broken = paths.slice(0, 200).filter((p) => s().thumbFailed(p));
    expect(broken.length).toBeGreaterThan(0);
    expect(broken.every((p) => s().thumbUrl(p) === undefined)).toBe(true);
    const made = browse.made.length;
    s().wantThumbs(paths.slice(0, 210));
    await until(() => browse.made.length >= made + 10);
    expect(browse.made.slice(made)).toEqual(paths.slice(200, 210));
  });

  it('are held within a budget in bytes, the least recently wanted let go', () => {
    const revoked: string[] = [];
    let n = 0;
    const cache = new ThumbCache(300, { make: () => `blob:${++n}`, revoke: (u) => revoked.push(u) });
    const blob = () => new Blob([new Uint8Array(100)]);
    cache.put('a', blob());
    cache.put('b', blob());
    cache.put('c', blob());
    cache.touch(['a']);
    cache.put('d', blob());
    expect(cache.size).toEqual({ count: 3, bytes: 300 });
    expect(cache.has('b')).toBe(false);
    expect(revoked).toEqual(['blob:2']);
    expect(cache.url('a')).toBe('blob:1');
  });
});

describe('Browse and Edit', () => {
  it('opens a photo in Edit with its folder as the filmstrip, and goes back', async () => {
    const { engine, s } = await browsing();
    await s().browseFolder(DAY);
    await settle();
    const photo = s().folderPhotos[2].path;
    await s().edit(photo);
    expect(s().mode).toBe('edit');
    expect(s().path).toBe(photo);
    expect(engine.calls.filter((c) => c.method === 'open')).toHaveLength(1);
    expect(s().photos).toHaveLength(8);
    s().setMode('browse');
    expect(s().folder).toBe(DAY);
    // Edit again shows the photo picked in the grid.
    s().select(s().folderPhotos[5].path);
    s().setMode('edit');
    await settle();
    expect(s().path).toBe(s().folderPhotos[5].path);
  });

  it('opens a photo from disk with its folder browsed', async () => {
    const { s } = await browsing();
    await s().openFolder(`${DAY}\\IMG_0415.CR3`);
    await settle();
    expect(s().mode).toBe('edit');
    expect(s().folder).toBe(DAY);
    expect(s().photos).toContain(`${DAY}\\IMG_0415.CR3`);
  });
});

describe('paths', () => {
  it('finds a file’s folder and the folders above it', () => {
    expect(folderOf('C:\\a\\b.CR3')).toBe('C:\\a');
    expect(folderOf('C:\\b.CR3')).toBe('C:\\');
    expect(folderOf('/home/x.cr3')).toBe('/home');
    expect(folderOf('/x.cr3')).toBe('/');
    expect(ancestorsOf('C:\\a\\b\\c')).toEqual(['C:\\', 'C:\\a', 'C:\\a\\b']);
    expect(ancestorsOf('/a/b')).toEqual(['/', '/a']);
    expect(folderName('C:\\')).toBe('C:');
    expect(folderName('C:\\Photos\\')).toBe('Photos');
    expect(folderName('/')).toBe('/');
  });

  it('tells two spellings of one Windows path apart from two paths', () => {
    expect(samePath('C:/Photos/IMG_1.CR3', 'c:\\photos\\img_1.cr3')).toBe(true);
    expect(samePath('/home/a.CR3', '/home/A.CR3')).toBe(false);
    expect(samePath('C:\\a', 'C:\\b')).toBe(false);
  });
});
