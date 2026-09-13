// What arrived from upstream since the last recorded review, and what it lands on.
//
// WHY THIS EXISTS
//
// `git merge` answers "do these texts collide". It cannot answer the question
// that actually matters to a fork: **has upstream now built the thing we built?**
// On 2026-09-13 ten upstream commits merged with zero conflicts in application
// code, and that was reported as proof the architecture works. It was not. If
// upstream had shipped their own white balance, git would have merged it in
// silence and Argentum would have had two, or theirs would quietly have won.
//
// The first version of this script started from the git merge-base, which meant
// the merge itself closed the window: afterwards it printed "nothing new" whether
// or not anybody had looked. It starts from the review register now, so the list
// survives the merge and goes on being printed until the decisions are written
// down. check-mergeability.mjs fails for exactly as long.
//
// Run:  npm run review:upstream

import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { borrowMarkers, borrowStatus, detectOverlaps } from './upstream-overlaps.mjs';
import { REGISTRY, activeFor, dependencyIndex } from './upstream-registry.mjs';
import { REVIEWS, reviewedThrough } from './upstream-decisions.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const git = (cmd, big = false) =>
  execSync(cmd, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: (big ? 96 : 8) * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
const short = (sha) => sha.slice(0, 8);

if (!existsSync(join(root, '.git'))) process.exit(0);

let head;
try {
  head = git('git rev-parse upstream/main').trim();
} catch {
  console.log('  no upstream/main ref - run: git fetch upstream');
  process.exit(0);
}

const from = reviewedThrough();
let base;
try {
  base = git(`git rev-parse --verify ${from}`).trim();
} catch {
  console.log(`  the review register names ${short(from)}, which is not a commit here.`);
  console.log('  Fetch upstream with full history and try again.');
  process.exit(1);
}

// Where the merge itself has got to. Commits between `base` and `merged` are
// already in our tree and unreviewed - the dangerous kind, because nothing will
// ever conflict over them again.
let merged = base;
try {
  merged = git('git merge-base HEAD upstream/main').trim();
} catch { /* keep base */ }

if (base === head) {
  console.log(`  upstream: nothing new since ${short(base)}, and it is reviewed`);
  process.exit(0);
}

const commits = git(`git log --format="%h%x09%s" ${base}..${head}`).trim().split('\n').filter(Boolean);
const mergedCount = base === merged
  ? 0
  : git(`git log --format=%h ${base}..${merged}`).trim().split('\n').filter(Boolean).length;

console.log('');
console.log(`  ${commits.length} upstream commits to review, ${short(base)}..${short(head)}`);
if (mergedCount > 0) {
  console.log(`  ${mergedCount} of them are ALREADY MERGED into this tree and unreviewed.`);
  console.log('  Nothing will conflict over those again. They are the ones to read first.');
}
console.log('');

// The window being prepared has no recorded review yet, so every retirement on
// the books is behind it. An entry retired in the review about to be written is
// still unretired here, which is why retiring one does not hide it.
const entries = activeFor(REGISTRY, REVIEWS, null);
const index = dependencyIndex(entries);
const marks = borrowMarkers(git);
const overlaps = detectOverlaps(git, base, head, { entries, index });
const isMerged = (sha) => {
  if (mergedCount === 0) return false;
  try {
    return git(`git merge-base --is-ancestor ${sha} ${merged} && echo yes`).trim() === 'yes';
  } catch {
    return false;
  }
};

const gated = overlaps.filter((o) => o.gated);
const deps = gated.filter((o) => o.kind === 'dep');
const features = gated.filter((o) => o.kind === 'feature');

if (deps.length === 0) {
  console.log('  nothing upstream touches a registered dependency in this batch.');
  console.log('');
} else {
  console.log(`  DEPENDENCY OVERLAPS - mechanical, and each needs a decision (${deps.length}):`);
  console.log('');
  for (const o of deps) {
    console.log(`    ${o.key}${isMerged(o.commit) ? '   [already merged]' : ''}`);
    console.log(`      ${o.commit}  ${o.subject}`);
    console.log(`      ${o.detail}`);
    console.log('');
  }
}

if (features.length > 0) {
  console.log(`  POSSIBLE FEATURE OVERLAPS - a hint from the subject line only (${features.length}):`);
  console.log('');
  for (const o of features) {
    console.log(`    ${o.key}${isMerged(o.commit) ? '   [already merged]' : ''}`);
    console.log(`      ${o.commit}  ${o.subject}`);
    console.log(`      ${o.detail}`);
    console.log('');
  }
}

// Borrowed fixes. Matching the pull request number finds a squash merge and
// nothing else: most RapidRAW commits are the maintainer's own and name no
// number, so a fix of theirs for the same bug is invisible to a grep. The
// dependency overlaps above are what actually catch that; this is the cheap
// extra check, reported per pull request so one landing cannot silence another.
for (const { pr, landedIn } of borrowStatus(git, base, head, marks.prs)) {
  if (landedIn.length > 0) {
    console.log(`  BORROWED #${pr} names a commit upstream (${landedIn.join(', ')})`);
    console.log('    If their block is now identical to ours, retire the registry entry in');
    console.log('    the review that says so - deleting the markers alone will fail the check.');
  } else {
    console.log(`  borrowed #${pr}: no commit in this batch names it - which is not proof`);
    console.log('    it is still pending. Check the dependency overlaps for its file.');
  }
}
console.log('');

console.log('  commits:');
console.log('');
for (const line of commits) {
  const [sha, ...rest] = line.split('\t');
  const flags = [...new Set(overlaps.filter((o) => o.commit === sha).map((o) => o.kind))];
  const mark = flags.length > 0 ? '  <-- ' + flags.join(', ') : '';
  console.log(`    ${sha}  ${rest.join('\t')}${mark}`);
}

console.log('');
console.log('  READ THAT LIST FOR FEATURES WE ALREADY HAVE. Nothing here detects that: a');
console.log('  file match finds upstream changing something we registered, never upstream');
console.log('  building the same thing somewhere we have never touched.');
console.log('');
console.log('  When the merge is done and each overlap is decided, add this to');
console.log('  scripts/upstream-decisions.mjs - check:merge fails until it is there:');
console.log('');
const today = new Date();
const stamp = [
  today.getFullYear(),
  String(today.getMonth() + 1).padStart(2, '0'),
  String(today.getDate()).padStart(2, '0'),
].join('-');
console.log('    {');
console.log(`      through: '${head}',`);
console.log(`      date: '${stamp}',`);
console.log('      decisions: [');
for (const o of gated) {
  console.log(`        { overlap: '${o.key}',`);
  console.log("          verdict: 'adopt | keep-ours | combine | not-applicable',");
  console.log("          why: '' },");
}
console.log('      ],');
console.log('      featureReview: {');
console.log("        verdict: 'none | overlap-found',");
console.log("        why: 'What you read the batch for, and what you concluded.',");
console.log('      },');
console.log('    },');
console.log('');
console.log(`  and make CHANGELOG.md say: **Based on RapidRAW \`x.y.z\` @ \`${short(head)}\`**`);
console.log('');
