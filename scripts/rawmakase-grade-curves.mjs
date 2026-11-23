// RAWmakase's colour grading curves, regenerated as text.
//
// Lightroom's colour grading (src-tauri/engine/src/rm_color_grade.rs) renders
// from RAWmakase's crates/rawmakase-engine/src/develop/color_grade_curves.bin:
// Camera Raw's own renders of a colour chart under about 740 grading settings,
// measured. The repository keeps no binaries, so the tables are kept as text
// (src-tauri/engine/src/rm_color_grade_curves.tsv), one row per measured
// setting, and this script writes them from one RAWmakase release, as
// scripts/rawmakase-cameras.mjs writes the camera table (DEC-34):
//
//   * one source version at a time, named on the table's "# Source:" line;
//   * the rows are generated, never edited by hand;
//   * check:sources warns when RAWmakase releases past the version named.
//
// No exceptions list: the tables are fitted as a whole, so a value of ours
// that differs from theirs would be a different fit. The table's other
// comments are ours and kept as they are.
//
// Run:  node scripts/rawmakase-grade-curves.mjs <release tag, or full commit>
//       e.g. node scripts/rawmakase-grade-curves.mjs v0.4.1
// Test: node scripts/test-rawmakase-grade-curves.mjs

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { releases } from './rawmakase-cameras.mjs';

export const SOURCE_PATH = 'crates/rawmakase-engine/src/develop/color_grade_curves.bin';
export const TABLE = 'src-tauri/engine/src/rm_color_grade_curves.tsv';

// The file's layout, from RAWmakase's color_grade_curves.rs: samples along a
// channel, the measured Blending and Balance, hues and saturations, and the
// Luminance positions (0, no change, is not stored).
const SAMPLES = 64;
const REGIONS = ['shadows', 'midtones', 'highlights', 'global'];
const BLENDINGS = [0, 25, 50, 75, 100];
const BALANCES = [-100, -88, -75, -62, -50, -38, -25, -12, 0, 12, 25, 38, 50, 62, 75, 88, 100];
const HUES = Array.from({ length: 12 }, (_, i) => i * 30);
const SATURATIONS = [25, 50, 75, 100];
const LUMINANCES = [-100, -75, -50, -25, 25, 50, 75, 100];
const SHAPED_TERMS = 2;
const TERMS = 3;

/**
 * The tables in the file's order, each a row: four label columns (what was
 * measured, the region and two settings, "-" where none) and its values.
 */
const SECTIONS = [
  ...REGIONS.slice(0, 3).flatMap((region) =>
    BLENDINGS.flatMap((b) =>
      BALANCES.map((a) => ({
        labels: ['profile', region, `blending ${b}`, `balance ${a}`],
        values: SAMPLES * SHAPED_TERMS,
      })),
    ),
  ),
  { labels: ['profile', 'global', '-', '-'], values: SAMPLES * TERMS },
  ...REGIONS.flatMap((region) =>
    HUES.flatMap((h) =>
      SATURATIONS.map((s) => ({
        labels: ['tint', region, `hue ${h}`, `saturation ${s}`],
        values: 3 * TERMS,
      })),
    ),
  ),
  ...REGIONS.flatMap((region) =>
    LUMINANCES.map((l) => ({
      labels: ['luminance', region, `amount ${l}`, '-'],
      values: SAMPLES,
    })),
  ),
];
export const ROWS = SECTIONS.length;
export const VALUES = SECTIONS.reduce((n, s) => n + s.values, 0);

/** An f32 as the fewest digits that read back as exactly the same f32. */
export function shortest(v) {
  if (Object.is(v, -0)) return '-0';
  for (let digits = 1; digits <= 9; digits++) {
    const text = String(Number(v.toPrecision(digits)));
    if (Math.fround(Number(text)) === v) return text;
  }
  throw new Error(`${v} does not round-trip as an f32`);
}

/** The tables' rows, each the file's little-endian f32 values as written. */
export function curveRows(bytes) {
  if (bytes.length !== VALUES * 4) {
    throw new Error(
      `the curves are ${bytes.length} bytes, not ${VALUES * 4} (${VALUES} f32 values): read its format before regenerating`,
    );
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  return SECTIONS.map(({ labels, values }) => {
    const row = [];
    for (let k = 0; k < values; k++, at++) row.push(shortest(view.getFloat32(4 * at, true)));
    return [...labels, ...row].join('\t');
  });
}

/** The "# Source:" line: which RAWmakase commit, and its release. */
export function sourceLine({ sha, tag }) {
  return `# Source: RAWmakase ${SOURCE_PATH} @ ${sha}${tag ? ` (${tag})` : ''}, ` + `${ROWS} rows, ${VALUES} values`;
}

/** The commit and release a table names, or null if it names none. */
export function recordedSource(tsv) {
  const m = /^# Source: RAWmakase \S+ @ ([0-9a-f]{40})(?: \((v[^)]+)\))?,/m.exec(tsv);
  return m ? { sha: m[1], tag: m[2] ?? null } : null;
}

/** The table rewritten from the curves: our comments kept, the Source line and rows replaced. */
export function regenerate(tsv, bytes, source) {
  const lines = tsv.split('\n');
  const end = lines.findIndex((l) => !l.startsWith('#'));
  const header = lines.slice(0, end === -1 ? lines.length : end);
  const at = header.findIndex((l) => l.startsWith('# Source:'));
  if (at === -1) throw new Error(`${TABLE} has no "# Source:" line in its header`);
  header[at] = sourceLine(source);
  return `${[...header, ...curveRows(bytes)].join('\n')}\n`;
}

/** The curves at one commit. */
export async function fetchCurves(sha) {
  const url = `https://raw.githubusercontent.com/pch/rawmakase/${sha}/${SOURCE_PATH}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(20000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

/** For check:sources: a warning when RAWmakase has released past the curves, else null. */
export async function curvesWarning(root) {
  // A checkout without the table (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, TABLE))) return null;
  const recorded = recordedSource(readFileSync(join(root, TABLE), 'utf8'));
  if (!recorded) return `${TABLE} names no RAWmakase source - node scripts/rawmakase-grade-curves.mjs <release>`;
  let tags;
  try {
    tags = await releases();
  } catch (error) {
    return `could not list RAWmakase's releases, so ${TABLE} is unchecked (${error.code ?? error.message})`;
  }
  const [newest, sha] = [...tags].at(-1) ?? [];
  if (!newest || sha === recorded.sha) return null;
  const at = recorded.tag ?? `commit ${recorded.sha.slice(0, 8)}, not a release`;
  return `RAWmakase ${newest} is out and the colour grading curves are at ${at} - node scripts/rawmakase-grade-curves.mjs ${newest}`;
}

async function main(ref) {
  if (!ref) throw new Error('usage: node scripts/rawmakase-grade-curves.mjs <release tag, or full commit>');
  const tags = await releases();
  let source;
  if (tags.has(ref)) source = { sha: tags.get(ref), tag: ref };
  else if (/^[0-9a-f]{40}$/.test(ref)) source = { sha: ref, tag: [...tags].find(([, s]) => s === ref)?.[0] ?? null };
  else throw new Error(`${ref} is not a RAWmakase release tag or a full 40-character commit`);

  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const path = join(root, TABLE);
  const before = readFileSync(path, 'utf8');
  const after = regenerate(before, await fetchCurves(source.sha), source);
  writeFileSync(path, after);
  console.log(`  ${TABLE}: ${source.tag ?? 'commit'} ${source.sha.slice(0, 8)}`);
  console.log(before === after ? '  unchanged' : '  changed - show graded photos side by side before merging');
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv[2]).catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
