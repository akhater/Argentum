// The release review: grouping a release's commits into features, and holding
// the review register to its rules.
//
// Pure: commits, tags and file facts in, problems out. check-mergeability.mjs
// asks git once for each fact; the tests hand these functions throwaway data.
// The rules themselves are written out at the top of scripts/upstream-decisions.mjs.

import {
  FEATURE_VERDICTS, PER_COMMIT_ENDS, SAME_AS_OURS, VERDICTS, isRelease,
} from './upstream-decisions.mjs';

const isShortSha = (s) => typeof s === 'string' && /^[0-9a-f]{7,40}$/.test(s);
const isFullSha = (s) => typeof s === 'string' && /^[0-9a-f]{40}$/.test(s);
const said = (s, min) => typeof s === 'string' && s.trim().length >= min;
const short = (sha) => String(sha).slice(0, 8);

/**
 * A release's commits, grouped the way they arrived on upstream's main line:
 * one group per pull request merged into it, one per commit made on it directly.
 * Oldest first, merge commits left out (their branch's commits carry the files).
 *
 * `commits` is what upstream-git.mjs `commitsIn(from, to)` returns. A group is
 * { what, pr, commits: [commit, ...] }. It is a starting point for the review's
 * features, not a verdict: the reviewer joins and splits lines as the features
 * really are.
 */
export function featureGroups(commits, to) {
  const bySha = new Map(commits.map((c) => [c.sha, c]));
  const order = new Map(commits.map((c, i) => [c.sha, i]));

  const mainline = [];
  for (let at = to; bySha.has(at); at = bySha.get(at).parents[0]) mainline.push(at);
  mainline.reverse();

  // Everything reachable from the main line so far. Walking it oldest first, a
  // merge brings in what its other parents reach that the main line did not.
  const seen = new Set();
  const reach = (start) => {
    const found = [];
    const stack = [start];
    while (stack.length > 0) {
      const sha = stack.pop();
      if (!bySha.has(sha) || seen.has(sha)) continue;
      seen.add(sha);
      found.push(bySha.get(sha));
      stack.push(...bySha.get(sha).parents);
    }
    return found;
  };

  const groups = [];
  for (const sha of mainline) {
    const commit = bySha.get(sha);
    seen.add(sha);
    if (commit.parents.length < 2) {
      groups.push({ what: commit.subject, pr: null, commits: [commit] });
      continue;
    }
    const members = commit.parents.slice(1).flatMap(reach)
      .filter((c) => c.parents.length < 2)
      .sort((a, b) => order.get(a.sha) - order.get(b.sha));
    if (members.length === 0) continue;
    const pr = commit.subject.match(/#(\d+)/)?.[1] ?? null;
    const title = commit.body.split('\n').find((l) => l.trim())?.trim() ?? commit.subject;
    groups.push({ what: pr ? `${title} (#${pr})` : title, pr, commits: members });
  }

  // Commits the main-line walk did not reach, if `to` was not their tip.
  for (const commit of commits) {
    if (seen.has(commit.sha) || commit.parents.length > 1) continue;
    groups.push({ what: commit.subject, pr: null, commits: [commit] });
  }
  return groups;
}

/**
 * Problems with the review register: { errors, warnings }, each { detail, fix }.
 *
 * `facts`:
 *   tags          Map tag -> commit sha
 *   commits       the commits of the newest release range, or null if it was not read
 *   exists(path)  is this file in the working tree
 *   isOurs(path)  is this path one of ours
 *   isRenderPath(path)
 *
 * Every entry is checked for shape. Only the newest release entry is checked
 * against git and the tree: older ones are records of what was true then, and a
 * file they copied into may since have moved.
 */
export function validateReviews(reviews, facts) {
  const errors = [];
  const warnings = [];
  const fail = (detail, fix) => errors.push({ detail, fix });

  // --- The per-commit history: shape only, and closed. --------------------------
  const perCommitEnd = reviews.findIndex((r) => !isRelease(r) && r.through === PER_COMMIT_ENDS);
  reviews.forEach((review, i) => {
    if (isRelease(review)) return;
    const name = short(review.through);
    if (perCommitEnd === -1 || i > perCommitEnd) {
      fail(`a per-commit review (${name}) after the last one, ${short(PER_COMMIT_ENDS)}`,
        'Per-commit review ended on 2026-10-18. Record a release review instead: npm run review:upstream.');
      return;
    }
    for (const d of review.decisions ?? []) {
      if (typeof d.overlap !== 'string' || !VERDICTS.includes(d.verdict) || !said(d.why, 20)) {
        fail(`${name}: the recorded decision ${d.overlap} has no valid verdict or no reasoning`,
          `Each decision is { overlap, verdict: ${VERDICTS.join(' | ')}, why }. These are history: restore it.`);
      }
    }
    if (i === 0) return;
    const fr = review.featureReview;
    if (!fr || !['none', 'overlap-found'].includes(fr.verdict) || !said(fr.why, 40)) {
      fail(`${name}: the recorded review has no valid featureReview`,
        'These are history: restore it as it was recorded.');
    }
  });

  // --- The release reviews -------------------------------------------------------
  const releases = reviews.map((r, i) => ({ r, i })).filter(({ r }) => isRelease(r));
  const names = new Set();
  let previousLive = null;
  releases.forEach(({ r, i }, n) => {
    const name = r.release || `release review ${n + 1}`;
    const newest = n === releases.length - 1;

    if (names.has(r.release)) fail(`${name} is reviewed twice`, 'One review per release.');
    names.add(r.release);

    if (!isFullSha(r.from) || !isFullSha(r.to)) {
      fail(`${name}: from and to must be full 40-character shas`,
        'npm run review:upstream prints them.');
    }
    const before = reviews[i - 1];
    const ended = before ? (isRelease(before) ? before.to : before.through) : null;
    if (ended && r.from !== ended) {
      fail(`${name}: starts at ${short(r.from)}, and the review before it ended at ${short(ended)}`,
        'The reviews form one unbroken chain: from is where the previous review ended.');
    }
    const tagged = facts.tags.get(r.release);
    if (tagged === undefined) {
      fail(`${name}: no tag of that name here`,
        'git fetch upstream (in CI the workflow fetches their tags), or correct the release name.');
    } else if (tagged !== r.to) {
      fail(`${name}: names ${short(tagged)}, and the review says to: ${short(r.to)}`,
        'to is the commit the release tag names.');
    }
    if (typeof r.on !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(r.on)) {
      fail(`${name}: on must be the review date, YYYY-MM-DD`, 'The day the review was done.');
    }
    if (typeof r.hours !== 'number' || !(r.hours > 0)) {
      fail(`${name}: hours must say how long the review took`, 'A number of hours, more than zero.');
    } else if (r.hours > 8) {
      warnings.push({ detail: `${name} took ${r.hours} hours; a release review is meant to take under a day` });
    }

    // Features.
    const features = Array.isArray(r.features) ? r.features : [];
    if (features.length === 0) {
      fail(`${name}: no features`, 'One line per feature of the release, from the template.');
    }
    for (const f of features) {
      const label = `${name}: feature "${String(f.what ?? '').slice(0, 60)}"`;
      if (!said(f.what, 3)) fail(`${name}: a feature with no what`, 'Say what the feature is.');
      if (!Array.isArray(f.commits) || f.commits.length === 0 || !f.commits.every(isShortSha)) {
        fail(`${label} names no commits`, 'commits: the short shas of the commits that carry it.');
      }
      if (!FEATURE_VERDICTS.includes(f.verdict)) {
        fail(`${label} has verdict "${f.verdict}"`, `One of: ${FEATURE_VERDICTS.join(', ')}.`);
      }
      if (!said(f.why, 20)) {
        fail(`${label} has no reasoning`,
          'A verdict with no why is a rubber stamp. Say what you read and what you concluded.');
      }
      const targets = f.copiedInto === undefined ? [] : [f.copiedInto].flat();
      if (f.verdict === 'copy' && targets.length === 0) {
        fail(`${label} is copied, and copiedInto does not say where`,
          'copiedInto: the file of ours it was copied into.');
      }
      for (const target of targets) {
        if (typeof target !== 'string' || !(facts.isOurs(target) || facts.isRenderPath(target))) {
          fail(`${label} is copied into ${target}, which is not a file of ours`,
            'A copy goes into our code: the engine, mods/, src/argentum/ or the render path.');
        } else if (newest && !facts.exists(target)) {
          fail(`${label} is copied into ${target}, which does not exist`, 'Name the file it is in.');
        }
      }
    }

    // They built something we have.
    const same = r.sameAsOurs;
    const decided = features.some((f) => f.verdict === 'decide');
    if (!same || !SAME_AS_OURS.includes(same.verdict) || !said(same.why, 40)) {
      fail(`${name}: sameAsOurs must be { verdict: ${SAME_AS_OURS.join(' | ')}, why }`,
        'Read the release for features Argentum already has, and say what you read and concluded. '
          + 'No detector does this part.');
    } else if ((same.verdict === 'found') !== decided) {
      fail(same.verdict === 'found'
        ? `${name}: sameAsOurs is found, and no feature is decide`
        : `${name}: a feature is decide, and sameAsOurs says none`,
      'decide is the verdict for something they built that we have; sameAsOurs says whether there was one.');
    }

    // Their files still on the preview, export and mask path.
    if (!Array.isArray(r.stillLive) || !r.stillLive.every((p) => typeof p === 'string')) {
      fail(`${name}: stillLive must list their files still used when previewing, exporting or masking`,
        'Paths, one per file. An empty list is the goal.');
    } else {
      if (new Set(r.stillLive).size !== r.stillLive.length) {
        fail(`${name}: stillLive lists a file twice`, 'One path per file.');
      }
      for (const path of r.stillLive) {
        if (facts.isOurs(path)) {
          fail(`${name}: stillLive lists ${path}, which is ours`, 'stillLive counts their files only.');
        } else if (newest && !facts.exists(path)) {
          fail(`${name}: stillLive lists ${path}, which is not in the tree`,
            'A file that is gone is no longer live. Take it off the list.');
        }
      }
      if (previousLive !== null && r.stillLive.length > previousLive.count) {
        fail(`${name}: ${r.stillLive.length} of their files still live, up from ${previousLive.count} at ${previousLive.name}`,
          'This number only goes down. Move what was added onto our side, or into the engine.');
      }
      previousLive = { count: r.stillLive.length, name };
    }

    if (r.retires !== undefined
      && (!Array.isArray(r.retires) || !r.retires.every((id) => typeof id === 'string'))) {
      fail(`${name}: retires must be a list of registry ids`, 'retires: [\'entry-id\', ...]');
    }

    // The newest range, against git.
    if (!newest || !facts.commits) return;
    const inRange = facts.commits.filter((c) => c.parents.length < 2);
    const owners = new Map(inRange.map((c) => [c.sha, []]));
    for (const f of features) {
      for (const ref of f.commits ?? []) {
        const hits = inRange.filter((c) => c.sha.startsWith(ref));
        if (hits.length !== 1) {
          fail(`${name}: feature "${String(f.what).slice(0, 60)}" names ${ref}, `
            + (hits.length === 0 ? `which is not a commit in ${short(r.from)}..${short(r.to)}` : 'which is ambiguous'),
          'List the short shas npm run review:upstream printed.');
          continue;
        }
        owners.get(hits[0].sha).push(f);
      }
    }
    for (const c of inRange) {
      const fs = owners.get(c.sha);
      if (fs.length === 0) {
        fail(`${name}: ${short(c.sha)} "${c.subject}" is in no feature`,
          'Every commit of the release is read. Put it in the feature it belongs to, or give it its own line.');
        continue;
      }
      const render = c.files.filter(facts.isRenderPath);
      if (render.length > 0 && fs.every((f) => f.verdict === 'take')) {
        fail(`${name}: ${short(c.sha)} changes ${render.join(', ')}, and every feature it is in is take`,
          'The render path is ours: their changes to it are copied by hand or skipped, never merged. '
            + 'Give its render-path part a line of its own with copy, skip or decide.');
      }
    }
  });

  return { errors, warnings };
}
