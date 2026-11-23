// RAWmakase's black & white chart, regenerated as text.
//
// B&W's mix (src-tauri/engine/src/rm_black_white.rs) renders from RAWmakase's
// crates/rawmakase-engine/src/develop/black_white_chart.bin: Camera Raw's own
// renders of a colour chart, measured. The repository keeps no binaries, so the
// chart is kept as text (src-tauri/engine/src/rm_black_white_chart.tsv), one row per
// table and hue, and this script writes it from one RAWmakase release, as
// scripts/rawmakase-cameras.mjs writes the camera table (DEC-34):
//
//   * one source version at a time, named on the table's "# Source:" line;
//   * the rows are generated, never edited by hand;
//   * check:sources warns when RAWmakase releases past the version named.
//
// No exceptions list: a chart is measured as a whole, so a value of ours that
// differs from theirs would be a different chart. The table's other comments
// are ours and kept as they are.
//
// Run:  node scripts/rawmakase-gray-chart.mjs <release tag, or full commit>
//       e.g. node scripts/rawmakase-gray-chart.mjs v0.4.0
// Test: node scripts/test-rawmakase-gray-chart.mjs

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { releases } from './rawmakase-cameras.mjs';

export const SOURCE_PATH = 'crates/rawmakase-engine/src/develop/black_white_chart.bin';
export const TABLE = 'src-tauri/engine/src/rm_black_white_chart.tsv';

/** The colour mixer's grid: hue x saturation x value cells, each table in that order. */
const HUES = 36;
const CELLS_PER_HUE = 6 * 6;
export const CELLS = HUES * CELLS_PER_HUE;

/** The bands, as RAWmakase orders its mix. */
export const BANDS = ['red', 'orange', 'yellow', 'green', 'aqua', 'blue', 'purple', 'magenta'];

/** Each band was measured at these slider positions. */
export const POSITIONS = ['-100', '-50', '+50', '+100'];

/** The tables in the file's order: the zero mix, then each band at each position. */
export const TABLE_NAMES = ['zero', ...BANDS.flatMap((band) => POSITIONS.map((p) => `${band}${p}`))];

/**
 * The chart's rows: table name, hue index, then that hue's 36 cells
 * (saturation outer, value inner), each the file's little-endian i16 as written.
 */
export function chartRows(bytes) {
  const expected = TABLE_NAMES.length * CELLS * 2;
  if (bytes.length !== expected) {
    throw new Error(
      `the chart is ${bytes.length} bytes, not ${expected} (${TABLE_NAMES.length} tables of ${CELLS} i16 cells): read its format before regenerating`,
    );
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const rows = [];
  TABLE_NAMES.forEach((name, table) => {
    for (let hue = 0; hue < HUES; hue++) {
      const cells = [];
      for (let k = 0; k < CELLS_PER_HUE; k++) {
        cells.push(view.getInt16(2 * (table * CELLS + hue * CELLS_PER_HUE + k), true));
      }
      rows.push([name, hue, ...cells].join('\t'));
    }
  });
  return rows;
}

/** The "# Source:" line: which RAWmakase commit, and its release. */
export function sourceLine({ sha, tag }) {
  return (
    `# Source: RAWmakase ${SOURCE_PATH} @ ${sha}${tag ? ` (${tag})` : ''}, ` +
    `${TABLE_NAMES.length} tables of ${CELLS} cells`
  );
}

/** The commit and release a table names, or null if it names none. */
export function recordedSource(tsv) {
  const m = /^# Source: RAWmakase \S+ @ ([0-9a-f]{40})(?: \((v[^)]+)\))?,/m.exec(tsv);
  return m ? { sha: m[1], tag: m[2] ?? null } : null;
}

/** The table rewritten from the chart: our comments kept, the Source line and rows replaced. */
export function regenerate(tsv, bytes, source) {
  const lines = tsv.split('\n');
  const end = lines.findIndex((l) => !l.startsWith('#'));
  const header = lines.slice(0, end === -1 ? lines.length : end);
  const at = header.findIndex((l) => l.startsWith('# Source:'));
  if (at === -1) throw new Error(`${TABLE} has no "# Source:" line in its header`);
  header[at] = sourceLine(source);
  return `${[...header, ...chartRows(bytes)].join('\n')}\n`;
}

/** The chart at one commit. */
export async function fetchChart(sha) {
  const url = `https://raw.githubusercontent.com/pch/rawmakase/${sha}/${SOURCE_PATH}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(20000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

/** For check:sources: a warning when RAWmakase has released past the chart, else null. */
export async function chartWarning(root) {
  // A checkout without the table (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, TABLE))) return null;
  const recorded = recordedSource(readFileSync(join(root, TABLE), 'utf8'));
  if (!recorded) return `${TABLE} names no RAWmakase source - node scripts/rawmakase-gray-chart.mjs <release>`;
  let tags;
  try {
    tags = await releases();
  } catch (error) {
    return `could not list RAWmakase's releases, so ${TABLE} is unchecked (${error.code ?? error.message})`;
  }
  const [newest, sha] = [...tags].at(-1) ?? [];
  if (!newest || sha === recorded.sha) return null;
  const at = recorded.tag ?? `commit ${recorded.sha.slice(0, 8)}, not a release`;
  return `RAWmakase ${newest} is out and the B&W chart is at ${at} - node scripts/rawmakase-gray-chart.mjs ${newest}`;
}

async function main(ref) {
  if (!ref) throw new Error('usage: node scripts/rawmakase-gray-chart.mjs <release tag, or full commit>');
  const tags = await releases();
  let source;
  if (tags.has(ref)) source = { sha: tags.get(ref), tag: ref };
  else if (/^[0-9a-f]{40}$/.test(ref)) source = { sha: ref, tag: [...tags].find(([, s]) => s === ref)?.[0] ?? null };
  else throw new Error(`${ref} is not a RAWmakase release tag or a full 40-character commit`);

  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const path = join(root, TABLE);
  const before = readFileSync(path, 'utf8');
  const after = regenerate(before, await fetchChart(source.sha), source);
  writeFileSync(path, after);
  console.log(`  ${TABLE}: ${source.tag ?? 'commit'} ${source.sha.slice(0, 8)}`);
  console.log(before === after ? '  unchanged' : '  changed - show B&W photos side by side before merging');
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv[2]).catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
