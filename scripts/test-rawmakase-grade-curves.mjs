// Tests for scripts/rawmakase-grade-curves.mjs.
//
// The one that matters: the committed table is exactly what the script makes
// from the RAWmakase commit it names, byte for byte. A hand edit to a row, or a
// script change that would alter one, fails here. It reads the curves from
// GitHub; offline it is skipped, in CI it fails.
//
// Run:  npm run test:checks   (or node scripts/test-rawmakase-grade-curves.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  ROWS,
  TABLE,
  VALUES,
  curveRows,
  fetchCurves,
  recordedSource,
  regenerate,
  shortest,
  sourceLine,
} from './rawmakase-grade-curves.mjs';

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

/** Curves whose every value is its own index in the file. */
function numbered() {
  const bytes = new Uint8Array(VALUES * 4);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < VALUES; i++) view.setFloat32(4 * i, i, true);
  return bytes;
}

await test("a row is four labels and its values, in the file's order", () => {
  const rows = curveRows(numbered());
  assert.equal(rows.length, ROWS);
  const first = rows[0].split('\t');
  assert.deepEqual(first.slice(0, 4), ['profile', 'shadows', 'blending 0', 'balance -100']);
  assert.equal(first.length, 4 + 128);
  assert.equal(first[4], '0');
  // Global's profile follows the three shaped regions' 255 rows of 128 values.
  const global = rows[255].split('\t');
  assert.deepEqual(global.slice(0, 2), ['profile', 'global']);
  assert.equal(global[4], String(255 * 128));
  assert.deepEqual(rows.at(-1).split('\t').slice(0, 3), ['luminance', 'global', 'amount 100']);
  assert.equal(rows.at(-1).split('\t').at(-1), String(VALUES - 1));
});

await test('values are written in as few digits as read back exactly', () => {
  for (const v of [0, 1, -0.5, Math.fround(0.1), Math.fround(-2.8378077), Math.fround(1e-7)]) {
    assert.equal(Math.fround(Number(shortest(v))), v);
  }
  assert.equal(shortest(Math.fround(0.1)), '0.1');
  assert.equal(shortest(-0), '-0');
});

await test('curves of another size stop the script', () => {
  assert.throws(() => curveRows(new Uint8Array(12)), /not \d+/);
});

await test('the Source line names the commit and release, and is read back', () => {
  const sha = 'a'.repeat(40);
  assert.deepEqual(recordedSource(sourceLine({ sha, tag: 'v1.2.3' })), { sha, tag: 'v1.2.3' });
  assert.deepEqual(recordedSource(sourceLine({ sha, tag: null })), { sha, tag: null });
});

await test('regenerating keeps our comments and replaces the Source line and rows', () => {
  const sha = 'b'.repeat(40);
  const tsv = '# ours\n# Source: old\n# also ours\nprofile\tshadows\t-\t-\t1\n';
  const lines = regenerate(tsv, numbered(), { sha, tag: null }).split('\n');
  assert.equal(lines[0], '# ours');
  assert.equal(lines[1], sourceLine({ sha, tag: null }));
  assert.equal(lines[2], '# also ours');
  assert.equal(lines.length, 3 + ROWS + 1);
});

await test(`${TABLE} is what the script makes from the RAWmakase commit it names`, async () => {
  const tsv = readFileSync(join(root, TABLE), 'utf8');
  const source = recordedSource(tsv);
  assert.ok(source, 'the table has no "# Source:" line');
  let bytes;
  try {
    bytes = await fetchCurves(source.sha);
  } catch (error) {
    if (inCI) throw error;
    return `could not fetch the curves: ${error.cause?.code ?? error.message}`;
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
