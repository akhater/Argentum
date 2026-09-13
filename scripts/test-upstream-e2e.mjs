// End-to-end tests: build a fork in a throwaway repository and run the real
// check-mergeability.mjs against it, asserting the exit code and the message.
//
// WHY THESE EXIST SEPARATELY
//
// test-upstream-checks.mjs tests the functions that decide what the gates see.
// That is not the same as testing that the gates fail. Two bypasses got through
// review precisely because the pieces were right and the wiring was not:
//
//   * a retirement stopped an entry generating requirements for exactly one
//     window, after which the entry came back and was live again for every
//     review that followed;
//   * deleting a registry entry and its markers in the same commit left nothing
//     in the tree and nothing in the registry to disagree about, so the
//     requirement quietly stopped existing.
//
// So each scenario here writes a real registry and a real review register into a
// real repository, runs the actual checker as a subprocess, and asserts it fails
// - and, for the cases that should be allowed, that it does not.
//
// Run:  npm run test:checks

import assert from 'node:assert/strict';
import { execSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));

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

// The helpers are the real ones: only the data is synthetic. Swapping the array
// and keeping the module means these tests exercise the code that ships.
const swapArray = (text, declaration, body) => {
  const open = text.indexOf(declaration);
  if (open < 0) throw new Error(`cannot find ${declaration}`);
  const close = text.indexOf('\n];\n', open);
  if (close < 0) throw new Error(`cannot find the end of ${declaration}`);
  return text.slice(0, open + declaration.length) + '\n' + body + '\n];' + text.slice(close + 3);
};

const repos = [];

/**
 * A fork with `reviews.length` upstream commits, one per recorded review, each
 * editing the file the registry entry depends on. Everything is merged, so the
 * merge-base is the upstream tip and the old merge-base window would be empty.
 */
function scenario({ entries, reviews, dropEntryAndMarkers = false }) {
  const dir = mkdtempSync(join(tmpdir(), 'ag-e2e-'));
  repos.push(dir);
  const git = (cmd) => execSync(cmd, { cwd: dir, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  const write = (file, body) => {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    writeFileSync(join(dir, file), Array.isArray(body) ? body.join('\n') + '\n' : body);
  };
  const commit = (message) => {
    git('git add -A');
    git(`git commit -q --allow-empty -m "${message}"`);
    return git('git rev-parse HEAD').trim();
  };

  git('git init -q -b main');
  git('git config user.email test@argentum.invalid');
  git('git config user.name Test');
  git('git config commit.gpgsign false');

  write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    let n = s.len();', '}']);
  commit('upstream base');
  git('git branch upstream/main');

  // One upstream commit per review, each touching the dependency.
  git('git checkout -q upstream/main');
  const shas = [git('git rev-parse HEAD').trim()];
  for (let i = 1; i < reviews.length; i += 1) {
    write('src-tauri/src/cache_utils.rs', [
      'fn hash() {', '    let n = s.len();', `    // revision ${i}`, '}',
    ]);
    shas.push(commit(`fix apply_spatial_transformations caching ${i}`));
  }
  git('git checkout -q main');

  // Our side: the borrowed fix, and the scripts under test.
  write('src-tauri/src/cache_utils.rs', [
    'fn hash() {',
    '    // upstream #1307',
    '    s.hash(h);',
    '    // end upstream #1307',
    '}',
  ]);
  mkdirSync(join(dir, 'scripts'), { recursive: true });
  for (const f of [
    'check-mergeability.mjs',
    'upstream-anchors.mjs',
    'upstream-diff.mjs',
    'upstream-overlaps.mjs',
  ]) {
    copyFileSync(join(here, f), join(dir, 'scripts', f));
  }

  const through = (i) => shas[i];
  const registryBody = entries.map((e) => e(through)).join('\n');
  const reviewsBody = reviews.map((r, i) => r(through, i)).join('\n');

  write('scripts/upstream-registry.mjs', swapArray(
    readFileSync(join(here, 'upstream-registry.mjs'), 'utf8'),
    'export const REGISTRY = [', registryBody,
  ));
  write('scripts/upstream-decisions.mjs', swapArray(
    readFileSync(join(here, 'upstream-decisions.mjs'), 'utf8'),
    'export const REVIEWS = [', reviewsBody,
  ));

  const tip = shas[shas.length - 1];
  write('CHANGELOG.md', `**Based on RapidRAW \`0.0.0\` @ \`${tip.slice(0, 8)}\`**\n`);
  commit('argentum: the fork as it stands');

  // Merge everything, which is what used to close the review window.
  git('git merge -q -X ours --no-edit upstream/main');

  if (dropEntryAndMarkers) {
    write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    s.hash(h);', '}']);
    write('scripts/upstream-registry.mjs', swapArray(
      readFileSync(join(dir, 'scripts/upstream-registry.mjs'), 'utf8'),
      'export const REGISTRY = [', unrelatedEntry(),
    ));
    commit('adopt theirs, and tidy the registry while we are here');
  }

  const run = spawnSync(process.execPath, ['scripts/check-mergeability.mjs'], {
    cwd: dir, encoding: 'utf8',
  });
  return { dir, shas, status: run.status, out: (run.stdout ?? '') + (run.stderr ?? '') };
}

// --- the pieces the scenarios are built from ---------------------------------

const borrowEntry = ({ retiredIn = null } = {}) => (through) => `  {
    id: 'borrow-1307',
    kind: 'borrowed-fix',
    what: 'cache key hashes contents, not length',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/cache_utils.rs', pr: '1307', how: 'borrows',
        note: 'Marked between // upstream #1307 and // end upstream #1307.' },
    ],
    tests: ['none'],
    keywords: /nothing-matches-this-subject/,${retiredIn === null ? '' : `
    retired: { recordedIn: '${retiredIn === 'bogus' ? 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeef' : through(retiredIn)}',
      why: 'Upstream merged the fix and our marked block is identical to theirs.' },`}
  },`;

const unrelatedEntry = () => `  {
    id: 'something-else',
    kind: 'feature',
    what: 'claims the file so the coverage gate stays quiet',
    ours: [],
    dependsOn: [{ file: 'src-tauri/src/cache_utils.rs', how: 'extends' }],
    tests: ['none'],
    keywords: /nothing-matches-this-subject/,
  },`;

const review = ({ decisions = [], featureReview = true } = {}) => (through, i) => `  {
    through: '${through(i)}',
    date: '2026-09-13',
    decisions: [
${decisions.map((d) => `      { overlap: '${typeof d.overlap === 'function' ? d.overlap(through) : d.overlap}', verdict: '${d.verdict ?? 'keep-ours'}',
        why: 'Read the commit; it reshuffles their hashing and does not touch the marked block.' },`).join('\n')}
    ],${featureReview ? `
    featureReview: { verdict: 'none',
      why: 'Read every subject in the batch for features Argentum already has, and found none.' },` : ''}
  },`;

/** The dependency overlap key the checker will demand, for upstream commit i. */
const depKeyAt = (i) => (through) =>
  `${through(i).slice(0, 8)}:dep:borrow-1307:src-tauri/src/cache_utils.rs`;

// --- scenarios ---------------------------------------------------------------

test('e2e: a live entry whose dependency upstream touched fails without a decision', () => {
  const { status, out } = scenario({
    entries: [borrowEntry()],
    reviews: [review(), review()],
  });
  assert.equal(status, 1, `expected a failure, got:\n${out}`);
  assert.match(out, /no decision for .*:dep:borrow-1307:/);
});

test('e2e: the same fork passes once the decision is recorded', () => {
  const { status, out } = scenario({
    entries: [borrowEntry()],
    reviews: [review(), review({ decisions: [{ overlap: depKeyAt(1) }] })],
  });
  assert.equal(status, 0, `expected a pass, got:\n${out}`);
});

test('e2e: retiring an entry in the window under review does NOT erase its decision', () => {
  const { status, out } = scenario({
    entries: [borrowEntry({ retiredIn: 1 })],
    reviews: [
      review(),
      review({ decisions: [{ overlap: 'retire:borrow-1307', verdict: 'adopt' }] }),
    ],
  });
  assert.equal(status, 1, `retiring it inside its own window must not silence it:\n${out}`);
  assert.match(out, /no decision for .*:dep:borrow-1307:/);
});

test('e2e: three windows later, a recorded retirement stays retired', () => {
  // The bug: recordedIn was compared against one sha, so the entry went quiet
  // for exactly one window and was live again for every review after it.
  const { status, out } = scenario({
    entries: [borrowEntry({ retiredIn: 1 })],
    reviews: [
      review(),
      review({ decisions: [{ overlap: 'retire:borrow-1307', verdict: 'adopt' }] }),
      review(),
      review(),
    ],
  });
  assert.equal(status, 0, `a retirement two reviews back must not come back to life:\n${out}`);
  assert.doesNotMatch(out, /no decision for/);
});

test('e2e: a retirement naming no real review fails', () => {
  const { status, out } = scenario({
    entries: [borrowEntry({ retiredIn: 'bogus' })],
    reviews: [review(), review()],
  });
  assert.equal(status, 1, `expected a failure, got:\n${out}`);
  assert.match(out, /names no review in REVIEWS/);
});

test('e2e: a retirement with no retire: decision in that review fails', () => {
  const { status, out } = scenario({
    entries: [borrowEntry({ retiredIn: 1 })],
    reviews: [review(), review()],
  });
  assert.equal(status, 1, `expected a failure, got:\n${out}`);
  assert.match(out, /records no retire:borrow-1307 decision/);
});

test('e2e: deleting the entry and its markers together is caught by our own history', () => {
  const { status, out } = scenario({
    entries: [borrowEntry()],
    reviews: [review(), review()],
    dropEntryAndMarkers: true,
  });
  assert.equal(status, 1, `expected a failure, got:\n${out}`);
  assert.match(out, /borrow-1307 was in the inventory and has been deleted from it/);
});

for (const dir of repos) {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch { /* git may still hold a pack file; the temp directory is disposable */ }
}

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exit(failed > 0 ? 1 : 0);
