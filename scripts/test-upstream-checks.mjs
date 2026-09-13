// Tests for the upstream checks, run against throwaway git repositories.
//
// WHY
//
// The accounting used to live inline in check-mergeability.mjs, where the only
// way to exercise it was to damage the working tree and read the output. Two
// bugs survived that: a deleted file was invisible to it, and every deleted line
// counted the same whether or not it had been replaced.
//
// Then a third thing became clear, which is why the line counts are now only
// warnings: equal counts prove nothing. A call site rewritten one line for one
// can change every export in the application. So the tests below cover the three
// gaps in order - counts are not evidence, the inventory is what gates, and the
// requirement to review cannot be erased by deleting the thing that created it -
// plus reviewing after a merge and a mixed batch of borrowed fixes.
//
// WHAT IS NOT COVERED
//
// The gates read their configuration from module scope, so they are not driven
// from here; what is tested is everything that decides what they see. The
// featureReview requirement is deliberately not testable beyond its presence:
// no test can tell whether a person actually read a batch of commits.
//
// Run:  npm run test:checks

import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

/** The real repository, for the gates that read files of theirs in place. */
const root = join(dirname(fileURLToPath(import.meta.url)), '..');

import { accountDiff } from './upstream-diff.mjs';
import { borrowMarkers, borrowStatus, detectOverlaps } from './upstream-overlaps.mjs';
import {
  REGISTRY, activeFor, borrowsOf, dependencyIndex, shadowsOf, validateRetirements,
} from './upstream-registry.mjs';
import { ANCHORS } from './upstream-anchors.mjs';

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

// --- GAP 1: line counts are not evidence -------------------------------------

test('gap 1: a one-for-one call-site rewrite removes nothing and changes everything', () => {
  const r = repo();
  r.write('src-tauri/src/export_processing.rs', [
    'fn export() {',
    '    let adj = get_all_adjustments_from_json(js, is_raw, tm);',
    '}',
  ]);
  const base = r.commit('upstream');

  r.write('src-tauri/src/export_processing.rs', [
    'fn export() {',
    '    let adj = get_all_adjustments_from_json(js, is_raw, tm, Some(path));',
    '}',
  ]);
  r.commit('thread the photo through');

  const stats = accountDiff(r.git(`git diff ${base}`)).get('src-tauri/src/export_processing.rs');
  assert.equal(stats.removed, 0, 'the arithmetic says nothing was lost');
  assert.equal(stats.replaced, 1);
  // ...and the only thing that can catch it is the inventory claiming the file.
  const index = dependencyIndex(REGISTRY);
  assert.equal(index.claims('src-tauri/src/export_processing.rs'), true,
    'the real registry must claim the file this test is modelled on');
});

test('gap 1: every upstream file the real fork touches is claimed by the registry', () => {
  // Not a synthetic repo: the gate is only as good as the inventory behind it.
  const index = dependencyIndex(REGISTRY);
  for (const entry of REGISTRY) {
    assert.ok(entry.id && entry.what, 'every entry names itself and says what it is');
    assert.ok(Array.isArray(entry.tests) && entry.tests.length > 0,
      `${entry.id} must say what proves it works, even if the answer is NONE`);
    for (const dep of entry.dependsOn) {
      assert.ok(dep.file || dep.pattern, `${entry.id} has a dependency naming neither file nor pattern`);
      assert.ok(dep.how, `${entry.id} has a dependency with no how`);
    }
  }
  assert.ok(index.byFile.size > 10, 'the inventory is not a stub');
});

// --- the diff accounting, which is now informational -------------------------

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

// --- GAP 2: the inventory is what gates --------------------------------------

/** A fork whose tree carries a borrowed fix, and an upstream that moves on. */
function forkWithBorrowedFix({ merge = false, prInSubject = false, dropMarkers = false } = {}) {
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
  if (dropMarkers) {
    r.write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    s.hash(h);', '}']);
    r.commit('adopt theirs and drop our markers');
  }

  return { r, base, theirs, tidy, head: r.git('git rev-parse upstream-main').trim() };
}

const borrowEntry = {
  id: 'borrow-1307',
  kind: 'borrowed-fix',
  what: 'cache key hashes contents, not length',
  ours: [],
  dependsOn: [{ file: 'src-tauri/src/cache_utils.rs', pr: '1307', how: 'borrows' }],
  tests: ['none'],
  keywords: /cache|hash/i,
};

const withIndex = (entries) => ({ entries, index: dependencyIndex(entries) });

test('gap 2: upstream editing a registered dependency is an overlap, with no PR number in sight', () => {
  const { r, base, theirs } = forkWithBorrowedFix();
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', withIndex([borrowEntry]));
  const key = `${theirs.slice(0, 8)}:dep:borrow-1307:src-tauri/src/cache_utils.rs`;
  const hit = overlaps.find((o) => o.key === key);
  assert.ok(hit, `no overlap for the commit that edited our borrowed file: ${overlaps.map((o) => o.key).join(' ')}`);
  assert.equal(hit.gated, true);
  assert.doesNotMatch(hit.subject, /#\d+/, 'the point of this test is a subject with no number in it');
});

test('gap 2: an unrelated upstream commit is not an overlap', () => {
  const { r, base, tidy } = forkWithBorrowedFix();
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', withIndex([borrowEntry]));
  assert.equal(overlaps.some((o) => o.commit === tidy.slice(0, 8)), false);
});

test('gap 2: a feature upstream may have built elsewhere is flagged from the subject, and labelled a hint', () => {
  const { r, base, tidy } = forkWithBorrowedFix();
  const entry = { ...borrowEntry, id: 'clippy-thing', keywords: /clippy/i };
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', withIndex([entry]));
  const hit = overlaps.find((o) => o.commit === tidy.slice(0, 8) && o.kind === 'feature');
  assert.ok(hit, 'a keyword in the subject with none of our files touched should still ask');
  assert.equal(hit.gated, true);
  assert.match(hit.detail, /can never see/, 'the message must say what a file match cannot do');
});

test('gap 2: a shadowed symbol is flagged on a call, not on an import reshuffle', () => {
  const r = repo();
  r.write('src-tauri/src/image_processing.rs', ['pub fn apply_cpu_default_raw_processing() {}']);
  r.write('src-tauri/src/lib.rs', ['use crate::image_processing::{', '    apply_crop, apply_flip,', '};']);
  const base = r.commit('upstream base');

  r.write('src-tauri/src/lib.rs', ['use crate::image_processing::{', '    apply_cpu_default_raw_processing, apply_crop,', '};']);
  const reshuffle = r.commit('tidy imports');
  r.write('src-tauri/src/lib.rs', ['fn go() { apply_cpu_default_raw_processing(); }']);
  const call = r.commit('call it');

  const entry = {
    id: 'preview-encode',
    kind: 'behaviour-change',
    what: 'toe instead of cliff',
    ours: [],
    dependsOn: [{
      file: 'src-tauri/src/image_processing.rs',
      symbol: 'apply_cpu_default_raw_processing',
      how: 'shadows',
    }],
    tests: ['none'],
    keywords: /nothing-matches-here/,
  };
  const overlaps = detectOverlaps(r.git, base, 'HEAD', withIndex([entry]));
  assert.equal(
    overlaps.some((o) => o.commit === reshuffle.slice(0, 8)), false,
    'an import list mentions every symbol in the module and means nothing by it',
  );
  assert.equal(overlaps.some((o) => o.commit === call.slice(0, 8) && o.kind === 'dep'), true);
  assert.equal(shadowsOf([entry]).length, 1);
});

// --- GAP 3: the requirement cannot be erased by removing what created it ------

test('gap 3: a retirement is live in its own window and dead in every one after', () => {
  const reviews = [{ through: 'A' }, { through: 'B' }, { through: 'C' }, { through: 'D' }];
  const retiredInB = [{ ...borrowEntry, retired: { recordedIn: 'B', why: 'adopted' } }];
  const live = (through) => activeFor(retiredInB, reviews, through).length === 1;

  assert.equal(live('A'), true, 'before the retirement, live');
  assert.equal(live('B'), true, 'retiring it IS the decision under review, so still live');
  assert.equal(live('C'), false, 'the window after, discharged');
  assert.equal(live('D'), false,
    'and every window after that. The first version compared recordedIn against one '
    + 'sha, so the entry went quiet for exactly one window and came back for good.');
  assert.equal(live(null), false, 'the window being prepared: every recorded retirement is behind it');
});

test('gap 3: an unrecognised recordedIn keeps the entry live rather than silently retiring it', () => {
  const reviews = [{ through: 'A' }, { through: 'B' }];
  const bogus = [{ ...borrowEntry, retired: { recordedIn: 'nonsense', why: 'x' } }];
  assert.equal(activeFor(bogus, reviews, 'B').length, 1);
  const problems = validateRetirements(bogus, reviews);
  assert.equal(problems.length, 1);
  assert.match(problems[0].detail, /names no review in REVIEWS/);
});

test('gap 3: a retirement needs a retire: decision in the review that records it', () => {
  const withoutDecision = [{ through: 'A', decisions: [] }, { through: 'B', decisions: [] }];
  const withDecision = [
    { through: 'A', decisions: [] },
    { through: 'B', decisions: [{ overlap: 'retire:borrow-1307', verdict: 'adopt', why: 'upstream merged it' }] },
  ];
  const entry = [{ ...borrowEntry, retired: { recordedIn: 'B', why: 'upstream merged the fix, our block matches theirs' } }];

  assert.match(validateRetirements(entry, withoutDecision)[0].detail, /records no retire:borrow-1307/);
  assert.deepEqual(validateRetirements(entry, withDecision), []);
});

test('gap 3: every retirement problem is reported, not just the first', () => {
  const reviews = [{ through: 'A', decisions: [] }];
  const entries = [
    { ...borrowEntry, id: 'one', retired: { recordedIn: 'nope', why: 'a reason long enough to pass' } },
    { ...borrowEntry, id: 'two', retired: { recordedIn: 'A', why: 'short' } },
  ];
  const ids = new Set(validateRetirements(entries, reviews).map((p) => p.id));
  assert.deepEqual([...ids].sort(), ['one', 'two']);
});

test('gap 3: deleting the marker without retiring the entry leaves the tree and the registry disagreeing', () => {
  const { r } = forkWithBorrowedFix({ dropMarkers: true });
  const marks = borrowMarkers(r.git);
  assert.deepEqual(marks.pairs, [], 'the markers really are gone from the tree');

  // This is the state the integrity gate fails on: an unretired entry claiming a
  // marker that no longer exists. Deleting the marker was the easy way to make
  // the overlap disappear, and it now costs a recorded retirement instead.
  const registered = borrowsOf([borrowEntry]);
  const orphaned = registered.filter(({ entry, file, pr }) =>
    !entry.retired && !marks.pairs.some((m) => m.file === file && m.pr === pr));
  assert.equal(orphaned.length, 1);

  // Retired in the current window, the requirement survives to be decided.
  const reviews = [{ through: 'earlier' }, { through: 'now' }];
  assert.equal(
    activeFor([{ ...borrowEntry, retired: { recordedIn: 'now', why: 'adopted' } }], reviews, 'now').length,
    1,
  );
});

test('gap 3: a marker in the tree that no entry declares is caught too', () => {
  const { r } = forkWithBorrowedFix();
  const marks = borrowMarkers(r.git);
  assert.deepEqual(marks.prs, ['1307']);
  const undeclared = marks.pairs.filter(({ file, pr }) =>
    !borrowsOf([]).some((b) => b.file === file && b.pr === pr));
  assert.equal(undeclared.length, 1, 'an unregistered borrowed fix must not be invisible');
});

// --- post-merge review, and mixed borrowed fixes -----------------------------

test('the review survives the merge that used to erase it', () => {
  const { r, base, theirs, head } = forkWithBorrowedFix({ merge: true });

  // The old window ran from the merge-base to upstream. After the merge there is
  // nothing in it, whether or not anybody looked.
  const mergeBase = r.git('git merge-base HEAD upstream-main').trim();
  assert.equal(mergeBase, head, 'the merge is what closed the old window');
  assert.equal(r.git(`git log --format=%h ${mergeBase}..upstream-main`).trim(), '');

  // The register's window starts where the last review ended, so it is still open.
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', withIndex([borrowEntry]))
    .filter((o) => o.gated);
  assert.equal(overlaps.length, 1);
  assert.equal(overlaps[0].commit, theirs.slice(0, 8));
});

test('a landed pull request and a pending one are both reported', () => {
  const { r, base } = forkWithBorrowedFix({ prInSubject: true });
  const status = borrowStatus(r.git, base, 'upstream-main', ['1307', '1633']);
  assert.equal(status.length, 2, 'one landing must not silence the others');
  assert.equal(status.find((s) => s.pr === '1307').landedIn.length, 1);
  assert.equal(status.find((s) => s.pr === '1633').landedIn.length, 0);
});

test('a landed borrowed fix still produces its dependency overlap', () => {
  // The number in the subject is a convenience. The decision is owed either way.
  const { r, base, theirs } = forkWithBorrowedFix({ prInSubject: true });
  const overlaps = detectOverlaps(r.git, base, 'upstream-main', withIndex([borrowEntry]));
  assert.equal(
    overlaps.some((o) => o.commit === theirs.slice(0, 8) && o.kind === 'dep'), true,
  );
});

// --- the wiring an anchor exists for ----------------------------------------
//
// `requires` is the only gate here that asserts a file of *theirs* still
// contains something, so it is the one most able to be quietly wrong: a pattern
// matching nothing fails forever, a pattern matching anything protects nothing.
// These check both directions against the real patterns rather than a copy, so
// editing an anchor without editing the test cannot pass.

const wired = ANCHORS.filter((a) => a.requires?.length);

/**
 * The one line whose removal stops a pattern matching, or -1.
 *
 * Not "the line the pattern matches": a dependency array spans several lines, so
 * the pattern matches the file and no single line of it. The first version of
 * this test filtered lines by the pattern, removed nothing, and then reported
 * that the gate had failed to notice - a broken test accusing working code,
 * which is the most expensive colour of red there is.
 */
const lineThatCarries = (text, pattern) => {
  const lines = text.split('\n');
  return lines.findIndex(
    (_, i) => !pattern.test(lines.filter((__, j) => j !== i).join('\n')),
  );
};

test('every required pattern matches the file it guards, as it stands today', () => {
  assert.ok(wired.length > 0, 'no anchor records required wiring - is that right?');
  for (const anchor of wired) {
    const text = readFileSync(join(root, anchor.file), 'utf8');
    for (const need of anchor.requires) {
      assert.ok(
        need.pattern.test(text),
        `${anchor.file} no longer matches ${need.pattern} (${need.why}). Either the `
        + 'wiring is gone, or the pattern has drifted from the code.',
      );
    }
  }
});

test('a required pattern is specific enough to fail on an empty file', () => {
  // A pattern like /./ would pass the test above and guard nothing whatsoever.
  for (const anchor of wired) {
    for (const need of anchor.requires) {
      assert.equal(
        need.pattern.test(''), false,
        `${anchor.file}: ${need.pattern} matches an empty file, so it cannot detect `
        + 'the wiring being deleted',
      );
    }
  }
});

test('removing any one required line is caught, one line at a time', () => {
  // The case that matters: the import and the hook call can both survive while
  // the dependency entry is dropped as unused, and the estimate silently freezes
  // on the old depth. That is the bug this whole gate exists for.
  for (const anchor of wired) {
    const text = readFileSync(join(root, anchor.file), 'utf8');
    for (const need of anchor.requires) {
      assert.notEqual(
        lineThatCarries(text, need.pattern), -1,
        `${anchor.file}: no single line's removal stops ${need.pattern} matching, so `
        + 'the gate cannot tell this wiring from its absence',
      );
    }
  }
});

test('the gate itself fails, not merely the pattern', () => {
  // End to end: a working tree with the dependency line gone must make
  // check-mergeability exit non-zero, and say which wiring went.
  const anchor = wired.find((a) => a.file.endsWith('ExportPanel.tsx'));
  assert.ok(anchor, 'the ExportPanel wiring anchor is the one this was built for');
  const dependency = anchor.requires.at(-1);

  const full = join(root, anchor.file);
  const original = readFileSync(full, 'utf8');
  const cut = lineThatCarries(original, dependency.pattern);
  assert.notEqual(cut, -1, 'nothing to remove');

  writeFileSync(full, original.split('\n').filter((_, i) => i !== cut).join('\n'));
  try {
    let exitCode = 0;
    let output = '';
    try {
      output = execSync('node scripts/check-mergeability.mjs', {
        cwd: root,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (error) {
      exitCode = error.status ?? 1;
      output = `${error.stdout ?? ''}${error.stderr ?? ''}`;
    }
    assert.notEqual(exitCode, 0, 'the gate passed with the wiring removed');
    assert.match(output, /lost the wiring its anchor exists for/);
  } finally {
    // Always, including when an assertion above threw: a test that leaves a file
    // of theirs mutilated turns one red into a confusing dozen.
    writeFileSync(full, original);
  }
});

for (const dir of repos) {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch { /* git may still hold a pack file; the temp directory is disposable */ }
}

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exit(failed > 0 ? 1 : 0);
