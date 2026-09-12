// Tests for the upstream checks, run against throwaway git repositories.
//
// WHY
//
// The accounting used to live inline in check-mergeability.mjs, where the only
// way to exercise it was to damage the working tree and read the output. Two
// bugs survived that: a deleted file was invisible to it, and every deleted line
// counted the same whether or not it had been replaced. Both are covered here.
//
// WHAT IS NOT COVERED
//
// The gates themselves are a handful of lines over these numbers and read their
// configuration from module scope, so they are not driven from here. What is
// tested is everything that decides what the numbers are, which is where the
// bugs were.
//
// Run:  npm run test:checks

import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

import { accountDiff } from './upstream-diff.mjs';
import { borrowMarkers, borrowStatus, detectOverlaps } from './upstream-overlaps.mjs';

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

const repos = [];
function repo() {
  const dir = mkdtempSync(join(tmpdir(), 'ag-upstream-'));
  repos.push(dir);
  const git = (cmd, big = false) =>
    execSync(cmd, { cwd: dir, encoding: 'utf8', maxBuffer: (big ? 96 : 8) * 1024 * 1024 });
  git('git init -q -b main');
  git('git config user.email test@argentum.invalid');
  git('git config user.name Test');
  git('git config commit.gpgsign false');
  const write = (file, body) => {
    const full = join(dir, file);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, body.join('\n') + '\n');
  };
  const remove = (file) => unlinkSync(join(dir, file));
  const commit = (message) => {
    git('git add -A');
    git(`git commit -q --allow-empty -m "${message}"`);
    return git('git rev-parse HEAD').trim();
  };
  return { dir, git, write, remove, commit };
}

const lines = (n, text) => Array.from({ length: n }, (_, i) => `${text} ${i}`);

// --- the diff accounting -----------------------------------------------------

test('a deleted file is counted against the file that was deleted', () => {
  const r = repo();
  r.write('src/components/Basic.tsx', lines(30, 'const basic'));
  r.write('src/components/Color.tsx', lines(4, 'const color'));
  const base = r.commit('upstream');

  r.remove('src/components/Basic.tsx');
  r.commit('delete a whole upstream component');

  const stats = accountDiff(r.git(`git diff ${base}`));
  const basic = stats.get('src/components/Basic.tsx');
  assert.ok(basic, 'the deleted file is missing from the accounting entirely');
  assert.equal(basic.removed, 30);
  // The bug this replaces charged those deletions to whichever file happened to
  // come before it in the diff, or dropped them when that file was one of ours.
  assert.equal(stats.get('src/components/Color.tsx')?.removed ?? 0, 0);
});

test('a deleted file next to one of ours is not swallowed by the skip', () => {
  const r = repo();
  r.write('src/argentum/ours.ts', lines(3, 'const ours'));
  r.write('src/components/Basic.tsx', lines(12, 'const basic'));
  const base = r.commit('upstream');

  r.write('src/argentum/ours.ts', lines(5, 'const ours'));
  r.remove('src/components/Basic.tsx');
  r.commit('ours grows, theirs goes');

  const isOurs = (f) => f.startsWith('src/argentum/');
  const stats = accountDiff(r.git(`git diff ${base}`), { isOurs });
  assert.equal(stats.get('src/components/Basic.tsx').removed, 12);
});

test('a line replaced in place is not a removal', () => {
  const r = repo();
  r.write('src/file_management.rs', ['let a = ".rrdata";', 'let b = ".rrdata";', 'keep me']);
  const base = r.commit('upstream');

  r.write('src/file_management.rs', ['let a = ".agdata";', 'let b = ".agdata";', 'keep me']);
  r.commit('rename the sidecar');

  const stats = accountDiff(r.git(`git diff ${base}`)).get('src/file_management.rs');
  assert.equal(stats.deleted, 2, 'git deleted two lines');
  assert.equal(stats.replaced, 2);
  assert.equal(stats.removed, 0, 'a rename leaves nothing of theirs missing');
});

test('a block answered by one call line is mostly removal', () => {
  const r = repo();
  r.write('src/shader.wgsl', ['header', ...lines(11, '    clip'), 'footer']);
  const base = r.commit('upstream');

  r.write('src/shader.wgsl', ['header', '    ag_stage_display(x);', 'footer']);
  r.commit('route clipping through our stage');

  const stats = accountDiff(r.git(`git diff ${base}`)).get('src/shader.wgsl');
  assert.equal(stats.replaced, 1);
  assert.equal(stats.removed, 10);
});

test('lines removed while pasting in a borrowed fix are not our divergence', () => {
  const r = repo();
  r.write('src/cache_utils.rs', [
    'fn hash() {',
    '    let n = s.len();',
    '    let m = t.len();',
    '    n.hash(h);',
    '    m.hash(h);',
    '}',
  ]);
  const base = r.commit('upstream');

  r.write('src/cache_utils.rs', [
    'fn hash() {',
    '    // upstream #1307',
    '    s.hash(h);',
    '    // end upstream #1307',
    '}',
  ]);
  r.commit('borrow their fix');

  const stats = accountDiff(r.git(`git diff ${base}`)).get('src/cache_utils.rs');
  assert.equal(stats.removed, 0, 'their fix replacing their code is not our removal');
  assert.equal(stats.borrowedRemoved, 1);
  assert.deepEqual([...stats.prs], ['1307']);
});

// --- overlap detection -------------------------------------------------------

/** A fork whose tree carries a borrowed fix, and an upstream that moves on. */
function forkWithBorrowedFix({ merge = false, prInSubject = false } = {}) {
  const r = repo();
  r.write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    let n = s.len();', '}']);
  r.write('src-tauri/src/unrelated.rs', ['fn other() {}']);
  const base = r.commit('upstream base');

  r.git('git branch upstream-main');
  r.write('src-tauri/src/cache_utils.rs', [
    'fn hash() {',
    '    // upstream #1307',
    '    s.hash(h);',
    '    // end upstream #1307',
    '}',
  ]);
  r.commit('borrow upstream #1307');

  r.git('git checkout -q upstream-main');
  r.write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    let n = s.len();', '    // reshuffled', '}']);
  const theirs = r.commit(prInSubject
    ? 'Merge pull request #1307 from someone/fix'
    : 'fix apply_spatial_transformations caching');
  r.write('src-tauri/src/unrelated.rs', ['fn other() { /* tidy */ }']);
  const tidy = r.commit('fix clippy warnings');

  r.git('git checkout -q main');
  if (merge) r.git('git merge -q -X ours --no-edit upstream-main');

  return { r, base, theirs, tidy, head: r.git('git rev-parse upstream-main').trim() };
}

const bare = { shadowed: [], sidecarKeys: [], sensitive: [] };

test('upstream editing a file we borrowed into is an overlap, with no PR number in sight', () => {
  const { r, base, theirs } = forkWithBorrowedFix();
  const marks = borrowMarkers(r.git);
  assert.deepEqual(marks.files, ['src-tauri/src/cache_utils.rs']);

  const overlaps = detectOverlaps(r.git, base, 'upstream-main', { ...bare, borrow: marks });
  const key = `${theirs.slice(0, 8)}:borrow:src-tauri/src/cache_utils.rs`;
  const hit = overlaps.find((o) => o.key === key);
  assert.ok(hit, `no overlap for the commit that edited our borrowed file: ${overlaps.map((o) => o.key).join(' ')}`);
  assert.equal(hit.gated, true);
  assert.doesNotMatch(hit.subject, /#\d+/, 'the point of this test is a subject with no number in it');
});

test('an unrelated upstream commit is not an overlap', () => {
  const { r, base, tidy } = forkWithBorrowedFix();
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', { ...bare, borrow: borrowMarkers(r.git) });
  assert.equal(overlaps.some((o) => o.commit === tidy.slice(0, 8)), false);
});

test('a landed pull request and a pending one are both reported', () => {
  const { r, base } = forkWithBorrowedFix({ prInSubject: true });
  const status = borrowStatus(r.git, base, 'upstream-main', ['1307', '1633']);
  assert.equal(status.length, 2, 'one landing must not silence the others');
  assert.equal(status.find((s) => s.pr === '1307').landedIn.length, 1);
  assert.equal(status.find((s) => s.pr === '1633').landedIn.length, 0);
});

test('the review survives the merge that used to erase it', () => {
  const { r, base, theirs, head } = forkWithBorrowedFix({ merge: true });

  // The old window ran from the merge-base to upstream. After the merge there is
  // nothing in it, whether or not anybody looked.
  const mergeBase = r.git('git merge-base HEAD upstream-main').trim();
  assert.equal(mergeBase, head, 'the merge is what closed the old window');
  assert.equal(r.git(`git log --format=%h ${mergeBase}..upstream-main`).trim(), '');

  // The register's window starts where the last review ended, so it is still open.
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', { ...bare, borrow: borrowMarkers(r.git) })
    .filter((o) => o.gated);
  assert.equal(overlaps.length, 1);
  assert.equal(overlaps[0].commit, theirs.slice(0, 8));
});

test('a shadowed symbol is flagged on a call, not on an import reshuffle', () => {
  const r = repo();
  r.write('src-tauri/src/image_processing.rs', ['pub fn apply_cpu_default_raw_processing() {}']);
  r.write('src-tauri/src/lib.rs', ['use crate::image_processing::{', '    apply_crop, apply_flip,', '};']);
  const base = r.commit('upstream base');

  r.write('src-tauri/src/lib.rs', ['use crate::image_processing::{', '    apply_cpu_default_raw_processing, apply_crop,', '};']);
  const reshuffle = r.commit('tidy imports');
  r.write('src-tauri/src/lib.rs', ['fn go() { apply_cpu_default_raw_processing(); }']);
  const call = r.commit('call it');

  const shadowed = [{
    file: 'src-tauri/src/image_processing.rs',
    symbol: 'apply_cpu_default_raw_processing',
    instead: 'mods::preview_encode',
  }];
  const overlaps = detectOverlaps(r.git, base, 'HEAD', {
    shadowed, sidecarKeys: [], sensitive: [], borrow: { files: [], prs: [] },
  });
  assert.equal(
    overlaps.some((o) => o.commit === reshuffle.slice(0, 8)), false,
    'an import list mentions every symbol in the module and means nothing by it',
  );
  assert.equal(overlaps.some((o) => o.commit === call.slice(0, 8) && o.kind === 'shadow'), true);
});

for (const dir of repos) {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch { /* git may still hold a pack file; the temp directory is disposable */ }
}

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exit(failed > 0 ? 1 : 0);
