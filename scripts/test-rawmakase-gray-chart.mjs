// Tests for scripts/rawmakase-gray-chart.mjs.
//
// The one that matters: the committed chart is exactly what the script makes
// from the RAWmakase commit it names, byte for byte. A hand edit to a row, or a
// script change that would alter one, fails here. It reads the chart from
// GitHub; offline it is skipped, in CI it fails.
//
// Run:  npm run test:checks   (or node scripts/test-rawmakase-gray-chart.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  CELLS,
  TABLE,
  TABLE_NAMES,
  chartRows,
  fetchChart,
  recordedSource,
  regenerate,
  sourceLine,
} from './rawmakase-gray-chart.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const inCI = process.env.CI === 'true' || process.env.CI === '1';

let ran = 0;
let failed = 0;
const test = async (name, fn) => {
  ran += 1;
  try {
    const skipped = await fn();
    console.log(`  ${skipped ? `skip  ${name} (${skipped})` : `ok    ${name}`}`);
  } catch (error) {
    failed += 1;
    console.log(`  FAIL  ${name}`);
    for (const line of String(error.message ?? error).split('\n')) console.log(`          ${line}`);
  }
};

/** A chart whose every cell holds its own index in the file, as an i16. */
function numbered() {
  const bytes = new Uint8Array(TABLE_NAMES.length * CELLS * 2);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < TABLE_NAMES.length * CELLS; i++) view.setInt16(2 * i, (i % 30000) - 15000, true);
  return bytes;
}

await test("a row is a table, a hue and its 36 cells, in the file's order", () => {
  const rows = chartRows(numbered());
  assert.equal(rows.length, TABLE_NAMES.length * 36);
  assert.equal(rows[0], ['zero', 0, ...Array.from({ length: 36 }, (_, k) => k - 15000)].join('\t'));
  // The second table starts after the first's 1296 cells: red at -100, hue 0.
  assert.equal(rows[36].split('\t').slice(0, 3).join(' '), `red-100 0 ${CELLS - 15000}`);
  assert.equal(rows.at(-1).split('\t')[0], 'magenta+100');
});

await test('a chart of another size stops the script', () => {
  assert.throws(() => chartRows(new Uint8Array(10)), /not \d+/);
});

await test('the Source line names the commit and release, and is read back', () => {
  const sha = 'a'.repeat(40);
  assert.deepEqual(recordedSource(sourceLine({ sha, tag: 'v1.2.3' })), { sha, tag: 'v1.2.3' });
  assert.deepEqual(recordedSource(sourceLine({ sha, tag: null })), { sha, tag: null });
});

await test('regenerating keeps our comments and replaces the Source line and rows', () => {
  const sha = 'b'.repeat(40);
  const tsv = '# ours\n# Source: old\n# also ours\nzero\t0\t1\n';
  const made = regenerate(tsv, numbered(), { sha, tag: null });
  const lines = made.split('\n');
  assert.equal(lines[0], '# ours');
  assert.equal(lines[1], sourceLine({ sha, tag: null }));
  assert.equal(lines[2], '# also ours');
  assert.equal(lines.length, 3 + TABLE_NAMES.length * 36 + 1);
});

await test(`${TABLE} is what the script makes from the RAWmakase commit it names`, async () => {
  const tsv = readFileSync(join(root, TABLE), 'utf8');
  const source = recordedSource(tsv);
  assert.ok(source, 'the table has no "# Source:" line');
  let bytes;
  try {
    bytes = await fetchChart(source.sha);
  } catch (error) {
    if (inCI) throw error;
    return `could not fetch the chart: ${error.cause?.code ?? error.message}`;
  }
  const made = regenerate(tsv, bytes, source);
  if (made !== tsv) {
    const [a, b] = [tsv.split('\n'), made.split('\n')];
    let i = 0;
    while (a[i] === b[i]) i += 1;
    assert.fail(
      `line ${i + 1} differs - regenerate it, never edit it:\n  table:  ${a[i]?.slice(0, 80)}\n  script: ${b[i]?.slice(0, 80)}`,
    );
  }
});

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exitCode = failed > 0 ? 1 : 0;
