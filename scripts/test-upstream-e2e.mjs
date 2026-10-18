// End-to-end tests: build a fork in a throwaway repository and run the real
// check-mergeability.mjs against it, asserting the exit code and the message.
//
// WHY THESE EXIST SEPARATELY
//
// test-upstream-checks.mjs tests the functions that decide what the gates see.
// That is not the same as testing that the gates fail. Bypasses have got through
// review because the pieces were right and the wiring was not: a retirement that
// lasted exactly one window, and an entry deleted together with its markers so
// that nothing was left to disagree about.
//
// So each scenario writes a real registry, review register and ratchet baseline
// into a real repository with a tagged upstream, runs the actual checker as a
// subprocess, and asserts that breaking a rule fails it and keeping the rule
// does not.
//
// Run:  npm run test:checks

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { gitIn } from './upstream-git.mjs';

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

// The modules are the real ones: only the data is synthetic. Swapping the array
// and keeping the module means these tests exercise the code that ships.
const swapArray = (text, declaration, body) => {
  const open = text.indexOf(declaration);
  if (open < 0) throw new Error(`cannot find ${declaration}`);
  const close = text.indexOf('\n];\n', open);
  if (close < 0) throw new Error(`cannot find the end of ${declaration}`);
  return text.slice(0, open + declaration.length) + '\n' + body + '\n];' + text.slice(close + 3);
};
const swapLine = (text, start, line) => {
  const re = new RegExp(`^${start}.*$`, 'm');
  if (!re.test(text)) throw new Error(`cannot find ${start}`);
  return text.replace(re, line);
};

const repos = [];

// --- The fixture -------------------------------------------------------------------
//
// Upstream:  base (v1.0.0) - sparkle, preview (v1.1.0) - tidy (v1.2.0) - unreleased
//   sparkle  edits Thing.tsx, a file of theirs a registry entry claims
//   preview  edits lib.rs, which is on the render path
//   tidy     removes `other` from Thing.tsx
// Ours: a borrowed fix in cache_utils.rs and one line in Thing.tsx, so the
// ratchet baseline is { Thing.tsx: 1 }.

const THING = ['export const oldThing = 1;', 'export const other = 2;', 'export const third = 3;'];

const entryText = {
  borrow: ({ retired } = {}) => `  {
    id: 'borrow-1307',
    kind: 'borrowed-fix',
    what: 'cache key hashes contents, not length',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/cache_utils.rs', pr: '1307', how: 'borrows',
        note: 'Marked between // upstream #1307 and // end upstream #1307.' },
    ],
    tests: ['none'],
    keywords: /nothing-matches-this-subject/,${retired ? `
    retired: { recordedIn: '${retired}',
      why: 'Upstream merged the fix and our marked block is identical to theirs.' },` : ''}
  },`,
  thing: ({ shadows = false } = {}) => `  {
    id: 'thing-feature',
    kind: 'feature',
    what: 'our line in their Thing',
    ours: [],
    dependsOn: [
      { file: 'src/components/Thing.tsx', how: 'extends' },${shadows ? `
      { file: 'src/components/Thing.tsx', symbol: 'other', how: 'shadows' },` : ''}
      { file: 'src-tauri/src/cache_utils.rs', how: 'extends' },
    ],
    tests: ['none'],
    keywords: /nothing-matches-this-subject/,
  },`,
};

/** The review of v1.1.0 that keeps every rule. Overrides change one thing. */
const review110 = (s, over = {}) => ({
  release: 'v1.1.0',
  from: s.base,
  to: s.preview,
  on: '2026-10-18',
  hours: 2,
  features: [
    { what: 'sparkle on the thing', commits: [s.sparkle.slice(0, 8)], verdict: 'take',
      why: 'Their sparkle is wanted as it is, and it merged unchanged.' },
    { what: 'a faster preview, in the render path', commits: [s.preview.slice(0, 8)], verdict: 'skip',
      why: 'Our render path already does this differently; not copied.' },
  ],
  sameAsOurs: { verdict: 'none', why: 'Read both commits for features Argentum already has, and found none.' },
  stillLive: ['src/components/Thing.tsx'],
  ...over,
});

const review120 = (s, over = {}) => ({
  release: 'v1.2.0',
  from: s.preview,
  to: s.tidy,
  on: '2026-10-19',
  hours: 1,
  features: [
    { what: 'tidy the thing', commits: [s.tidy.slice(0, 8)], verdict: 'take',
      why: 'A tidy-up of their own file; taken as it is.' },
  ],
  sameAsOurs: { verdict: 'none', why: 'Read the one commit for features Argentum already has, and found none.' },
  stillLive: ['src/components/Thing.tsx'],
  ...over,
});

const seed = (s) => ({
  through: s.base,
  date: '2026-09-13',
  decisions: [],
  note: 'Seed entry for the fixture.',
});

const writer = (dir) => (file, body) => {
  mkdirSync(dirname(join(dir, file)), { recursive: true });
  writeFileSync(join(dir, file), Array.isArray(body) ? body.join('\n') + '\n' : body);
};
const committer = (git) => (message) => {
  git(['add', '-A']);
  git(['commit', '-q', '--allow-empty', '-m', message]);
  return git(['rev-parse', 'HEAD']).trim();
};

/**
 * Upstream's history and the fork's own lines, built once and copied for each
 * scenario: about a hundred milliseconds a git process on Windows made building
 * it twenty-seven times the slowest part of the suite.
 */
let template = null;
function upstreamAndFork() {
  if (template) return template;
  const dir = mkdtempSync(join(tmpdir(), 'ag-e2e-template-'));
  repos.push(dir);
  const git = gitIn(dir);
  const write = writer(dir);
  const commit = committer(git);

  git(['init', '-q', '-b', 'main']);
  git(['config', 'user.email', 'test@argentum.invalid']);
  git(['config', 'user.name', 'Test']);
  git(['config', 'commit.gpgsign', 'false']);
  git(['config', 'core.autocrlf', 'false']);

  // Upstream.
  write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    let n = s.len();', '}']);
  write('src-tauri/src/lib.rs', ['fn run() {}']);
  write('src/components/Thing.tsx', THING);
  write('src/components/Spare.tsx', ['export const spare = 1;']);
  const s = { base: commit('upstream base') };
  git(['tag', 'v1.0.0']);
  git(['checkout', '-q', '-b', 'upstream/main']);
  write('src/components/Thing.tsx', ['export const oldThing = 1; // sparkle', ...THING.slice(1)]);
  s.sparkle = commit('add sparkle to the thing');
  write('src-tauri/src/lib.rs', ['fn run() { fast(); }']);
  s.preview = commit('speed up the preview');
  git(['tag', '-a', 'v1.1.0', '-m', 'v1.1.0']);
  write('src/components/Thing.tsx', ['export const oldThing = 1; // sparkle', THING[2]]);
  s.tidy = commit('tidy the thing');
  git(['tag', 'v1.2.0']);
  write('src/components/Spare.tsx', ['export const spare = 2;']);
  s.unreleased = commit('work for the next release');
  git(['checkout', '-q', 'main']);

  // Ours: the scripts under test and the fork's own lines. The registers are
  // each scenario's own.
  mkdirSync(join(dir, 'scripts'), { recursive: true });
  for (const f of [
    'check-mergeability.mjs',
    'upstream-anchors.mjs',
    'upstream-borrows.mjs',
    'upstream-diff.mjs',
    'upstream-git.mjs',
    'upstream-ratchet.mjs',
    'upstream-releases.mjs',
  ]) {
    copyFileSync(join(here, f), join(dir, 'scripts', f));
  }
  write('src-tauri/src/cache_utils.rs', [
    'fn hash() {', '    // upstream #1307', '    s.hash(h);', '    // end upstream #1307', '}',
  ]);
  write('src/components/Thing.tsx', [...THING, 'export const ours = true;']);
  commit('argentum: the scripts and our lines');
  template = { dir, s };
  return template;
}

/**
 * Build a fork and run the checker on it.
 *
 *   merged      the release tag merged into the fork
 *   reviews     s -> REVIEWS (s holds the upstream shas by name)
 *   entries     s -> registry entry texts
 *   edits       files to write in a last commit of ours
 *   baseline    the committed ratchet numbers
 *   mainBaseline  if set, origin/main carries these numbers and the branch moves to `baseline`
 *   changelog   s -> the CHANGELOG line, or 'auto'
 *   ci          run as CI
 *   dropEntryAndMarkers  delete the borrow entry and its markers in a last commit
 */
function scenario({
  merged = 'v1.0.0',
  reviews = (s) => [seed(s)],
  entries = () => [entryText.borrow(), entryText.thing()],
  edits = {},
  baseline = { 'src/components/Thing.tsx': 1 },
  mainBaseline = null,
  changelog = 'auto',
  ci = false,
  dropEntryAndMarkers = false,
} = {}) {
  const { dir: from, s } = upstreamAndFork();
  const dir = mkdtempSync(join(tmpdir(), 'ag-e2e-'));
  repos.push(dir);
  cpSync(from, dir, { recursive: true });
  const git = gitIn(dir);
  const write = writer(dir);
  const commit = committer(git);

  write('scripts/upstream-registry.mjs', swapArray(
    readFileSync(join(here, 'upstream-registry.mjs'), 'utf8'),
    'export const REGISTRY = [', entries(s).join('\n'),
  ));
  write('scripts/upstream-decisions.mjs', swapLine(
    swapArray(
      readFileSync(join(here, 'upstream-decisions.mjs'), 'utf8'),
      'export const REVIEWS = [', reviews(s).map((r) => `  ${JSON.stringify(r)},`).join('\n'),
    ),
    'export const PER_COMMIT_ENDS = ', `export const PER_COMMIT_ENDS = '${s.base}';`,
  ));
  const tagged = { 'v1.0.0': s.base, 'v1.1.0': s.preview, 'v1.2.0': s.tidy };
  write('CHANGELOG.md', changelog === 'auto'
    ? `**Based on RapidRAW \`${merged.slice(1)}\` @ \`${tagged[merged].slice(0, 8)}\`**\n`
    : changelog(s));
  const ratchet = (files) => `${JSON.stringify({ about: 'fixture', files }, null, 2)}\n`;
  write('scripts/upstream-ratchet.json', ratchet(mainBaseline ?? baseline));
  const first = commit('argentum: the registers');
  if (mainBaseline) {
    git(['update-ref', 'refs/remotes/origin/main', first]);
    write('scripts/upstream-ratchet.json', ratchet(baseline));
    commit('argentum: change the ratchet');
  }

  if (merged !== 'v1.0.0') git(['merge', '-q', '-X', 'ours', '--no-edit', merged]);

  if (Object.keys(edits).length > 0) {
    for (const [file, body] of Object.entries(edits)) write(file, body);
    commit('argentum: a change');
  }
  if (dropEntryAndMarkers) {
    write('src-tauri/src/cache_utils.rs', ['fn hash() {', '    s.hash(h);', '}']);
    write('scripts/upstream-registry.mjs', swapArray(
      readFileSync(join(dir, 'scripts/upstream-registry.mjs'), 'utf8'),
      'export const REGISTRY = [', entryText.thing(),
    ));
    commit('adopt theirs, and tidy the registry while we are here');
  }

  const run = spawnSync(process.execPath, ['scripts/check-mergeability.mjs'], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, CI: ci ? 'true' : '' },
  });
  return {
    s,
    status: run.status,
    out: (run.stdout ?? '') + (run.stderr ?? ''),
    read: (file) => readFileSync(join(dir, file), 'utf8'),
  };
}

const fails = (r, pattern) => {
  assert.equal(r.status, 1, `expected a failure, got:\n${r.out}`);
  assert.match(r.out, pattern);
};
const passes = (r) => assert.equal(r.status, 0, `expected a pass, got:\n${r.out}`);

// --- The fork as it stands ----------------------------------------------------------

test('e2e: a fork merged up to its last reviewed release passes, and says the next one is waiting', () => {
  const r = scenario();
  passes(r);
  assert.match(r.out, /RapidRAW v1\.1\.0, v1\.2\.0 are out and not reviewed/);
});

// --- The ratchet --------------------------------------------------------------------

test('e2e: ratchet - one more line of ours in a file of theirs fails', () => {
  fails(scenario({ edits: { 'src/components/Thing.tsx': [...THING, 'export const ours = true;', 'export const more = 1;'] } }),
    /src\/components\/Thing\.tsx\s+2 of our lines, up from 1/);
});

test('e2e: ratchet - lines of ours in a file of theirs that had none fail', () => {
  fails(scenario({
    edits: {
      'src-tauri/src/cache_utils.rs': [
        'fn hash() {', '    // upstream #1307', '    s.hash(h);', '    // end upstream #1307', '    ours();', '}',
      ],
    },
  }), /1 of our lines in a file of theirs that had none/);
});

test('e2e: ratchet - going down locally passes and lowers the committed number', () => {
  const r = scenario({ edits: { 'src/components/Thing.tsx': THING } });
  passes(r);
  assert.match(r.out, /ratchet: src\/components\/Thing\.tsx 1 -> 0/);
  assert.deepEqual(JSON.parse(r.read('scripts/upstream-ratchet.json')).files, {});
});

test('e2e: ratchet - going down in CI fails until the lower number is committed', () => {
  fails(scenario({ edits: { 'src/components/Thing.tsx': THING }, ci: true }),
    /1 file is below the baseline: src\/components\/Thing\.tsx 1 -> 0/);
});

test('e2e: ratchet - raising the committed number by hand fails against main', () => {
  fails(scenario({
    mainBaseline: { 'src/components/Thing.tsx': 1 },
    baseline: { 'src/components/Thing.tsx': 5 },
    edits: { 'src/components/Thing.tsx': [...THING, 'export const ours = true;', 'a;', 'b;'] },
  }), /src\/components\/Thing\.tsx is allowed 5, and was allowed 1 on main/);
});

test('e2e: ratchet - a lower number than main passes', () => {
  passes(scenario({
    mainBaseline: { 'src/components/Thing.tsx': 3 },
    baseline: { 'src/components/Thing.tsx': 1 },
  }));
});

// --- The render path ------------------------------------------------------------------

const hooks = (n) => Array.from({ length: n }, (_, i) => `    mods::tool_${i}::run();`);

test('e2e: render path - lib.rs takes any number of our lines and calls into our code', () => {
  passes(scenario({ edits: { 'src-tauri/src/lib.rs': ['fn run() {', ...hooks(20), '}'] } }));
});

test('e2e: render path - the same lines in a file of theirs outside it fail the anchor and the ratchet', () => {
  const r = scenario({ edits: { 'src/components/Thing.tsx': [...THING, 'export const ours = true;', ...hooks(3)] } });
  fails(r, /3 calls into our code, the anchor allows 0/);
  assert.match(r.out, /4 of our lines, up from 1/);
});

// --- Coverage ------------------------------------------------------------------------------

test('e2e: coverage - changing a file of theirs no registry entry claims fails', () => {
  fails(scenario({
    edits: { 'src/components/Spare.tsx': ['export const spare = 1;', 'export const mine = 1;'] },
    baseline: { 'src/components/Thing.tsx': 1, 'src/components/Spare.tsx': 1 },
  }), /we changed this file of theirs and no registry entry claims it/);
});

// --- The release reviews ---------------------------------------------------------------------

test('e2e: release - a complete review of the merged release passes', () => {
  passes(scenario({ merged: 'v1.1.0', reviews: (s) => [seed(s), review110(s)] }));
});

test('e2e: release - merging a release with no review of it fails', () => {
  fails(scenario({ merged: 'v1.1.0' }),
    /reviewed up to .{8}, and upstream is merged in up to .{8} \(v1\.1\.0\)/);
});

test('e2e: release - a review that leaves a commit out fails', () => {
  fails(scenario({
    merged: 'v1.1.0',
    reviews: (s) => [seed(s), review110(s, { features: [review110(s).features[1]] })],
  }), /"add sparkle to the thing" is in no feature/);
});

test('e2e: release - a render-path change marked take fails', () => {
  fails(scenario({
    merged: 'v1.1.0',
    reviews: (s) => [seed(s), review110(s, {
      features: review110(s).features.map((f) => ({ ...f, verdict: 'take' })),
    })],
  }), /changes src-tauri\/src\/lib\.rs, and every feature it is in is take/);
});

test('e2e: release - sameAsOurs found with nothing decided fails', () => {
  fails(scenario({
    merged: 'v1.1.0',
    reviews: (s) => [seed(s), review110(s, {
      sameAsOurs: { verdict: 'found', why: 'They built our sparkle, and the two need reconciling by hand.' },
    })],
  }), /sameAsOurs is found, and no feature is decide/);
});

test('e2e: release - stillLive going up from one release review to the next fails', () => {
  fails(scenario({
    merged: 'v1.2.0',
    entries: () => [entryText.borrow(), entryText.thing()],
    reviews: (s) => [seed(s), review110(s), review120(s, {
      stillLive: ['src/components/Thing.tsx', 'src/components/Spare.tsx'],
    })],
  }), /2 of their files still live, up from 1 at v1\.1\.0/);
});

test('e2e: release - two release reviews in a row, stillLive level, pass', () => {
  passes(scenario({ merged: 'v1.2.0', reviews: (s) => [seed(s), review110(s), review120(s)] }));
});

test('e2e: release - a gap in the chain of reviews fails', () => {
  fails(scenario({
    merged: 'v1.2.0',
    reviews: (s) => [seed(s), review110(s), review120(s, { from: s.sparkle })],
  }), /v1\.2\.0: starts at .{8}, and the review before it ended at .{8}/);
});

test('e2e: release - CHANGELOG.md naming another release fails', () => {
  fails(scenario({
    merged: 'v1.1.0',
    reviews: (s) => [seed(s), review110(s)],
    changelog: (s) => `**Based on RapidRAW \`1.0.0\` @ \`${s.preview.slice(0, 8)}\`**\n`,
  }), /CHANGELOG\.md\s+says 1\.0\.0 @/);
});

// --- The per-commit history ------------------------------------------------------------------

test('e2e: history - a per-commit review added after the last one fails', () => {
  fails(scenario({
    reviews: (s) => [seed(s), {
      through: s.preview, date: '2026-10-20', decisions: [],
      featureReview: { verdict: 'none', why: 'Read every subject in the batch and found nothing of ours.' },
    }],
  }), /a per-commit review \(.{8}\) after the last one/);
});

test('e2e: history - a recorded per-commit decision with no reasoning fails', () => {
  fails(scenario({
    reviews: (s) => [{ ...seed(s), decisions: [{ overlap: 'abc:dep:x', verdict: 'adopt', why: 'ok' }] }],
  }), /the recorded decision abc:dep:x has no valid verdict or no reasoning/);
});

// --- Integrity --------------------------------------------------------------------------------

test('e2e: integrity - a retirement listed in its release review passes', () => {
  passes(scenario({
    merged: 'v1.1.0',
    entries: () => [entryText.borrow({ retired: 'v1.1.0' }), entryText.thing()],
    reviews: (s) => [seed(s), review110(s, { retires: ['borrow-1307'] })],
  }));
});

test('e2e: integrity - a retirement its release review does not list fails', () => {
  fails(scenario({
    merged: 'v1.1.0',
    entries: () => [entryText.borrow({ retired: 'v1.1.0' }), entryText.thing()],
    reviews: (s) => [seed(s), review110(s)],
  }), /is retired in v1\.1\.0, and that review does not list it in retires/);
});

test('e2e: integrity - a retirement naming no review fails', () => {
  fails(scenario({
    entries: () => [entryText.borrow({ retired: 'v9.9.9' }), entryText.thing()],
  }), /names no review in REVIEWS/);
});

test('e2e: integrity - deleting the entry and its markers together is caught by our own history', () => {
  fails(scenario({ dropEntryAndMarkers: true }),
    /borrow-1307 was in the inventory and has been deleted from it/);
});

// --- Shadows ------------------------------------------------------------------------------------

test('e2e: shadows - a symbol we step around, gone from the release we merged, fails', () => {
  fails(scenario({
    merged: 'v1.2.0',
    entries: () => [entryText.borrow(), entryText.thing({ shadows: true })],
    reviews: (s) => [seed(s), review110(s), review120(s)],
  }), /other is gone from the release we merged, and thing-feature steps around it/);
});

test('e2e: shadows - the same symbol gone only from their main is a warning', () => {
  const r = scenario({
    merged: 'v1.1.0',
    entries: () => [entryText.borrow(), entryText.thing({ shadows: true })],
    reviews: (s) => [seed(s), review110(s)],
  });
  passes(r);
  assert.match(r.out, /warning: src\/components\/Thing\.tsx: other, which thing-feature steps around, is gone from upstream\/main/);
});

for (const dir of repos) {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch { /* git may still hold a pack file; the temp directory is disposable */ }
}

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exit(failed > 0 ? 1 : 0);
