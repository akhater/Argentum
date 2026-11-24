// Tests for scripts/check-compat-corpus.mjs, on the real corpus and on a folder made here.
//
// Run:  npm run test:checks   (or node scripts/test-check-compat-corpus.mjs)

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { FOLDER_FILES, cargoVersion, problems } from './check-compat-corpus.mjs';

let failed = 0;
const test = (name, fn) => {
  try {
    fn();
    console.log(`  ok    ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`  FAIL  ${name}`);
    for (const line of String(error.message ?? error).split('\n')) console.log(`          ${line}`);
  }
};

const corpus = (version, files) => {
  const root = mkdtempSync(join(tmpdir(), 'compat-'));
  mkdirSync(join(root, version), { recursive: true });
  mkdirSync(join(root, 'snapshots'));
  for (const file of files) writeFileSync(join(root, version, file), '{}');
  writeFileSync(join(root, 'snapshots', `${version}.txt`), '');
  return root;
};

test('a version with no folder is named, folder and snapshot', () => {
  const found = problems('99.99.9', undefined, corpus('26.1.1', FOLDER_FILES));
  assert.equal(found.length, FOLDER_FILES.length + 1);
  assert.match(found.join('\n'), /compat\/99\.99\.9\/settings\.json is missing/);
  assert.match(found.join('\n'), /snapshots\/99\.99\.9\.txt is missing/);
});

test('a complete folder passes', () => {
  assert.deepEqual(problems('26.1.1', '26.1.1', corpus('26.1.1', FOLDER_FILES)), []);
});

test('a folder without the processing settings file is named', () => {
  const found = problems('26.1.1', undefined, corpus('26.1.1', FOLDER_FILES.slice(0, 2)));
  assert.deepEqual(found, ['src-tauri/fixtures/compat/26.1.1/argentum-processing.json is missing']);
});

test('a tag that is not the Cargo.toml version fails', () => {
  assert.match(problems('26.1.1', '26.1.2', corpus('26.1.1', FOLDER_FILES))[0], /tagged 26\.1\.2/);
});

test('the version is read from Cargo.toml', () => {
  assert.equal(cargoVersion('[package]\nname = "x"\nversion = "26.47.4"\n'), '26.47.4');
});

process.exit(failed ? 1 : 0);
