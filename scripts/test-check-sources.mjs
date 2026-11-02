// Tests for scripts/check-sources.mjs.
//
// The one that matters: every wiring line is in today's tree, and a tree that
// lost one fails. The rest test the warnings without the network.
//
// Run:  npm run test:checks   (or node scripts/test-check-sources.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  WIRING,
  harvestWarning,
  missingWiring,
  pinWarning,
  pinnedFlashRaw,
  pinnedRawler,
  rawlerWarning,
  reviewedRelease,
} from './check-sources.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const readTree = (file) => {
  try {
    return readFileSync(join(root, file), 'utf8');
  } catch {
    return null;
  }
};

let ran = 0;
let failed = 0;
const test = (name, fn) => {
  ran += 1;
  try {
    fn();
    console.log(`  ok    ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`  FAIL  ${name}`);
    for (const line of String(error.message ?? error).split('\n')) console.log(`          ${line}`);
  }
};

test('every wiring line is in the tree', () => {
  const missing = missingWiring(readTree);
  assert.deepEqual(
    missing.map(({ file, pattern }) => `${file} ${pattern}`),
    [],
  );
});

test('a file that lost its wiring line is reported', () => {
  const [first] = WIRING;
  const read = (file) => (file === first.file ? readTree(file).replace(first.pattern, '= removed(') : readTree(file));
  assert.deepEqual(
    missingWiring(read).map(({ pattern }) => pattern),
    [first.pattern],
  );
});

test('a file that is gone is reported once per line it held', () => {
  const read = (file) => (file === 'src-tauri/src/formats.rs' ? null : readTree(file));
  assert.equal(missingWiring(read).length, 1);
});

test('the dependency-list pattern is not satisfied by a comment', () => {
  const rule = WIRING.find(({ pattern }) => String(pattern).includes('tiffDepth'));
  assert.equal(rule.pattern.test('// see [tiffDepth] in the effect'), false);
  assert.equal(rule.pattern.test('}, [format, tiffDepth]);'), true);
});

test('the CHANGELOG names the RapidRAW release last read', () => {
  assert.match(reviewedRelease(readTree('CHANGELOG.md')) ?? '', /^v\d+\.\d+\.\d+$/);
  assert.equal(reviewedRelease('Forked from RapidRAW 1.6.6; read up to `v1.6.7`.'), 'v1.6.7');
  assert.equal(reviewedRelease('no marker here'), null);
});

test('a newer RapidRAW release asks for a harvest review', () => {
  const tags = new Map([
    ['v1.6.6', 'a'],
    ['v1.6.7', 'b'],
    ['v1.6.8', 'c'],
  ]);
  assert.match(harvestWarning('v1.6.7', tags), /v1\.6\.8 is out .* read up to v1\.6\.7/);
  assert.equal(harvestWarning('v1.6.8', tags), null);
  assert.match(harvestWarning(null, tags), /names no RapidRAW release/);
});

test('the engine pins one FlashRaw version', () => {
  assert.match(pinnedFlashRaw(readTree('src-tauri/engine/Cargo.toml')) ?? '', /^\d+\.\d+\.\d+$/);
  assert.equal(pinnedFlashRaw('flashraw = { version = "=0.1.1", default-features = false }'), '0.1.1');
  assert.equal(pinnedFlashRaw('flashraw = "0.1"'), null);
  assert.equal(pinnedFlashRaw('flashraw-core = "=0.1.1"'), null);
});

test('a newer FlashRaw release asks for a measurement', () => {
  const tags = new Map([
    ['v0.1.1', 'a'],
    ['v0.1.10', 'b'],
  ]);
  assert.match(pinWarning('0.1.1', tags), /FlashRaw v0\.1\.10 is out and the engine pins 0\.1\.1/);
  assert.equal(pinWarning('0.1.10', tags), null);
  assert.equal(pinWarning('0.2.0', tags), null); // published before it was tagged
  assert.match(pinWarning(null, tags), /no longer pins FlashRaw/);
});

test('Argentum pins one rawler commit', () => {
  assert.match(pinnedRawler(readTree('src-tauri/Cargo.toml')) ?? '', /^[0-9a-f]{40}$/);
  assert.equal(pinnedRawler('rawler = { git = "https://example.com/x.git", rev = "abc123" }'), 'abc123');
  assert.equal(pinnedRawler('rawler = "0.7"'), null);
  assert.equal(pinnedRawler('rawler-core = { git = "x", rev = "abc123" }'), null);
  assert.equal(pinnedRawler('# rawler = { git = "x", rev = "abc123" }'), null);
});

// `test` is synchronous, so these runs happen first and their results are asserted below.
const ours = pinnedRawler(readTree('src-tauri/Cargo.toml'));
const shoeboxPinning = (rev) => async () => `[dependencies]\nrawler = { git = "x", rev = "${rev}" }\n`;
const samePin = await rawlerWarning(root, shoeboxPinning(ours));
const otherPin = await rawlerWarning(root, shoeboxPinning('16b9b01068ac1f7a9b5319705a075ef6c7789d7a'));
const noPin = await rawlerWarning(root, async () => '404: Not Found');
const offline = await rawlerWarning(root, async () => {
  throw new TypeError('fetch failed');
});
const noCheckout = await rawlerWarning(join(root, 'no-such-checkout'), shoeboxPinning(ours));

test('a matching rawler pin says nothing', () => {
  assert.equal(samePin, null);
});

test('a different rawler pin names both commits and asks to align them', () => {
  assert.ok(otherPin.includes(ours.slice(0, 7)), otherPin);
  assert.ok(otherPin.includes('16b9b01'), otherPin);
  assert.ok(!otherPin.includes('16b9b010'), otherPin); // short commits, not the full ones
  assert.match(otherPin, /move both to the same commit/);
});

test('a Shoebox file that cannot be read is one line, not a failure', () => {
  assert.match(noPin, /Shoebox/);
  assert.match(offline, /could not read Shoebox's Cargo\.toml \(fetch failed\)/);
  for (const warning of [noPin, offline]) assert.ok(!warning.includes('\n'), warning);
});

test('a checkout without the Cargo.toml has nothing to check', () => {
  assert.equal(noCheckout, null);
});

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exitCode = failed > 0 ? 1 : 0;
