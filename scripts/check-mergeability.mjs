// THE RULE: Argentum's code goes in Argentum's files, and our lines in
// RapidRAW's files only go down.
//
// Argentum began as a fork of RapidRAW and is becoming its own application: new
// tools go into the engine (src-tauri/engine/), the render path is ours to
// change, and RapidRAW is a source reviewed once per release rather than a base
// merged forever. This script holds that line instead of trusting anyone to
// remember it. It runs in CI as a required check (.github/workflows/upstream.yml)
// and with `npm run check:merge`, and it fails the build when:
//
//   * a file of theirs holds more of our lines than scripts/upstream-ratchet.json
//     allows (the ratchet; it only goes down);
//   * a file from RapidRAW we changed is claimed by no registry entry (coverage);
//   * the registry and the tree disagree, an entry was deleted, or a retirement
//     does not hold up (integrity);
//   * a file of theirs calls into our code more often than its anchor allows,
//     or lost the wiring its anchor exists for;
//   * upstream is merged in further than the release reviews reach, or the
//     newest release review breaks a rule (scripts/upstream-decisions.mjs);
//   * something of theirs we step around is gone from the release we merged.
//
// The render path and today's interface (RENDER_PATH and INTERFACE in
// scripts/upstream-registry.mjs) came from RapidRAW and are ours to change in
// place: no ratchet, no anchor count, no shadow check. Coverage still applies,
// and their upstream changes are read at each release review, never merged.
//
// Nothing here asks a pull request for paperwork about upstream commits: a
// change to a file from RapidRAW needs a registry entry, and overlaps with
// upstream are handled once, at the release review.
//
// Every question is asked of git once, by a streamed command
// (scripts/upstream-git.mjs). The run used to start about 390 git processes and
// take 37-74 s; it now starts about ten.
//
// Run standalone:  node scripts/check-mergeability.mjs [--verbose]

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { ANCHORS } from './upstream-anchors.mjs';
import { borrowMarkers } from './upstream-borrows.mjs';
import { REVIEWS, isRelease, reviewedTo } from './upstream-decisions.mjs';
import { accountDiff } from './upstream-diff.mjs';
import { commitsIn, gitIn, readBlobs, tags } from './upstream-git.mjs';
import { BASELINE, compareRatchet, formatBaseline, parseBaseline } from './upstream-ratchet.mjs';
import {
  IDENTITY, OURS, REGISTRY, activeFor, borrowsOf, dependencyIndex, historicalIds, isOurs,
  isOursToChange, shadowsOf, validateRetirements,
} from './upstream-registry.mjs';
import { validateReviews } from './upstream-releases.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const started = Date.now();
const verbose = process.argv.includes('--verbose');

let gitCalls = 0;
const gitRaw = gitIn(root);
const git = (args, options) => {
  gitCalls += 1;
  return gitRaw(args, options);
};

/**
 * Does this added line reach into Argentum's code?
 *
 * A hook is a call, an import or a mount point — a place their code depends on
 * ours. The rebrand is not: `.agdata`, or "Argentum" appearing inside a
 * sentence they already had. Those are edits to text, and text does not grow
 * with the number of features.
 *
 * The shader pattern used to be `ag_stage_` alone, which meant a call to any
 * other `ag_` function in their WGSL was not counted at all — a hole found by
 * walking through it. It matches any `ag_…(` call now.
 */
const isHook = (line) => {
  if (/\.agdata|\.agexif/.test(line)) return false;
  return /\bmods::|^\s*mod mods;|\bag_[a-z0-9_]*\(|argentum\/|'\.\/argentum|data-argentum|<Argentum\b|\bArgentum from\b/.test(line);
};

const short = (sha) => String(sha).slice(0, 8);

/**
 * Prerequisites.
 *
 * Everything below is measured against upstream/main. Without that ref there is
 * nothing to compare against: skip on a contributor's fresh clone, fail in CI,
 * where a vacuous pass is indistinguishable from a real one and the check is a
 * required one. The workflow adds the remote and fetches full history.
 */
const inCI = process.env.CI === 'true' || process.env.CI === '1';
const prerequisite = (detail, fix) => {
  if (inCI) {
    console.error('');
    console.error('  UPSTREAM CHECK CANNOT RUN');
    console.error('');
    console.error(`    ${detail}`);
    console.error(`    ${fix}`);
    console.error('');
    process.exit(1);
  }
  console.log(`  mergeability: ${detail} — skipping`);
  process.exit(0);
};

if (!existsSync(join(root, '.git'))) {
  prerequisite('not a git repository', 'Nothing to compare against.');
}

let shallow = 'false';
try {
  shallow = git(['rev-parse', '--is-shallow-repository']).trim();
} catch { /* old git: assume full */ }
if (shallow === 'true') {
  prerequisite(
    'the clone is shallow, so no merge-base with upstream can be computed',
    'actions/checkout needs fetch-depth: 0.',
  );
}

let base = '';
try {
  base = git(['merge-base', 'HEAD', 'upstream/main']).trim();
} catch {
  prerequisite(
    'no upstream/main ref',
    'git remote add upstream https://github.com/CyberTimon/RapidRAW.git && git fetch upstream',
  );
}
if (!base) {
  prerequisite('merge-base with upstream/main is empty', 'Fetch upstream: git fetch upstream');
}

const errors = [];
const warnings = [];
const notes = [];

// --- One pass over the real diff ---------------------------------------------
//
// Deliberately NOT `-w`: git merge does not ignore whitespace, so a number that
// does is not the number to look at. Our own paths are left out by git itself;
// the accounting (scripts/upstream-diff.mjs) is tested against throwaway
// repositories.
const excluded = [...OURS, ...IDENTITY].map((p) => `:(exclude)${p}`);
const stats = accountDiff(git(['diff', base, '--', '.', ...excluded]), {
  isOurs, skip: IDENTITY, isHook,
});

const touched = [];
for (const [file, st] of stats) {
  // Borrowed lines are upstream's own fix carried early. They are not our
  // divergence: when upstream merges the pull request they become identical to
  // their copy, so they count for nothing.
  touched.push({ file, ...st, added: st.added - st.borrowed, adopted: isOursToChange(file) });
}
// Theirs: merged once per release, under the ratchet. Adopted: the render path
// and today's interface, ours to change.
const theirs = touched.filter((t) => !t.adopted);
const adopted = touched.filter((t) => t.adopted);

// --- Gate: every file from RapidRAW we touch must be claimed -----------------
//
// Line counts do not prove behaviour was preserved. `get_all_adjustments_from_json`
// gained a fifth parameter and five call sites across four of their files were
// rewritten one line for one: nothing added, nothing removed, and the whole
// export path depending on a signature upstream owns. So if we changed a file
// that came from them, render path and interface included, some entry in
// scripts/upstream-registry.mjs must name it: that is what the release review
// reads their changes against.
const index = dependencyIndex(REGISTRY);
for (const t of touched) {
  if (t.added === 0 && t.deleted === 0 && t.borrowed === 0) continue;
  if (index.claims(t.file)) continue;
  errors.push({
    file: t.file,
    detail: 'we changed this file of theirs and no registry entry claims it',
    fix: 'Name it in the dependsOn of whichever feature needs it, in '
      + 'scripts/upstream-registry.mjs, with a how and a note. If nothing needs '
      + 'it, revert the change.',
  });
}

// --- Gate: the ratchet ---------------------------------------------------------
//
// Our lines in each file of theirs outside the render path, against the
// committed baseline. Up fails; down is written back (scripts/upstream-ratchet.mjs).
const current = new Map(theirs.filter((t) => t.added > 0).map((t) => [t.file, t.added]));
const baselinePath = join(root, BASELINE);
const baseline = existsSync(baselinePath)
  ? parseBaseline(readFileSync(baselinePath, 'utf8'))
  : null;

let previous = null;
try {
  const startedFrom = git(['merge-base', 'HEAD', 'origin/main']).trim();
  previous = parseBaseline(readBlobs(git, [`${startedFrom}:${BASELINE}`]).get(`${startedFrom}:${BASELINE}`));
} catch {
  warnings.push(`no origin/main to compare ${BASELINE} with, so a number raised by hand would go unnoticed in this run`);
}

let lowerTo = null;
if (baseline === null) {
  if (inCI) {
    errors.push({
      file: BASELINE,
      detail: 'the ratchet baseline is missing',
      fix: 'Run npm run check:merge locally, which writes it from the tree, and commit it.',
    });
  } else {
    lowerTo = current;
    notes.push(`${BASELINE} was missing; written from the tree`);
  }
} else {
  const { rises, lowered, raised } = compareRatchet(current, baseline, previous);
  for (const r of rises) {
    errors.push({
      file: r.file,
      detail: r.allowed === 0
        ? `${r.now} of our lines in a file of theirs that had none`
        : `${r.now} of our lines, up from ${r.allowed}`,
      fix: 'Our lines in their files only go down. Move the new code into a file of ours '
        + '(the engine, mods/, src/argentum/) and leave at most the line that was there.',
    });
  }
  for (const r of raised) {
    errors.push({
      file: BASELINE,
      detail: `${r.file} is allowed ${r.to}, and was allowed ${r.from} on main`,
      fix: 'The baseline is only ever lowered. Put the number back.',
    });
  }
  if (lowered.length > 0) {
    if (inCI) {
      errors.push({
        file: BASELINE,
        detail: `${lowered.length} file${lowered.length === 1 ? ' is' : 's are'} below the baseline: `
          + lowered.map((l) => `${l.file} ${l.from} -> ${l.to}`).join(', '),
        fix: 'Run npm run check:merge locally, which lowers the numbers, and commit the file.',
      });
    } else {
      lowerTo = new Map([...baseline].map(([file, n]) => [file, Math.min(n, current.get(file) ?? 0)]));
      for (const l of lowered) notes.push(`ratchet: ${l.file} ${l.from} -> ${l.to}`);
    }
  }
}

// --- Gate: anchors -------------------------------------------------------------
const anchorFor = (file) => ANCHORS.find((a) => a.file === file);
for (const t of theirs) {
  if (t.hooks.length === 0) continue;
  const anchor = anchorFor(t.file);
  const allowed = anchor?.hooks ?? 0;
  if (t.hooks.length > allowed) {
    errors.push({
      file: t.file,
      detail: `${t.hooks.length} calls into our code, the anchor allows ${allowed}`,
      fix: anchor?.hooks
        ? `${anchor.what} — do this instead: ${anchor.instead}`
        : 'This file has no anchor at all, and should have none.',
      lines: t.hooks,
    });
  }
}

// --- Gate: the wiring an anchor exists for -------------------------------------
//
// An anchor says how many lines of theirs may mention us. It says nothing about
// whether those lines still do their job: an import and a hook can survive while
// the one dependency entry that makes them work is dropped as unused. Only our
// own identifiers are asserted, so upstream may rename anything of theirs.
//
// A tree without the file is not a tree that lost the wiring (the fixture
// repositories in the e2e tests hold two files). A typo in `anchor.file` is
// caught from the other side: test-upstream-checks.mjs reads every guarded path
// unguarded.
for (const anchor of ANCHORS) {
  if (!anchor.requires) continue;
  const full = join(root, anchor.file);
  if (!existsSync(full)) continue;
  const text = readFileSync(full, 'utf8');
  for (const need of anchor.requires) {
    if (!need.pattern.test(text)) {
      errors.push({
        file: anchor.file,
        detail: `lost the wiring its anchor exists for: ${need.why}`,
        fix:
          `Restore the line matching ${need.pattern}. If it is genuinely no longer `
          + 'needed, remove it from `requires` in the same commit and say why in the '
          + 'registry entry.',
      });
    }
  }
}

// --- Gate: registry integrity ----------------------------------------------------
//
// The tree and the registry have to agree about borrowed fixes, so that deleting
// a `// upstream #1307` marker cannot quietly delete the reason anyone had to
// think about it.
{
  const marks = borrowMarkers(git);
  const registered = borrowsOf(REGISTRY);
  for (const { file, pr } of marks.pairs) {
    if (registered.some((b) => b.file === file && b.pr === pr)) continue;
    errors.push({
      file,
      detail: `carries a // upstream #${pr} marker that no registry entry declares`,
      fix: 'Add a borrowed-fix entry in scripts/upstream-registry.mjs, or remove the marker.',
    });
  }
  const live = new Set(activeFor(REGISTRY, REVIEWS, null).map((e) => e.id));
  for (const { entry, file, pr } of registered) {
    if (entry.retired && !live.has(entry.id)) continue;
    if (marks.pairs.some((m) => m.file === file && m.pr === pr)) continue;
    errors.push({
      file: 'scripts/upstream-registry.mjs',
      detail: `${entry.id} claims a // upstream #${pr} marker in ${file}, and there is none`,
      fix: 'If upstream merged it and the block is gone, retire the entry with '
        + 'retired: { recordedIn, why } in the release review that says so.',
    });
  }
}

// The inventory is append-only. Deleting an entry and its markers in one commit
// leaves nothing to disagree about, so every id this file has ever held, read
// back out of our own history, must still be here.
{
  const ever = historicalIds(git);
  if (ever === null) {
    warnings.push('cannot read the history of scripts/upstream-registry.mjs, so '
      + 'a deleted entry would go unnoticed in this run');
  } else {
    const now = new Set(REGISTRY.map((e) => e.id));
    for (const id of ever) {
      if (now.has(id)) continue;
      errors.push({
        file: 'scripts/upstream-registry.mjs',
        detail: `${id} was in the inventory and has been deleted from it`,
        fix: 'Entries are never deleted. Put it back and retire it with '
          + 'retired: { recordedIn, why }, listed in that release review\'s retires.',
      });
    }
  }
}

// A retirement is how an entry stops being checked, so it is the obvious thing
// to fake: it names a review that exists, and that review records it.
for (const problem of validateRetirements(REGISTRY, REVIEWS)) {
  errors.push({
    file: 'scripts/upstream-registry.mjs',
    detail: `${problem.id} ${problem.detail}`,
    fix: problem.fix,
  });
}

// --- Gate: the release reviews ---------------------------------------------------
//
// `git merge` answers "do these texts collide". It cannot answer whether
// upstream has built, moved or renamed the thing we built around; those changes
// conflict with nothing. So upstream is merged once per release, after a review
// recorded in scripts/upstream-decisions.mjs, and the merge cannot run ahead of
// the reviews.
const allTags = tags(git);
const newest = REVIEWS[REVIEWS.length - 1];
let rangeCommits = null;
if (isRelease(newest)) {
  try {
    rangeCommits = commitsIn(git, newest.from, newest.to);
  } catch {
    errors.push({
      file: 'scripts/upstream-decisions.mjs',
      detail: `cannot read ${short(newest.from)}..${short(newest.to)}, the newest release review's range`,
      fix: 'git fetch upstream. In CI this means fetch-depth: 0 and the upstream fetch step.',
    });
  }
}
{
  const found = validateReviews(REVIEWS, {
    tags: allTags,
    commits: rangeCommits,
    exists: (path) => existsSync(join(root, path)),
    isOurs,
    isOursToChange,
  });
  for (const p of found.errors) errors.push({ file: 'scripts/upstream-decisions.mjs', ...p });
  for (const w of found.warnings) warnings.push(`scripts/upstream-decisions.mjs: ${w.detail}`);
}

const reviewed = reviewedTo();
const tagFor = (sha) => [...allTags].find(([name, at]) => at === sha && /^v\d/.test(name))?.[0];
if (reviewed !== base) {
  errors.push({
    file: 'scripts/upstream-decisions.mjs',
    detail: `reviewed up to ${short(reviewed)}, and upstream is merged in up to ${short(base)}`
      + (tagFor(base) ? ` (${tagFor(base)})` : ' (not a release)'),
    fix: 'Upstream is merged once per release, after its review: npm run review:upstream, '
      + 'then add the release review its template prints. Merge the release tag, not their main.',
  });
}

// CHANGELOG.md is documentation of the register, checked against it.
{
  const changelog = join(root, 'CHANGELOG.md');
  if (existsSync(changelog)) {
    const text = readFileSync(changelog, 'utf8');
    const recorded = text.match(/Based on RapidRAW\s*`v?([^`]+)`\s*@\s*`([0-9a-f]{7,40})`/);
    const version = isRelease(newest) ? newest.release.replace(/^v/, '') : null;
    if (!recorded) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: 'no "Based on RapidRAW `x.y.z` @ `sha`" line found',
        fix: 'Record the upstream release this fork is reviewed and merged up to.',
      });
    } else if (!reviewed.startsWith(recorded[2]) || (version && recorded[1] !== version)) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: `says ${recorded[1]} @ ${recorded[2]}, the review register says `
          + `${version ?? '?'} @ ${short(reviewed)}`,
        fix: 'The register is the source. Make this line agree with it.',
      });
    }
  }
}

// --- Gate: what we step around must still be there ------------------------------
//
// A shadow is their code left in place while ours runs instead. If the release
// we merged no longer has the symbol, our hook may be bypassing nothing. Their
// main is looked at too, as an early warning for the next release. The render
// path and today's interface are ours, and their changes there are never
// merged, so nothing of theirs is shadowed in them.
{
  const shadows = shadowsOf(REGISTRY).filter((s) => !isOursToChange(s.file));
  const specs = [...new Set(shadows.flatMap((s) => [`${base}:${s.file}`, `upstream/main:${s.file}`]))];
  const blobs = readBlobs(git, specs);
  for (const { entry, file, symbol } of shadows) {
    const merged = blobs.get(`${base}:${file}`);
    const next = blobs.get(`upstream/main:${file}`);
    if (merged === null) {
      errors.push({
        file,
        detail: `${entry.id} steps around ${symbol} here, and the release we merged no longer has this file`,
        fix: 'Our hook may now be bypassing nothing. Find what replaced it.',
      });
    } else if (!merged.includes(symbol)) {
      errors.push({
        file,
        detail: `${symbol} is gone from the release we merged, and ${entry.id} steps around it`,
        fix: 'A rename is exactly the change that would otherwise slip through - check the hook still bypasses what we think it does.',
      });
    } else if (next === null || !next.includes(symbol)) {
      warnings.push(`${file}: ${symbol}, which ${entry.id} steps around, is gone from upstream/main - the next release review needs to look`);
    }
  }
}

// --- Warnings: is upstream current, and is a release waiting? ---------------------
try {
  const when = Number(git(['log', '-1', '--format=%ct', 'upstream/main']).trim());
  const days = Math.floor((Date.now() / 1000 - when) / 86400);
  if (days > 14) {
    warnings.push(`upstream/main is ${days} days old, so a new release would go unnoticed. Run: git fetch upstream`);
  }
} catch { /* no upstream ref: handled above */ }

const versionOf = (name) => name.slice(1).split('.').map(Number);
const newer = (a, b) => {
  const [x, y] = [versionOf(a), versionOf(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  }
  return false;
};
const releaseTags = [...allTags.keys()].filter((n) => /^v\d+(\.\d+)*$/.test(n));
const reviewedName = isRelease(newest) ? newest.release : tagFor(reviewed);
const waiting = reviewedName ? releaseTags.filter((n) => newer(n, reviewedName)) : [];
if (waiting.length > 0) {
  warnings.push(`RapidRAW ${waiting.sort((a, b) => (newer(a, b) ? 1 : -1)).join(', ')} `
    + `${waiting.length === 1 ? 'is' : 'are'} out and not reviewed - npm run review:upstream`);
}

// --- Report -----------------------------------------------------------------------
const seconds = ((Date.now() - started) / 1000).toFixed(1);
if (errors.length > 0) {
  console.error('\n  UPSTREAM CHECK FAILED\n');
  for (const e of errors) {
    console.error(`    ${e.file}`);
    console.error(`      ${e.detail}`);
    if (e.lines) for (const l of e.lines) console.error(`        + ${l}`);
    console.error(`      ${e.fix}\n`);
  }
  if (lowerTo) console.error('  (The ratchet will be lowered once everything above passes.)\n');
  console.error('  See docs/ARCHITECTURE.md, "Staying mergeable".');
  console.error(`  check:merge ${seconds} s, ${gitCalls} git calls\n`);
  process.exit(1);
}

if (lowerTo) {
  writeFileSync(baselinePath, formatBaseline(lowerTo));
  notes.push(`${BASELINE} updated - commit it`);
}

const sum = (list, key) => list.reduce((n, t) => n + t[key], 0);
const used = theirs.filter((t) => t.added > 0 || t.deleted > 0 || t.borrowed > 0);
const hooked = theirs.filter((t) => t.hooks.length > 0);
console.log(`  upstream: merged and reviewed up to ${reviewedName ?? short(reviewed)} (${short(reviewed)})`);
console.log(`  their files: ${used.length} touched, ${sum(used, 'added')} of our lines, none above the ratchet`);
console.log(`  render path and interface: ${sum(adopted, 'added')} of our lines in ${adopted.length} files - ours to change, not counted`);
console.log(`  anchors: ${hooked.reduce((n, t) => n + t.hooks.length, 0)} calls into our code across ${hooked.length} of their files`);
console.log(`  their lines: ${sum(used, 'removed')} removed unreplaced, ${sum(used, 'replaced')} replaced in place - counts, not evidence`
  + (verbose ? '' : ' (--verbose lists them)'));
if (verbose) {
  for (const t of used.filter((x) => x.removed > 0)) {
    console.log(`    ${t.file}: ${t.removed} removed unreplaced, ${t.replaced} replaced`);
  }
}
const borrowed = used.filter((t) => t.borrowed > 0);
if (borrowed.length > 0) {
  console.log(`  borrowed from upstream, pending their merge (${borrowed.length}):`);
  for (const b of borrowed) {
    console.log(`    ${b.file}  ${b.borrowed} lines, ${[...b.prs].map((n) => `#${n}`).join(', ')}`);
  }
}
for (const n of notes) console.log(`  ${n}`);
for (const w of warnings) console.log(`  warning: ${w}`);
console.log(`  check:merge ${seconds} s, ${gitCalls} git calls`);
process.exit(0);
