// Tests for scripts/rawtherapee-profiles.mjs.
//
// The one that matters: the committed table is exactly what the script makes
// from the RawTherapee commit it names, byte for byte. A hand edit to a row, or
// a script change that would alter one, fails here. It reads the listing from
// GitHub; offline it is skipped, in CI it fails.
//
// Run:  npm run test:checks   (or node scripts/test-rawtherapee-profiles.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { TABLE, fetchSource, recordedSource, regenerate, sourceLine, tableRows } from './rawtherapee-profiles.mjs';

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

const hash = (c) => c.repeat(40);
const listing = [
  { type: 'file', name: 'Canon EOS 100D.dcp', size: 10, sha: hash('a') },
  { type: 'file', name: 'NIKON D800.dcp', size: 20, sha: hash('b') },
  { type: 'file', name: 'NIKON D800E.dcp', size: 30, sha: hash('c') },
  { type: 'file', name: 'camera_model_aliases.json', size: 5, sha: hash('d') },
  { type: 'dir', name: 'old', size: 0, sha: hash('e') },
];
const aliases = {
  'Canon EOS 100D': ['Canon EOS Kiss X7', 'Canon EOS REBEL SL1'],
  'NIKON D800E': ['NIKON D800'],
  'Canon EOS 200D': ['Canon EOS Kiss X9'],
};

await test('one row per profile, with its other names, sorted by file name', () => {
  assert.deepEqual(tableRows(listing, aliases), [
    `Canon EOS 100D.dcp\t10\t${hash('a')}\tCanon EOS Kiss X7;Canon EOS REBEL SL1`,
    `NIKON D800.dcp\t20\t${hash('b')}\t`,
    `NIKON D800E.dcp\t30\t${hash('c')}\t`,
  ]);
});

await test('a name with a profile of its own is never another body\'s other name', () => {
  const rows = tableRows(listing, aliases);
  assert.ok(!rows.some((r) => r.startsWith('NIKON D800E') && r.includes('NIKON D800\t')));
  assert.ok(!rows.join('\n').includes('Kiss X9'), 'an alias of a body with no profile');
});

await test('a file without a git hash stops the script', () => {
  assert.throws(() => tableRows([{ type: 'file', name: 'x.dcp', size: 1, sha: 'nope' }], {}), /no git hash/);
});

await test('the Source line names the commit and counts, and is read back', () => {
  const rows = tableRows(listing, aliases);
  const line = sourceLine(hash('f'), rows);
  assert.match(line, /@ f{40}, 3 profiles, 2 other names$/);
  assert.equal(recordedSource(line), hash('f'));
});

await test(`${TABLE} is what the script makes from the RawTherapee commit it names`, async () => {
  const tsv = readFileSync(join(root, TABLE), 'utf8');
  const sha = recordedSource(tsv);
  assert.ok(sha, 'the table has no "# Source:" line');
  let source;
  try {
    source = await fetchSource(sha);
  } catch (error) {
    if (inCI) throw error;
    return `could not fetch RawTherapee's listing: ${error.cause?.code ?? error.message}`;
  }
  const made = regenerate(tsv, source.listing, source.aliases, sha);
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
