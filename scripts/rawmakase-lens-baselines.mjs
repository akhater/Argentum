// RAWmakase's record of the golden photos' built-in lens corrections, regenerated.
//
// RAWmakase records what its decoder reads from each CC0 file of raw.pixls.us
// (tests/corpus/dataset/pixls-decode.json, "decode fingerprints"), the camera's
// own lens correction included. Argentum's readers (src-tauri/engine/src/
// rm_lens_builtin/) are copied from RAWmakase's, so on the golden photos
// (src-tauri/tests/look/samples.tsv) they must read the same tables; the golden
// photos' run checks that (src-tauri/src/mods/look_check/lens_baseline.rs).
// This script writes the table that run reads, from one RAWmakase release:
//
//   * one source version at a time, named on the table's "# Source:" line;
//   * the rows are generated, never edited by hand;
//   * a field RAWmakase records and Argentum's reader does not carry is one
//     entry in LEFT_OUT below, with the reason and what removes it;
//   * check:sources warns when RAWmakase releases past the version named.
//
// Run:  node scripts/rawmakase-lens-baselines.mjs <release tag, or full commit>
// Test: node scripts/test-rawmakase-lens-baselines.mjs
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { releases } from './rawmakase-cameras.mjs';

export const SOURCE_PATH = 'tests/corpus/dataset/pixls-decode.json';
export const TABLE = 'src-tauri/tests/look/rm_lens_baselines.tsv';
export const SAMPLES = 'src-tauri/tests/look/samples.tsv';

/**
 * Fields of RAWmakase's lens record that Argentum's LensCorrection does not
 * carry, so the rows leave them out. Each entry:
 *
 *   { field: 'centre', reason: 'why ours has none', removedWhen: 'what lets it in' }
 *
 * A field no golden photo's record holds any more stops the script: delete it.
 */
export const LEFT_OUT = [
  {
    field: 'gain_map',
    reason: "a DNG's gain maps are applied in Argentum's decode (mods/decode.rs), not carried in the lens correction",
    removedWhen: 'the lens correction carries them',
  },
  {
    field: 'vignetting_measured',
    reason:
      "says the photo is measured (Auto, Whites) with an OpcodeList3 vignette applied; it changes what is measured, not the correction, and Argentum's LensCorrection has no such flag",
    removedWhen: "Argentum's measurement takes the file's OpcodeList3 vignette from the lens correction",
  },
];

/** The golden photos, in samples.tsv's order: [id, sha256]. */
export function goldenPhotos(samplesTsv) {
  return samplesTsv
    .split('\n')
    .filter((line) => /^\d/.test(line))
    .map((line) => {
      const [id, , sha256] = line.split('\t');
      return [id, sha256];
    });
}

/** A value written with its object keys in order, so a row is the same whatever order RAWmakase writes them in. */
function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, sorted(value[key])]),
    );
  }
  return value;
}

/**
 * The image RAWmakase's record is measured in: its decoded image's width and
 * height, and the default crop's left, top, width and height in it, in pixels.
 * Its radii are in units of the decoded image's half diagonal and its centre in
 * fractions of it; Argentum's are in its developed image, the default crop.
 */
function frameOf(metadata) {
  const keys = ['width', 'height', 'crop_left', 'crop_top', 'crop_width', 'crop_height'];
  const values = keys.map((key) => metadata[key]);
  if (!values.every(Number.isInteger)) throw new Error(`a record has no ${keys.join(', ')}`);
  return values.join(' ');
}

/**
 * The table's rows: a golden photo's id, its sha256, the image RAWmakase
 * measures in (`frameOf`), RAWmakase's lens record and its stand-in record
 * (JSON, `null` for none), LEFT_OUT's fields removed. A golden photo RAWmakase
 * has not recorded has no row.
 *
 * RAWmakase keeps a correction in two records: `lens`, what is applied from the
 * file whatever the lens profile, and `lens_stand_in`, what stands in for a
 * profile that has none (Sony's distortion and vignetting). Argentum's one
 * LensCorrection holds both and says which with `always`.
 */
export function lensRows(baselines, photos, leftOut = LEFT_OUT) {
  const bySha = new Map(baselines.files.map((file) => [file.sha256, file]));
  const seen = new Set();
  const kept = (record) => {
    if (!record) return null;
    const out = { ...record };
    for (const { field } of leftOut) {
      if (field in out) {
        seen.add(field);
        delete out[field];
      }
    }
    return sorted(out);
  };
  const rows = [];
  for (const [id, sha256] of photos) {
    const file = bySha.get(sha256);
    if (!file) continue;
    const metadata = file.header?.metadata ?? {};
    rows.push(
      [
        id,
        sha256,
        frameOf(metadata),
        JSON.stringify(kept(metadata.lens)),
        JSON.stringify(kept(metadata.lens_stand_in)),
      ].join('\t'),
    );
  }
  const stale = leftOut.filter(({ field }) => !seen.has(field));
  if (stale.length > 0) {
    throw new Error(
      `LEFT_OUT names fields no golden photo's record holds any more: ${stale.map((s) => s.field).join(', ')} - delete them`,
    );
  }
  return rows;
}

/** The "# Source:" line: which RAWmakase commit, its release, and what the rows hold. */
export function sourceLine({ sha, tag }, rows, leftOut = LEFT_OUT) {
  const corrected = rows.filter((row) => !row.endsWith('\tnull\tnull')).length;
  return (
    `# Source: RAWmakase ${SOURCE_PATH} @ ${sha}${tag ? ` (${tag})` : ''}, ` +
    `${rows.length} golden photos, ${corrected} with a lens correction, ` +
    `left out: ${leftOut.map((l) => l.field).join(', ') || 'nothing'}`
  );
}

/** The commit and release a table names, or null if it names none. */
export function recordedSource(tsv) {
  const m = /^# Source: RAWmakase \S+ @ ([0-9a-f]{40})(?: \((v[^)]+)\))?,/m.exec(tsv);
  return m ? { sha: m[1], tag: m[2] ?? null } : null;
}

/** The table rewritten from RAWmakase's record: our comments kept, the Source line and rows replaced. */
export function regenerate(tsv, baselines, photos, source, leftOut = LEFT_OUT) {
  const lines = tsv.split('\n');
  const end = lines.findIndex((l) => !l.startsWith('#'));
  const header = lines.slice(0, end === -1 ? lines.length : end);
  const at = header.findIndex((l) => l.startsWith('# Source:'));
  if (at === -1) throw new Error(`${TABLE} has no "# Source:" line in its header`);
  const rows = lensRows(baselines, photos, leftOut);
  header[at] = sourceLine(source, rows, leftOut);
  return `${[...header, ...rows].join('\n')}\n`;
}

/** RAWmakase's record at one commit. */
export async function fetchBaselines(sha) {
  const url = `https://raw.githubusercontent.com/pch/rawmakase/${sha}/${SOURCE_PATH}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(60000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.json();
}

/** For check:sources: a warning when RAWmakase has released past the table, else null. */
export async function lensWarning(root) {
  // A checkout without the table (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, TABLE))) return null;
  const recorded = recordedSource(readFileSync(join(root, TABLE), 'utf8'));
  if (!recorded) return `${TABLE} names no RAWmakase source - node scripts/rawmakase-lens-baselines.mjs <release>`;
  let tags;
  try {
    tags = await releases();
  } catch (error) {
    return `could not list RAWmakase's releases, so ${TABLE} is unchecked (${error.code ?? error.message})`;
  }
  const [newest, sha] = [...tags].at(-1) ?? [];
  if (!newest || sha === recorded.sha) return null;
  const at = recorded.tag ?? `commit ${recorded.sha.slice(0, 8)}, not a release`;
  return `RAWmakase ${newest} is out and the lens baselines are at ${at} - node scripts/rawmakase-lens-baselines.mjs ${newest}`;
}

async function main(ref) {
  if (!ref) throw new Error('usage: node scripts/rawmakase-lens-baselines.mjs <release tag, or full commit>');
  const tags = await releases();
  let source;
  if (tags.has(ref)) source = { sha: tags.get(ref), tag: ref };
  else if (/^[0-9a-f]{40}$/.test(ref)) source = { sha: ref, tag: [...tags].find(([, s]) => s === ref)?.[0] ?? null };
  else throw new Error(`${ref} is not a RAWmakase release tag or a full 40-character commit`);

  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const path = join(root, TABLE);
  const before = readFileSync(path, 'utf8');
  const photos = goldenPhotos(readFileSync(join(root, SAMPLES), 'utf8'));
  const after = regenerate(before, await fetchBaselines(source.sha), photos, source);
  writeFileSync(path, after);
  console.log(`  ${TABLE}: ${source.tag ?? 'commit'} ${source.sha.slice(0, 8)}`);
  console.log(before === after ? '  unchanged' : '  changed - the golden photos run compares our readers to it');
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv[2]).catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
