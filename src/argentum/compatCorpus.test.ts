// The interface's side of the compatibility corpus (src-tauri/src/mods/compat_corpus.rs):
// every edit a release stored in a sidecar, the photo's and each snapshot's, comes out
// of normalizeLoadedAdjustments with every key and value it was stored with. Filling in
// what is missing is its job; losing or changing what is there loses the edit. A key it
// drops on purpose goes in DROPPED, with the reason and the PR.

import { describe, expect, it } from 'vitest';
import { normalizeLoadedAdjustments, type Adjustments } from '../utils/adjustments';

const sidecars = import.meta.glob('../../src-tauri/fixtures/compat/*/photo.agdata', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

interface Dropped {
  /** A JSON Pointer into the edit, `*` standing for any one key or index. */
  path: string;
  reason: string;
  pr: string;
}

const DROPPED: Dropped[] = [
  {
    path: '/agDngColour',
    reason: 'the camera colour mark of 26.44.1 to 26.45.x; every photo takes that colour now (mods/dng_colour.rs)',
    pr: '#329',
  },
];

interface Sidecar {
  adjustments: Record<string, unknown>;
  agSnapshots?: { name: string; adjustments: Record<string, unknown> }[];
}

const matches = (pattern: string, path: string): boolean => {
  const [p, q] = [pattern.split('/'), path.split('/')];
  return p.length === q.length && p.every((part, i) => part === '*' || part === q[i]);
};

const isObject = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

/** What of `stored` is not in `kept`, one line each. */
const lost = (stored: unknown, kept: unknown, path = ''): string[] => {
  if (DROPPED.some((d) => matches(d.path, path))) return [];
  const shown = path || '/';
  if (kept === undefined) return [`${shown} is gone`];
  if (Array.isArray(stored)) {
    if (!Array.isArray(kept)) return [`${shown} was a list, now ${JSON.stringify(kept)}`];
    if (stored.length !== kept.length) return [`${shown} had ${stored.length} items, now ${kept.length}`];
    return stored.flatMap((item, i) => lost(item, kept[i], `${path}/${i}`));
  }
  if (isObject(stored)) {
    if (!isObject(kept)) return [`${shown} was an object, now ${JSON.stringify(kept)}`];
    return Object.entries(stored).flatMap(([key, value]) =>
      lost(value, kept[key], `${path}/${key.replaceAll('~', '~0').replaceAll('/', '~1')}`),
    );
  }
  return stored === kept ? [] : [`${shown} was ${JSON.stringify(stored)}, now ${JSON.stringify(kept)}`];
};

const releases = Object.entries(sidecars)
  .map(([file, text]) => ({ release: file.split('/').at(-2) ?? file, sidecar: JSON.parse(text) as Sidecar }))
  .sort((a, b) => a.release.localeCompare(b.release, undefined, { numeric: true }));

describe('the compatibility corpus, as the editor loads it', () => {
  it('finds a sidecar for each release', () => {
    expect(releases.map((r) => r.release)).toContain('26.43.3');
  });

  it('names any index in a dropped path', () => {
    expect(matches('/masks/*/adjustments/oldKey', '/masks/3/adjustments/oldKey')).toBe(true);
    expect(matches('/masks/*/adjustments/oldKey', '/masks/3/adjustments')).toBe(false);
    expect(lost({ a: 1, b: { c: [1, 2] } }, { a: 1, b: { c: [1, 3] } })).toEqual(['/b/c/1 was 2, now 3']);
  });

  for (const { release, sidecar } of releases) {
    const edits: [string, Record<string, unknown>][] = [
      ['edit', sidecar.adjustments],
      ...(sidecar.agSnapshots ?? []).map((s, i): [string, Record<string, unknown>] => [
        `snapshot ${i} "${s.name}"`,
        s.adjustments,
      ]),
    ];
    for (const [label, edit] of edits) {
      it(`${release}, ${label}: keeps every key and value it was stored with`, () => {
        const loaded = normalizeLoadedAdjustments(structuredClone(edit) as unknown as Adjustments);
        expect(lost(edit, loaded)).toEqual([]);
      });
    }
  }
});
