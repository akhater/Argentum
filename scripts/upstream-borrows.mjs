// Upstream's own fixes, carried early, and whether upstream has landed them.
//
// A borrowed fix is a block of theirs pasted into a file of theirs before they
// merge it, between `// upstream #NNNN` and `// end upstream #NNNN`. Two things
// can happen to it upstream, and only one of them names a number: most RapidRAW
// commits are the maintainer's own and mention no pull request at all. Upstream
// 8737fc4e rewrote the hashing module one of ours sits inside without naming
// one. So the markers are used for integrity (the tree and the registry must
// agree), and the release review lists their changes to the registered file.

/**
 * Files carrying a `// upstream #NNNN` marker in the working tree, and the PR
 * numbers in them. One `git grep`.
 */
export function borrowMarkers(run) {
  let out = '';
  try {
    out = run(['grep', '-nE', '// upstream #[0-9]+', '--', 'src', 'src-tauri']);
  } catch {
    // git grep exits 1 when nothing matches.
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
 * For each borrowed pull request, the upstream commits in `commits` whose
 * message names it - weak evidence, and labelled as such where it is printed.
 *
 * Reported per pull request. The first version asked whether *any* borrowed PR
 * had landed and, if one had, printed nothing about the others: one landing
 * silenced the rest.
 */
export function borrowStatus(commits, prs) {
  return prs.map((pr) => {
    const names = new RegExp(`#${pr}\\b`);
    const landedIn = commits
      .filter((c) => names.test(`${c.subject}\n${c.body}`))
      .map((c) => c.sha.slice(0, 8));
    return { pr, landedIn };
  });
}
