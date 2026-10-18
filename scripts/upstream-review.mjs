// The release review: what RapidRAW changed between the last reviewed release
// and their newest one, in the order it is read, with the register entry ready
// to paste into scripts/upstream-decisions.mjs.
//
// WHY PER RELEASE
//
// Until 2026-10-18 every upstream commit that touched a file we depend on needed
// its own recorded decision. The RapidRAW 1.6.5 review produced 1,249 of them,
// 84% "not-applicable". Most of the effort went into saying that unrelated
// commits were unrelated. A release is read once now, one line per feature:
//
//   1. their shader changes first: the most valuable, and the easiest to copy;
//   2. their render-path changes, to read and copy by hand, never to merge;
//   3. which of their changes land on files our registry entries depend on;
//   4. every feature of the release, with a verdict.
//
// `git merge` cannot say whether upstream built something we already have, in
// files we have never touched. Nothing here can either: sameAsOurs is the
// reviewer's answer to that, in words.
//
// Run:  git fetch upstream && npm run review:upstream [-- <tag>] [-- --full]
//
// --full also prints the render-path diff; the shader diff is always printed.

import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { borrowMarkers, borrowStatus } from './upstream-borrows.mjs';
import { REVIEWS, isRelease, reviewedTo } from './upstream-decisions.mjs';
import { commitsIn, gitIn, tags } from './upstream-git.mjs';
import {
  REGISTRY, RENDER_PATH, activeFor, dependencyIndex, isOurs, isRenderPath,
} from './upstream-registry.mjs';
import { featureGroups } from './upstream-releases.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const git = gitIn(root);
const short = (sha) => String(sha).slice(0, 8);
const args = process.argv.slice(2);
const full = args.includes('--full');
const SHADERS = 'src-tauri/src/shaders/';

if (!existsSync(join(root, '.git'))) process.exit(0);

const versionOf = (name) => name.slice(1).split('.').map(Number);
const byVersion = (a, b) => {
  const [x, y] = [versionOf(a), versionOf(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) - (y[i] ?? 0);
  }
  return 0;
};

const allTags = tags(git);
const releases = [...allTags.keys()].filter((n) => /^v\d+(\.\d+)*$/.test(n)).sort(byVersion);
const last = REVIEWS[REVIEWS.length - 1];
const from = reviewedTo();
const fromName = isRelease(last)
  ? last.release
  : releases.find((n) => allTags.get(n) === from) ?? short(from);

const asked = args.find((a) => !a.startsWith('--'));
const target = asked ?? releases[releases.length - 1];
if (!target || !allTags.has(target)) {
  console.log(`  no release tag ${asked ?? ''} here - run: git fetch upstream`);
  process.exit(asked ? 1 : 0);
}
const to = allTags.get(target);

if (to === from || (releases.includes(fromName) && byVersion(target, fromName) <= 0)) {
  if (asked) {
    console.log(`  RapidRAW ${target} is already reviewed: the register reaches ${fromName}.`);
    process.exit(0);
  }
  let since = 0;
  try {
    since = Number(git(['rev-list', '--count', `${from}..upstream/main`]).trim());
  } catch { /* no upstream/main: nothing more to say */ }
  console.log(`  RapidRAW ${fromName} is reviewed, and it is their newest release here.`);
  if (since > 0) console.log(`  ${since} commits on their main since; they are for the next release review.`);
  console.log('  (git fetch upstream first if this is out of date.)');
  process.exit(0);
}

const commits = commitsIn(git, from, to);
const work = commits.filter((c) => c.parents.length < 2);
const touches = (c, test) => c.files.some(test);
const isShader = (f) => f.startsWith(SHADERS);

console.log('');
console.log(`  RapidRAW ${fromName} -> ${target}: ${work.length} commits (${commits.length - work.length} merges), `
  + `${short(from)}..${short(to)}`);
console.log('');

// --- 1. Their shaders -------------------------------------------------------------
const shaderFiles = [...new Set(work.flatMap((c) => c.files.filter(isShader)))].sort();
console.log('  1. THEIR SHADER CHANGES - read first: the most valuable and the easiest to copy');
console.log('');
if (shaderFiles.length === 0) {
  console.log('     none in this release');
} else {
  for (const c of work.filter((x) => touches(x, isShader))) {
    console.log(`     ${short(c.sha)}  ${c.subject}`);
  }
  console.log('');
  const diff = git(['diff', from, to, '--', ...shaderFiles]);
  for (const line of diff.split('\n')) console.log(`     ${line}`);
}
console.log('');

// --- 2. The render path -------------------------------------------------------------
const renderCommits = work.filter((c) => touches(c, isRenderPath));
console.log('  2. THEIR RENDER-PATH CHANGES - ours to change: read, copy by hand what is wanted, never merge');
console.log('');
if (renderCommits.length === 0) {
  console.log('     none in this release');
} else {
  for (const c of renderCommits) {
    const files = c.files.filter(isRenderPath).map((f) => f.split('/').pop()).join(', ');
    console.log(`     ${short(c.sha)}  ${c.subject}  [${files}]`);
  }
  console.log('');
  const stat = git(['diff', '--stat', from, to, '--', ...RENDER_PATH]);
  for (const line of stat.trimEnd().split('\n')) console.log(`     ${line}`);
  console.log('');
  if (full) {
    for (const line of git(['diff', from, to, '--', ...RENDER_PATH]).split('\n')) console.log(`     ${line}`);
  } else {
    console.log(`     git diff ${short(from)} ${short(to)} -- ${RENDER_PATH.join(' ')}`);
    console.log('     (or npm run review:upstream -- --full)');
  }
}
console.log('');

// --- 3. What lands on code of ours ---------------------------------------------------
const entries = activeFor(REGISTRY, REVIEWS, null);
const index = dependencyIndex(entries);
const changed = [...new Set(work.flatMap((c) => c.files))].sort();
const claimed = changed.filter((f) => !isRenderPath(f) && index.claims(f));
console.log('  3. THEIR CHANGES TO FILES OUR REGISTRY ENTRIES DEPEND ON - each entry says what it relies on');
console.log('');
if (claimed.length === 0) console.log('     none');
for (const file of claimed) {
  const n = work.filter((c) => c.files.includes(file)).length;
  console.log(`     ${file}  (${n} commit${n === 1 ? '' : 's'}): ${index.claimants(file).join(', ')}`);
}
const marks = borrowMarkers(git);
for (const { pr, landedIn } of borrowStatus(commits, marks.prs)) {
  console.log(landedIn.length > 0
    ? `     borrowed #${pr} is named by ${landedIn.join(', ')}: if their block now matches ours, retire the entry in this review`
    : `     borrowed #${pr}: no commit names it - not proof it is still pending; read the file's changes above`);
}
console.log('');

// --- 4. The features -------------------------------------------------------------------
const groups = featureGroups(commits, to);
const filesOf = (g) => [...new Set(g.commits.flatMap((c) => c.files))];
const rank = (g) => (filesOf(g).some(isShader) ? 0 : filesOf(g).some(isRenderPath) ? 1 : 2);
const ordered = groups.map((g, i) => ({ g, i })).sort((a, b) => rank(a.g) - rank(b.g) || a.i - b.i).map(({ g }) => g);
const readsLike = (g) => entries
  .filter((e) => e.keywords && g.commits.some((c) => e.keywords.test(c.subject)))
  .map((e) => e.id);
const hint = (g) => {
  const files = filesOf(g);
  const parts = [];
  if (files.some(isShader)) parts.push('shader');
  const lands = files.filter((f) => !isRenderPath(f) && index.claims(f)).map((f) => f.split('/').pop());
  if (lands.length > 0) parts.push(`lands on registered: ${lands.join(', ')}`);
  const like = readsLike(g);
  if (like.length > 0) parts.push(`reads like: ${like.join(', ')}`);
  return parts.length > 0 ? `  // ${parts.join('; ')}` : '';
};

console.log(`  4. THE FEATURES - ${groups.length} groups, one per pull request or direct commit; join and split them as the features really are`);
console.log('');
for (const g of ordered) {
  console.log(`     ${g.what}`);
  for (const c of g.commits) console.log(`       ${short(c.sha)}  ${c.subject}`);
}
console.log('');

// --- The register entry -------------------------------------------------------------------
const q = (s) => `'${String(s).replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
const list = (shas) => `[${shas.map((s) => q(short(s))).join(', ')}]`;
const today = new Date();
const stamp = [
  today.getFullYear(),
  String(today.getMonth() + 1).padStart(2, '0'),
  String(today.getDate()).padStart(2, '0'),
].join('-');
const previousRelease = [...REVIEWS].reverse().find(isRelease);

console.log('  When the review is done, the release merged and the tests pass, add this to the end of');
console.log('  REVIEWS in scripts/upstream-decisions.mjs. npm run check:merge holds it to the rules');
console.log('  written at the top of that file.');
console.log('');
console.log('    {');
console.log(`      release: ${q(target)},`);
console.log(`      from: ${q(from)},`);
console.log(`      to: ${q(to)},`);
console.log(`      on: ${q(stamp)},`);
console.log('      hours: 0,');
console.log('      features: [');
let section = -1;
const headings = [
  '// Their shaders',
  '// Touching the render path: each "in the render path" line is copy, skip or decide, never take',
  '// Everything else',
];
for (const g of ordered) {
  const r = rank(g);
  if (r !== section) {
    console.log(`        ${headings[r]}`);
    section = r;
  }
  const render = g.commits.filter((c) => touches(c, isRenderPath));
  const rest = g.commits.filter((c) => c.files.some((f) => !isRenderPath(f)));
  if (rest.length > 0) {
    console.log(`        { what: ${q(g.what)}, commits: ${list(rest.map((c) => c.sha))}, `
      + `verdict: 'take | copy | skip | decide', why: '' },${hint(g)}`);
  }
  if (render.length > 0) {
    const files = [...new Set(render.flatMap((c) => c.files.filter(isRenderPath)))].map((f) => f.split('/').pop());
    console.log(`        { what: ${q(`${g.what}, in the render path (${files.join(', ')})`)}, `
      + `commits: ${list(render.map((c) => c.sha))}, verdict: 'copy | skip | decide', why: '' },`);
  }
}
console.log('      ],');
console.log("      sameAsOurs: { verdict: 'none | found', why: 'What you read the release for, and what you concluded.' },");
if (previousRelease) {
  console.log(`      // ${previousRelease.release} listed ${previousRelease.stillLive.length}; this list may not be longer.`);
  console.log('      stillLive: [');
  for (const p of previousRelease.stillLive) console.log(`        ${q(p)},`);
  console.log('      ],');
} else {
  // Candidates, not the answer: their shaders, and their Rust modules that the
  // render code (gpu_processing.rs, image_processing.rs) names. lib.rs is left
  // out of the search because it names every module it registers. They are
  // printed commented out, so the first list is a decision and not a paste.
  const render = ['src-tauri/src/gpu_processing.rs', 'src-tauri/src/image_processing.rs']
    .map((f) => (existsSync(join(root, f)) ? readFileSync(join(root, f), 'utf8') : ''))
    .join('\n');
  const candidates = git(['ls-files', '--', 'src-tauri/src']).split('\n').filter((f) => {
    if (!f || isOurs(f) || isRenderPath(f)) return false;
    if (f.startsWith(SHADERS)) return f.endsWith('.wgsl');
    const stem = f.match(/^src-tauri\/src\/([a-z0-9_]+)\.rs$/)?.[1];
    return stem !== undefined && new RegExp(`\\b${stem}::`).test(render);
  });
  console.log('      // Their files still run when previewing, exporting or masking. The first release');
  console.log('      // review sets the number and every later one may not list more. Candidates from');
  console.log('      // the render code are below, commented out: check each, and the export and mask');
  console.log('      // paths too.');
  console.log('      stillLive: [');
  for (const p of candidates) console.log(`        // ${q(p)},`);
  console.log('      ],');
}
console.log('    },');
console.log('');
console.log('  Merging the release, with the render path kept as ours:');
console.log('');
console.log(`    git merge --no-commit ${target}`);
console.log(`    git checkout HEAD -- ${RENDER_PATH.join(' ')}`);
console.log('    (copy in by hand what the review marked copy, then resolve, test and commit)');
console.log('');
console.log(`  and make CHANGELOG.md say: **Based on RapidRAW \`${target.replace(/^v/, '')}\` @ \`${short(to)}\`**`);
console.log('');
