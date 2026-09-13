// Which upstream commits land on something Argentum depends on.
//
// WHY THIS EXISTS
//
// `git merge` answers "do these texts collide". It cannot answer the question a
// fork lives or dies on: **has upstream now built, moved or renamed the thing we
// built around?** Those changes conflict with nothing. They merge in silence and
// leave our hook bypassing code that no longer does what we thought.
//
// The dependencies come from scripts/upstream-registry.mjs — every feature,
// borrowed fix and deliberate behaviour change, with what of theirs it rests on.
// Overlaps are keyed to the exact upstream commit that caused them, and
// scripts/upstream-decisions.mjs records what was decided about each.
//
// TWO KINDS, AND ONLY ONE OF THEM IS DETECTION
//
//   dep      a commit touches a file, symbol or key some entry registers. This
//            is mechanical and reliable: the file is named, the diff is read.
//   feature  a commit's subject matches an entry's keywords while touching none
//            of its files. This is a HINT. It fires on the word, not on the
//            behaviour, and a commit called "improve colour handling" matches
//            nothing at all. It is gated anyway, because a false positive costs
//            one line of reasoning and a false negative costs the feature.
//
// Neither finds a feature upstream built somewhere we have never looked. That is
// what the featureReview block on every review entry is for, and nothing
// verifies that sentence beyond requiring someone to write it.

export { REGISTRY, activeFor, borrowsOf, dependencyIndex, shadowsOf } from './upstream-registry.mjs';

/** The gated kinds: both require a recorded decision. */
export const GATED = ['dep', 'feature'];

/**
 * Files carrying a `// upstream #NNNN` marker in the working tree, and the PR
 * numbers in them.
 *
 * Used for integrity, not for detection. A borrowed fix is upstream's own work
 * carried early; two things can happen to it, and only one of them names a
 * number. Upstream 8737fc4e rewrote the hashing module one of ours sits inside
 * and mentioned no pull request at all, because most RapidRAW commits are the
 * maintainer's own. Watching the registered *file* is what catches that.
 */
export function borrowMarkers(run) {
  let out = '';
  try {
    out = run('git grep -nE "// upstream #[0-9]+" -- src src-tauri');
  } catch {
    return { files: [], prs: [], pairs: [] };
  }
  const pairs = [];
  for (const line of out.split('\n')) {
    const m = line.match(/^([^:]+):\d+:.*\/\/\s*upstream #(\d+)/);
    if (!m) continue;
    pairs.push({ file: m[1], pr: m[2] });
  }
  return {
    files: [...new Set(pairs.map((p) => p.file))],
    prs: [...new Set(pairs.map((p) => p.pr))],
    pairs,
  };
}

/**
 * For each borrowed pull request, the upstream commits in this range that name
 * it — which is weak evidence, and is labelled as such where it is printed.
 *
 * Reported per pull request. The first version asked whether *any* borrowed PR
 * had landed and, if one had, printed nothing about the others: one landing
 * silenced the rest.
 */
export function borrowStatus(run, from, to, prs) {
  return prs.map((pr) => {
    const landed = run(`git log --format=%h ${from}..${to} --grep="#${pr}"`).trim();
    return { pr, landedIn: landed ? landed.split('\n') : [] };
  });
}

const CALL = /\s*\(/.source;
const DEF = /(fn|function)\s+/.source;

/** A call or a definition, not a name inside a `use` list. */
const mentionsSymbol = (patch, symbol) => new RegExp(
  '(' + DEF + '|[^A-Za-z0-9_])' + symbol + CALL,
).test(patch);

/**
 * Every overlap between `from..to` of upstream and the registry as it stands.
 *
 * `run` executes a git command in the repository and returns stdout.
 */
export function detectOverlaps(run, from, to, { entries, index }) {
  const log = run(`git log --format="%H%x09%s" ${from}..${to}`).trim();
  if (!log) return [];
  const overlaps = [];

  for (const line of log.split('\n')) {
    const [sha, ...rest] = line.split('\t');
    const subject = rest.join('\t');
    const short = sha.slice(0, 8);
    const files = run(`git show --name-only --format= ${sha}`).trim().split('\n').filter(Boolean);
    let patch = '';
    try {
      patch = run(`git show --format= ${sha}`, true);
    } catch { /* enormous commit: filenames only, and the dep check still works */ }

    const hitEntries = new Set();
    const add = (kind, target, detail, entryId) => {
      overlaps.push({
        key: `${short}:${kind}:${target}`,
        commit: short,
        subject,
        kind,
        target,
        entryId,
        detail,
        gated: GATED.includes(kind),
      });
    };

    for (const [file, claims] of index.byFile) {
      const touched = files.includes(file);
      for (const { entry, dep } of claims) {
        const bySymbol = !touched && dep.symbol && mentionsSymbol(patch, dep.symbol);
        const byKey = !touched && dep.key && patch.includes(dep.key);
        if (!touched && !bySymbol && !byKey) continue;
        hitEntries.add(entry.id);
        const what = dep.symbol ? `${file}#${dep.symbol}` : dep.key ? `${file}:${dep.key}` : file;
        add('dep', `${entry.id}:${what}`,
          `${entry.id} (${entry.kind}) ${dep.how} this. ${dep.note ?? ''}`.trim(),
          entry.id);
      }
    }

    for (const entry of entries) {
      if (hitEntries.has(entry.id)) continue;
      if (!entry.keywords?.test(subject)) continue;
      add('feature', entry.id,
        `the subject reads like ${entry.id} (${entry.what}) but touches none of its files. `
        + 'Either it is unrelated, or upstream has built this independently — which is the '
        + 'case a file match can never see. Say which.',
        entry.id);
    }
  }
  return overlaps;
}
