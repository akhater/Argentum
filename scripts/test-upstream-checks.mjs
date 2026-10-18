// Tests for the upstream checks, run against throwaway git repositories.
//
// WHY
//
// The accounting used to live inline in check-mergeability.mjs, where the only
// way to exercise it was to damage the working tree and read the output. Two
// bugs survived that: a deleted file was invisible to it, and every deleted line
// counted the same whether or not it had been replaced.
//
// Then a third thing became clear, which is why the line counts are only
// information: equal counts prove nothing. A call site rewritten one line for
// one can change every export in the application. So these cover the pieces the
// gates are built from - the diff accounting, the streamed git readers, the
// inventory and its history, the release-review rules, the ratchet - and
// test-upstream-e2e.mjs runs the real checker on throwaway forks.
//
// WHAT IS NOT COVERED
//
// sameAsOurs is deliberately not testable beyond its presence and its agreement
// with the verdicts: no test can tell whether a person actually read a release.
//
// Run:  npm run test:checks

import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

/** The real repository, for the gates that read files of theirs in place. */
const root = join(dirname(fileURLToPath(import.meta.url)), '..');

import { ANCHORS } from './upstream-anchors.mjs';
import { borrowMarkers, borrowStatus } from './upstream-borrows.mjs';
import { PER_COMMIT_ENDS, REVIEWS, isRelease } from './upstream-decisions.mjs';
import { accountDiff } from './upstream-diff.mjs';
import { commitsIn, gitIn, readBlobs, tags } from './upstream-git.mjs';
import { BASELINE, compareRatchet, formatBaseline, parseBaseline } from './upstream-ratchet.mjs';
import {
  REGISTRY, activeFor, borrowsOf, dependencyIndex, historicalIds, isInterface, isOurs,
  isOursToChange, validateRetirements,
} from './upstream-registry.mjs';
import { featureGroups, validateReviews } from './upstream-releases.mjs';

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
  const git = gitIn(dir);
  git(['init', '-q', '-b', 'main']);
  git(['config', 'user.email', 'test@argentum.invalid']);
  git(['config', 'user.name', 'Test']);
  git(['config', 'commit.gpgsign', 'false']);
  git(['config', 'core.autocrlf', 'false']);
  const write = (file, body) => {
    const full = join(dir, file);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, body.join('\n') + '\n');
  };
  const remove = (file) => unlinkSync(join(dir, file));
  const commit = (message) => {
    git(['add', '-A']);
    git(['commit', '-q', '--allow-empty', '-m', message]);
    return git(['rev-parse', 'HEAD']).trim();
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

  const stats = accountDiff(r.git(['diff', base])).get('src-tauri/src/export_processing.rs');
  assert.equal(stats.removed, 0, 'the arithmetic says nothing was lost');
  assert.equal(stats.replaced, 1);
  // ...and the only thing that can catch it is the inventory claiming the file.
  assert.equal(dependencyIndex(REGISTRY).claims('src-tauri/src/export_processing.rs'), true,
    'the real registry must claim the file this test is modelled on');
});

test('gap 1: every entry of the real registry names itself, its dependencies and its proof', () => {
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

// --- the diff accounting ---------------------------------------------------------

test('a deleted file is counted against the file that was deleted', () => {
  const r = repo();
  r.write('src/components/Basic.tsx', lines(30, 'const basic'));
  r.write('src/components/Color.tsx', lines(4, 'const color'));
  const base = r.commit('upstream');

  r.remove('src/components/Basic.tsx');
  r.commit('delete a whole upstream component');

  const stats = accountDiff(r.git(['diff', base]));
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

  const stats = accountDiff(r.git(['diff', base]), { isOurs });
  assert.equal(stats.get('src/components/Basic.tsx').removed, 12);
});

test('a line replaced in place is not a removal', () => {
  const r = repo();
  r.write('src/file_management.rs', ['let a = ".rrdata";', 'let b = ".rrdata";', 'keep me']);
  const base = r.commit('upstream');

  r.write('src/file_management.rs', ['let a = ".agdata";', 'let b = ".agdata";', 'keep me']);
  r.commit('rename the sidecar');

  const stats = accountDiff(r.git(['diff', base])).get('src/file_management.rs');
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

  const stats = accountDiff(r.git(['diff', base])).get('src/shader.wgsl');
  assert.equal(stats.replaced, 1);
  assert.equal(stats.removed, 10);
});

test('lines removed while pasting in a borrowed fix are not our divergence', () => {
  const r = repo();
  r.write('src/cache_utils.rs', [
    'fn hash() {', '    let n = s.len();', '    let m = t.len();', '    n.hash(h);', '    m.hash(h);', '}',
  ]);
  const base = r.commit('upstream');

  r.write('src/cache_utils.rs', [
    'fn hash() {', '    // upstream #1307', '    s.hash(h);', '    // end upstream #1307', '}',
  ]);
  r.commit('borrow their fix');

  const stats = accountDiff(r.git(['diff', base])).get('src/cache_utils.rs');
  assert.equal(stats.removed, 0, 'their fix replacing their code is not our removal');
  assert.equal(stats.borrowedRemoved, 1);
  assert.deepEqual([...stats.prs], ['1307']);
});

// --- one git process per question ------------------------------------------------

test('git: readBlobs reads many files in one process, multi-byte text intact, missing ones null', () => {
  const r = repo();
  r.write('a.txt', ['café — 1']);
  r.write('b.txt', ['plain']);
  r.commit('two files');
  const blobs = readBlobs(r.git, ['HEAD:a.txt', 'HEAD:nope.txt', 'HEAD:b.txt']);
  assert.equal(blobs.get('HEAD:a.txt'), 'café — 1\n', 'sizes are bytes; decoding must not drift');
  assert.equal(blobs.get('HEAD:nope.txt'), null);
  assert.equal(blobs.get('HEAD:b.txt'), 'plain\n', 'the blob after a missing one is still read');
});

test('git: commitsIn gives parents, subject, body and files, oldest first, merges without files', () => {
  const r = repo();
  r.write('a.txt', ['a']);
  const base = r.commit('base');
  r.git(['checkout', '-q', '-b', 'pr']);
  r.write('b.txt', ['b']);
  r.commit('feat: b');
  r.git(['checkout', '-q', 'main']);
  r.write('c.txt', ['c']);
  r.commit('direct c');
  r.git(['merge', '-q', '--no-ff', 'pr', '-m', 'Merge pull request #12 from x/pr', '-m', 'Add the b feature']);
  const commits = commitsIn(r.git, base, 'HEAD');
  assert.equal(commits.length, 3);
  const merge = commits.find((c) => c.parents.length === 2);
  assert.ok(merge, 'the merge is listed');
  assert.deepEqual(merge.files, []);
  assert.equal(merge.body, 'Add the b feature');
  assert.deepEqual(commits.find((c) => c.subject === 'feat: b').files, ['b.txt']);
});

test('git: tags peels an annotated tag to its commit', () => {
  const r = repo();
  r.write('a.txt', ['a']);
  const sha = r.commit('base');
  r.git(['tag', '-a', 'v1.0.0', '-m', 'release']);
  r.git(['tag', 'light']);
  const all = tags(r.git);
  assert.equal(all.get('v1.0.0'), sha);
  assert.equal(all.get('light'), sha);
});

// --- the inventory's history ---------------------------------------------------------

const registryText = (ids) => [
  'export const REGISTRY = [',
  ...ids.flatMap((id) => ['  {', `    id: '${id}',`, '  },']),
  '];',
];

test('history: an id deleted from the registry is still known to git', () => {
  const r = repo();
  r.write('scripts/upstream-registry.mjs', registryText(['one', 'two']));
  r.commit('two entries');
  r.write('scripts/upstream-registry.mjs', registryText(['one']));
  r.commit('delete one');
  assert.deepEqual([...historicalIds(r.git)].sort(), ['one', 'two']);
});

test('history: an id that only ever existed inside a merge is found', () => {
  // Two entries of the real registry arrived this way; reading only ordinary
  // commits' patches missed both.
  const r = repo();
  r.write('scripts/upstream-registry.mjs', registryText(['one']));
  r.commit('one entry');
  r.git(['checkout', '-q', '-b', 'side']);
  r.write('side.txt', ['x']);
  r.commit('side work');
  r.git(['checkout', '-q', 'main']);
  r.write('main.txt', ['y']);
  r.commit('main work');
  r.git(['merge', '-q', '--no-commit', '--no-ff', 'side']);
  r.write('scripts/upstream-registry.mjs', registryText(['one', 'merged-in']));
  r.commit('merge side, adding an entry while resolving');
  r.write('scripts/upstream-registry.mjs', registryText(['one']));
  r.commit('and delete it again');
  assert.ok(historicalIds(r.git).has('merged-in'));
});

// --- the inventory gates --------------------------------------------------------------

const borrowEntry = {
  id: 'borrow-1307',
  kind: 'borrowed-fix',
  what: 'cache key hashes contents, not length',
  ours: [],
  dependsOn: [{ file: 'src-tauri/src/cache_utils.rs', pr: '1307', how: 'borrows' }],
  tests: ['none'],
  keywords: /cache|hash/i,
};

function forkWithBorrowedFix({ dropMarkers = false } = {}) {
  const r = repo();
  r.write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    let n = s.len();', '}']);
  r.commit('upstream base');
  r.write('src-tauri/src/cache_utils.rs', [
    'fn hash() {', '    // upstream #1307', '    s.hash(h);', '    // end upstream #1307', '}',
  ]);
  r.commit('borrow upstream #1307');
  if (dropMarkers) {
    r.write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    s.hash(h);', '}']);
    r.commit('adopt theirs and drop our markers');
  }
  return r;
}

test('integrity: a marker in the tree that no entry declares is found', () => {
  const marks = borrowMarkers(forkWithBorrowedFix().git);
  assert.deepEqual(marks.prs, ['1307']);
  const undeclared = marks.pairs.filter(({ file, pr }) =>
    !borrowsOf([]).some((b) => b.file === file && b.pr === pr));
  assert.equal(undeclared.length, 1, 'an unregistered borrowed fix must not be invisible');
});

test('integrity: deleting the marker without retiring the entry leaves the two disagreeing', () => {
  const marks = borrowMarkers(forkWithBorrowedFix({ dropMarkers: true }).git);
  assert.deepEqual(marks.pairs, [], 'the markers really are gone from the tree');
  const orphaned = borrowsOf([borrowEntry]).filter(({ entry, file, pr }) =>
    !entry.retired && !marks.pairs.some((m) => m.file === file && m.pr === pr));
  assert.equal(orphaned.length, 1);
});

test('integrity: a landed pull request and a pending one are both reported', () => {
  const commits = [
    { sha: 'a'.repeat(40), subject: 'Merge pull request #1307 from someone/fix', body: '' },
    { sha: 'b'.repeat(40), subject: 'tidy', body: 'mentions #13070, which is another number' },
  ];
  const status = borrowStatus(commits, ['1307', '1633']);
  assert.equal(status.length, 2, 'one landing must not silence the others');
  assert.deepEqual(status.find((s) => s.pr === '1307').landedIn, ['aaaaaaaa']);
  assert.deepEqual(status.find((s) => s.pr === '1633').landedIn, []);
});

test('retirement: live in its own window and dead in every one after', () => {
  const reviews = [{ through: 'A' }, { through: 'B' }, { release: 'v2', to: 'C' }, { release: 'v3', to: 'D' }];
  const live = (recordedIn, through) =>
    activeFor([{ ...borrowEntry, retired: { recordedIn, why: 'adopted' } }], reviews, through).length === 1;
  assert.equal(live('B', 'A'), true, 'before the retirement, live');
  assert.equal(live('B', 'B'), true, 'retiring it IS the decision under review, so still live');
  assert.equal(live('B', 'v2'), false, 'the window after, discharged');
  assert.equal(live('B', 'v3'), false,
    'and every window after that. The first version compared recordedIn against one '
    + 'sha, so the entry went quiet for exactly one window and came back for good.');
  assert.equal(live('v2', null), false, 'a release review retires by its release name');
});

test('retirement: an unrecognised recordedIn keeps the entry live rather than silently retiring it', () => {
  const reviews = [{ through: 'A' }, { release: 'v2', to: 'B' }];
  const bogus = [{ ...borrowEntry, retired: { recordedIn: 'nonsense', why: 'x' } }];
  assert.equal(activeFor(bogus, reviews, 'v2').length, 1);
  assert.match(validateRetirements(bogus, reviews)[0].detail, /names no review in REVIEWS/);
});

test('retirement: a per-commit review records it as a retire: decision', () => {
  const entry = [{ ...borrowEntry, retired: { recordedIn: 'B', why: 'upstream merged the fix, our block matches theirs' } }];
  assert.match(validateRetirements(entry, [{ through: 'B', decisions: [] }])[0].detail,
    /records no retire:borrow-1307/);
  assert.deepEqual(validateRetirements(entry, [{
    through: 'B',
    decisions: [{ overlap: 'retire:borrow-1307', verdict: 'adopt', why: 'upstream merged it' }],
  }]), []);
});

test('retirement: a release review records it in retires, and names nothing it did not retire', () => {
  const entry = { ...borrowEntry, retired: { recordedIn: 'v2', why: 'upstream merged the fix, our block matches theirs' } };
  assert.match(validateRetirements([entry], [{ release: 'v2' }])[0].detail,
    /does not list it in retires/);
  assert.deepEqual(validateRetirements([entry], [{ release: 'v2', retires: ['borrow-1307'] }]), []);
  assert.match(validateRetirements([borrowEntry], [{ release: 'v2', retires: ['borrow-1307'] }])[0].detail,
    /the registry does not retire it there/);
});

test('retirement: every problem is reported, not just the first', () => {
  const reviews = [{ through: 'A', decisions: [] }];
  const entries = [
    { ...borrowEntry, id: 'one', retired: { recordedIn: 'nope', why: 'a reason long enough to pass' } },
    { ...borrowEntry, id: 'two', retired: { recordedIn: 'A', why: 'short' } },
  ];
  const ids = new Set(validateRetirements(entries, reviews).map((p) => p.id));
  assert.deepEqual([...ids].sort(), ['one', 'two']);
});

// --- the release review: grouping --------------------------------------------------------

test('features: one group per pull request merged into their main line, one per direct commit', () => {
  const r = repo();
  r.write('a.txt', ['a']);
  const base = r.commit('base');
  r.write('a.txt', ['a2']);
  r.commit('direct one');
  r.git(['checkout', '-q', '-b', 'pr']);
  r.write('b.txt', ['b']);
  r.commit('feat: b, part 1');
  r.write('b.txt', ['b2']);
  r.commit('feat: b, part 2');
  r.git(['checkout', '-q', 'main']);
  r.git(['merge', '-q', '--no-ff', 'pr', '-m', 'Merge pull request #12 from x/pr', '-m', 'Add the b feature']);
  r.write('c.txt', ['c']);
  const tip = r.commit('direct two');
  const groups = featureGroups(commitsIn(r.git, base, tip), tip);
  assert.deepEqual(groups.map((g) => [g.what, g.commits.map((c) => c.subject)]), [
    ['direct one', ['direct one']],
    ['Add the b feature (#12)', ['feat: b, part 1', 'feat: b, part 2']],
    ['direct two', ['direct two']],
  ]);
});

// --- the release review: the rules ---------------------------------------------------------

const sha = (c) => c.repeat(40);
const SEED = { through: sha('0'), date: '2026-09-13', decisions: [] };
const COMMITS = [
  { sha: sha('1'), parents: [sha('0')], subject: 'sparkle', body: '', files: ['src/components/Thing.tsx'] },
  { sha: sha('2'), parents: [sha('1')], subject: 'faster preview', body: '', files: ['src-tauri/src/lib.rs', 'src/components/Thing.tsx'] },
];
const LATER = [
  { sha: sha('3'), parents: [sha('2')], subject: 'tidy', body: '', files: ['src/components/Thing.tsx'] },
];
const laterRelease = (over = {}) => goodRelease({
  release: 'v1.2.0',
  from: sha('2'),
  to: sha('3'),
  features: [{ what: 'tidy', commits: ['33333333'], verdict: 'take', why: 'A tidy-up of their own file; taken as it is.' }],
  ...over,
});
const goodRelease = (over = {}) => ({
  release: 'v1.1.0',
  from: sha('0'),
  to: sha('2'),
  on: '2026-10-18',
  hours: 2,
  features: [
    { what: 'sparkle', commits: ['11111111', '22222222'], verdict: 'take', why: 'Wanted as it is; merged unchanged.' },
    { what: 'faster preview, in the render path', commits: ['22222222'], verdict: 'copy',
      why: 'The idea is good; copied by hand into our render code.', copiedInto: 'src-tauri/src/lib.rs' },
  ],
  sameAsOurs: { verdict: 'none', why: 'Read both commits for features Argentum already has, and found none.' },
  stillLive: ['src/components/Thing.tsx'],
  ...over,
});
const factsFor = ({ commits = COMMITS, tagged = { 'v1.1.0': sha('2'), 'v1.2.0': sha('3') }, files = ['src/components/Thing.tsx', 'src-tauri/src/lib.rs'] } = {}) => ({
  tags: new Map(Object.entries(tagged)),
  commits,
  exists: (p) => files.includes(p),
  isOurs,
  isOursToChange,
});
const check = (reviews, facts = factsFor()) => {
  // The rules read PER_COMMIT_ENDS from the register; these reviews start from it.
  const seeded = reviews.map((r) => (r.through === sha('0') ? { ...r, through: PER_COMMIT_ENDS } : r))
    .map((r) => (r.from === sha('0') ? { ...r, from: PER_COMMIT_ENDS } : r));
  return validateReviews(seeded, facts).errors.map((e) => e.detail).join('\n');
};

test('release rules: a complete review passes', () => {
  assert.equal(check([SEED, goodRelease()]), '');
});

for (const [name, reviews, pattern, facts] of [
  ['a commit in no feature', [SEED, goodRelease({ features: [goodRelease().features[1]] })], /11111111 "sparkle" is in no feature/],
  ['a commit outside the range', [SEED, goodRelease({ features: [...goodRelease().features, { what: 'stray', commits: ['99999999'], verdict: 'skip', why: 'Not wanted in Argentum at all.' }] })], /names 99999999, which is not a commit in/],
  ['a render-path change only ever taken', [SEED, goodRelease({ features: [goodRelease().features[0]] })], /22222222 changes src-tauri\/src\/lib\.rs, and every feature it is in is take/],
  ['an interface change only ever taken', [SEED, goodRelease({ features: [{ what: 'a new panel', commits: ['44444444'], verdict: 'take', why: 'Their panel is better than ours; take it.' }], stillLive: [] })], /44444444 changes src\/components\/panel\/right\/NewPanel\.tsx, and every feature it is in is take/, factsFor({ commits: [{ sha: sha('4'), parents: [sha('0')], subject: 'a new panel', body: '', files: ['src/components/panel/right/NewPanel.tsx'] }] })],
  ['a verdict outside the four', [SEED, goodRelease({ features: [{ ...goodRelease().features[0], verdict: 'adopt' }, goodRelease().features[1]] })], /has verdict "adopt"/],
  ['a verdict with no reason', [SEED, goodRelease({ features: [{ ...goodRelease().features[0], why: 'ok' }, goodRelease().features[1]] })], /has no reasoning/],
  ['a copy that says not where', [SEED, goodRelease({ features: [goodRelease().features[0], { ...goodRelease().features[1], copiedInto: undefined }] })], /is copied, and copiedInto does not say where/],
  ['a copy into a file of theirs', [SEED, goodRelease({ features: [goodRelease().features[0], { ...goodRelease().features[1], copiedInto: 'src/components/Thing.tsx' }] })], /which is not a file of ours/],
  ['sameAsOurs found and nothing decided', [SEED, goodRelease({ sameAsOurs: { verdict: 'found', why: 'They built our sparkle, and the two need reconciling by hand.' } })], /sameAsOurs is found, and no feature is decide/],
  ['something decided and sameAsOurs none', [SEED, goodRelease({ features: [{ ...goodRelease().features[0], verdict: 'decide' }, goodRelease().features[1]] })], /a feature is decide, and sameAsOurs says none/],
  ['no sameAsOurs', [SEED, goodRelease({ sameAsOurs: undefined })], /sameAsOurs must be/],
  ['stillLive naming a file of ours', [SEED, goodRelease({ stillLive: ['src/argentum/Thing.tsx'] })], /stillLive lists src\/argentum\/Thing\.tsx, which is ours/],
  ['stillLive naming a file that is gone', [SEED, goodRelease({ stillLive: ['src/components/Gone.tsx'] })], /which is not in the tree/],
  ['stillLive going up', [SEED, goodRelease(), laterRelease({ stillLive: ['src/components/Thing.tsx', 'src-tauri/src/lib.rs'] })], /2 of their files still live, up from 1 at v1\.1\.0/, factsFor({ commits: LATER })],
  ['a gap in the chain', [SEED, goodRelease(), laterRelease({ from: sha('1') })], /v1\.2\.0: starts at 11111111, and the review before it ended at 22222222/, factsFor({ commits: LATER })],
  ['a release reviewed twice', [SEED, goodRelease(), goodRelease({ from: sha('2') })], /v1\.1\.0 is reviewed twice/, factsFor({ commits: [] })],
  ['a tag that names another commit', [SEED, goodRelease()], /v1\.1\.0: names 33333333, and the review says to: 22222222/, factsFor({ tagged: { 'v1.1.0': sha('3') } })],
  ['a tag that is not here', [SEED, goodRelease()], /v1\.1\.0: no tag of that name here/, factsFor({ tagged: {} })],
  ['no hours', [SEED, goodRelease({ hours: 0 })], /hours must say how long the review took/],
  ['a per-commit review after the last one', [SEED, { through: sha('5'), date: '2026-10-19', decisions: [], featureReview: { verdict: 'none', why: 'x'.repeat(50) } }], /a per-commit review \(55555555\) after the last one/],
  ['a recorded per-commit decision changed', [{ ...SEED, decisions: [{ overlap: 'abc:dep:x', verdict: 'keep', why: 'Kept ours because it is better.' }] }], /the recorded decision abc:dep:x has no valid verdict/],
]) {
  test(`release rules: ${name} fails`, () => {
    assert.match(check(reviews, facts), pattern);
  });
}

test('release rules: stillLive level from one review to the next passes', () => {
  assert.equal(check([SEED, goodRelease(), laterRelease()], factsFor({ commits: LATER })), '');
});

test('release rules: the real register passes them', () => {
  const git = gitIn(root);
  const newest = REVIEWS[REVIEWS.length - 1];
  let commits = null;
  try {
    if (isRelease(newest)) commits = commitsIn(git, newest.from, newest.to);
    const { errors } = validateReviews(REVIEWS, {
      tags: tags(git), commits, exists: (p) => existsSync(join(root, p)), isOurs, isOursToChange,
    });
    assert.deepEqual(errors, []);
  } catch (error) {
    if (error.name === 'GitError') return; // no upstream history here: check:merge says so itself
    throw error;
  }
});

// --- the ratchet -------------------------------------------------------------------------------

test('ratchet: up fails, down is lowered, a new file counts from zero', () => {
  const baseline = new Map([['a.rs', 5], ['b.rs', 3]]);
  const current = new Map([['a.rs', 6], ['b.rs', 1], ['c.rs', 2]]);
  const { rises, lowered, raised } = compareRatchet(current, baseline, null);
  assert.deepEqual(rises, [{ file: 'a.rs', now: 6, allowed: 5 }, { file: 'c.rs', now: 2, allowed: 0 }]);
  assert.deepEqual(lowered, [{ file: 'b.rs', from: 3, to: 1 }]);
  assert.deepEqual(raised, [], 'with nothing on main to compare with, there is nothing raised');
});

test('ratchet: a number higher than on main is raised, a file new to it too', () => {
  const previous = new Map([['a.rs', 5]]);
  const { raised } = compareRatchet(new Map(), new Map([['a.rs', 7], ['b.rs', 1]]), previous);
  assert.deepEqual(raised, [{ file: 'a.rs', from: 5, to: 7 }, { file: 'b.rs', from: 0, to: 1 }]);
});

test('ratchet: the baseline text is sorted, drops zeros, and reads back', () => {
  const text = formatBaseline(new Map([['z.rs', 1], ['a.rs', 2], ['gone.rs', 0]]));
  assert.deepEqual(Object.keys(JSON.parse(text).files), ['a.rs', 'z.rs']);
  assert.deepEqual([...parseBaseline(text)], [['a.rs', 2], ['z.rs', 1]]);
  assert.equal(parseBaseline(null), null);
});

test('ratchet: the committed baseline holds positive whole numbers for their files only', () => {
  const baseline = parseBaseline(readFileSync(join(root, BASELINE), 'utf8'));
  assert.ok(baseline.size > 10, 'the baseline is not a stub');
  for (const [file, n] of baseline) {
    assert.ok(Number.isInteger(n) && n > 0, `${file}: ${n}`);
    assert.ok(!isOurs(file), `${file} is ours and has no number`);
    assert.ok(!isOursToChange(file), `${file} is on the render path or the interface and has no number`);
  }
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
 * the pattern matches the file and no single line of it.
 */
const lineThatCarries = (text, pattern) => {
  const all = text.split('\n');
  return all.findIndex(
    (_, i) => !pattern.test(all.filter((__, j) => j !== i).join('\n')),
  );
};

test('the interface is today\'s app shell, editor and panels, and nothing beside them', () => {
  for (const f of ['src/App.tsx', 'src/components/panel/Editor.tsx', 'src/components/panel/right/ExportPanel.tsx',
    'src/components/adjustments/Color.tsx', 'src/components/views/EditorView.tsx']) {
    assert.equal(isInterface(f), true, f);
  }
  for (const f of ['src/AppX.tsx', 'src/components/ui/Slider.tsx', 'src/components/modals/ImportSettingsModal.tsx',
    'src/hooks/useExportSettings.ts', 'src/components/panelish/X.tsx']) {
    assert.equal(isInterface(f), false, f);
  }
  assert.equal(isOursToChange('src-tauri/src/lib.rs'), true);
});

test('every anchor names a file that exists, and the files ours to change have no hook count', () => {
  for (const anchor of ANCHORS) {
    assert.ok(existsSync(join(root, anchor.file)), `${anchor.file} does not exist: a typo guards nothing`);
    if (isOursToChange(anchor.file)) {
      assert.equal(anchor.hooks, undefined, `${anchor.file} is ours to change, which has no anchor count`);
    } else {
      assert.ok(Number.isInteger(anchor.hooks) && anchor.what && anchor.instead,
        `${anchor.file} needs hooks, what and instead`);
    }
  }
});

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
  // End to end on the real tree: with the dependency line gone, check-mergeability
  // must exit non-zero and say which wiring went.
  const anchor = wired.find((a) => a.file.endsWith('ExportPanel.tsx'));
  assert.ok(anchor, 'the ExportPanel wiring anchor is the one this was built for');
  const dependency = anchor.requires.at(-1);

  const full = join(root, anchor.file);
  const original = readFileSync(full, 'utf8');
  const baseline = readFileSync(join(root, BASELINE), 'utf8');
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
    // of theirs mutilated, or the ratchet lowered by its damage, turns one red
    // into a confusing dozen.
    writeFileSync(full, original);
    writeFileSync(join(root, BASELINE), baseline);
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
