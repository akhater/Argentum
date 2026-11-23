// Tests for scripts/rawmakase-cameras.mjs.
//
// The one that matters: the committed table is exactly what the script makes
// from the RAWmakase commit it names, byte for byte. A hand edit to a row, or a
// script change that would alter one, fails here. It reads cameras.toml from
// GitHub; offline it is skipped, in CI it fails.
//
// Run:  npm run test:checks   (or node scripts/test-rawmakase-cameras.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { TABLE, fetchCameras, parseCameras, recordedSource, regenerate, tableRows } from './rawmakase-cameras.mjs';

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

const TOML = `
# a comment
[[camera]]
make = "Canon"
model = "EOS 100D"
aliases = ["EOS Rebel SL1", "EOS Kiss X7"]
baseline_exposure = -0.10
levels = [2047, 12277, 16383]
unit_calibration = { preset = "daylight", reference = [2185, 1024, 1584] }
source = "adobe-dng"

[[camera]]
make = "Sony"
model = "ILCE-7M3"
baseline_exposure = 0.0  # a trailing comment
lowest_iso = 100
`;

const exception = {
  make: 'Sony',
  model: 'ILCE-7M3',
  field: 'baseline_exposure',
  value: '0.2',
  reason: 'test',
  removedWhen: 'test',
};

await test('a row is make, model|aliases, offset, lowest ISO and levels, numbers in shortest form', () => {
  assert.deepEqual(tableRows(parseCameras(TOML), []), [
    'Canon\tEOS 100D|EOS Rebel SL1|EOS Kiss X7\t-0.1\t\t2047,12277,16383',
    'Sony\tILCE-7M3\t0\t100\t',
  ]);
});

await test('an exception replaces one value', () => {
  assert.equal(tableRows(parseCameras(TOML), [exception])[1], 'Sony\tILCE-7M3\t0.2\t100\t');
});

await test('an exception stops the script once it is stale', () => {
  assert.throws(() => tableRows(parseCameras(TOML), [{ ...exception, model: 'ILCE-9' }]), /no such body/);
  assert.throws(() => tableRows(parseCameras(TOML), [{ ...exception, value: '0' }]), /RAWmakase now has/);
  assert.throws(() => tableRows(parseCameras(TOML), [{ ...exception, reason: '' }]), /needs a field/);
});

await test('a change of format in cameras.toml stops the script', () => {
  assert.throws(() => parseCameras('[meta]\nversion = 1\n'), /not a key = value line/);
  assert.throws(() => parseCameras('[[camera]]\nlevels = [\n  1,\n]\n'), /one-line array/);
  assert.throws(() => parseCameras('[[camera]]\nmodel = "A|B"\n'), /plain string/);
  assert.throws(() => tableRows(parseCameras('[[camera]]\nmake = "A"\nmodel = "B"\n'), []), /without/);
});

await test(`${TABLE} is what the script makes from the RAWmakase commit it names`, async () => {
  const tsv = readFileSync(join(root, TABLE), 'utf8');
  const source = recordedSource(tsv);
  assert.ok(source, 'the table has no "# Source:" line');
  let toml;
  try {
    toml = await fetchCameras(source.sha);
  } catch (error) {
    if (inCI) throw error;
    return `could not fetch cameras.toml: ${error.cause?.code ?? error.message}`;
  }
  // assert.equal would print 270 lines; name the first that differs instead.
  const made = regenerate(tsv, toml, source);
  if (made !== tsv) {
    const [a, b] = [tsv.split('\n'), made.split('\n')];
    let i = 0;
    while (a[i] === b[i]) i += 1;
    assert.fail(`line ${i + 1} differs - regenerate it, never edit it:\n  table:  ${a[i]}\n  script: ${b[i]}`);
  }
});

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exitCode = failed > 0 ? 1 : 0;
