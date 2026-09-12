// Reading a unified diff the way `git merge` reads it.
//
// WHY THIS IS ITS OWN FILE
//
// The accounting used to live inline in check-mergeability.mjs and could not be
// tested without the real repository, so two bugs sat in it for a day:
//
//   * `^\+\+\+ b/(.+)$` does not match a deleted file, which git writes as
//     `+++ /dev/null`. Deleting all 257 lines of an upstream component changed
//     the checker's output by nothing at all; deleting a 1021-line one reported
//     the deletions against whichever file happened to precede it in the diff.
//   * Every `-` line counted the same. A rename replaces a line and leaves
//     nothing of theirs missing; removing a block leaves a hole an upstream
//     edit lands in. Only the second one conflicts, and only the second one is
//     what CLAUDE.md means by "never delete their function".
//
// So this file is pure — text in, numbers out — and scripts/test-upstream-checks.mjs
// drives it with real git output from throwaway repositories.

/** `// upstream #1234` … `// end upstream #1234` — their fix, carried early. */
export const BORROW_START = /\/\/\s*upstream #(\d+)/;
export const BORROW_END = /\/\/\s*end upstream #(\d+)/;

/**
 * Count a diff per file.
 *
 * Returns Map<file, {
 *   added,     lines we added that are ours
 *   borrowed,  lines we added that are upstream's own fix, carried early
 *   deleted,   every `-` line, for the record
 *   removed,   `-` lines that were NOT replaced — the ones that conflict
 *   replaced,  `-` lines that were, one for one, on the line below
 *   borrowedRemoved, removals made while adopting a borrowed fix
 *   hooks[],   added lines that call into our code
 *   prs        borrowed PR numbers seen in this file
 * }>
 *
 * `removed` is the number the gate uses. Within one run of changed lines —
 * a maximal group of `-` and `+` between context lines — min(dels, adds) is a
 * replacement and the excess is a removal. That is not a heuristic standing in
 * for judgement: it is the difference between a line of theirs that still
 * exists under a new spelling and a line of theirs that is gone.
 */
export function accountDiff(diff, { isOurs = () => false, skip = [], isHook = () => false } = {}) {
  const stats = new Map();
  const statFor = (file) => {
    if (!stats.has(file)) {
      stats.set(file, {
        added: 0, borrowed: 0, deleted: 0, removed: 0, replaced: 0,
        borrowedRemoved: 0, hooks: [], prs: new Set(),
      });
    }
    return stats.get(file);
  };

  let current = null;
  let pendingDeletePath = null;
  let borrowing = null;
  // `---`/`+++` are only file headers between `diff --git` and the first hunk.
  // A deleted line whose text begins with `-- ` looks exactly like one otherwise.
  let inHeader = false;

  // One group of changed lines: deletions, additions, and whether any of the
  // additions were upstream's own fix.
  let group = { dels: 0, adds: 0, borrowedAdds: 0 };
  const closeGroup = () => {
    if (current && (group.dels > 0 || group.adds > 0)) {
      const st = statFor(current);
      const replaced = Math.min(group.dels, group.adds);
      const removed = group.dels - replaced;
      st.replaced += replaced;
      // Deletions made while pasting in a fix of theirs are not our divergence
      // for the same reason the additions are not: when upstream merges the
      // pull request, both sides become their text.
      if (group.borrowedAdds > 0) st.borrowedRemoved += removed;
      else st.removed += removed;
    }
    group = { dels: 0, adds: 0, borrowedAdds: 0 };
  };

  for (const line of diff.split('\n')) {
    if (line.startsWith('diff --git ')) {
      closeGroup();
      inHeader = true;
      continue;
    }
    if (line.startsWith('@@')) {
      inHeader = false;
      closeGroup();
      continue;
    }

    // `--- a/path` arrives before `+++`. Hold on to it: for a deleted file the
    // `+++` side is /dev/null and this is the only place the path appears.
    const from = inHeader && line.match(/^--- (?:a\/(.*)|\/dev\/null)$/);
    if (from) {
      closeGroup();
      pendingDeletePath = from[1] ?? null;
      continue;
    }
    const to = inHeader && line.match(/^\+\+\+ (?:b\/(.*)|\/dev\/null)$/);
    if (to) {
      closeGroup();
      current = to[1] ?? pendingDeletePath;
      pendingDeletePath = null;
      borrowing = null;
      if (current) statFor(current);
      continue;
    }
    if (!current) continue;
    if (isOurs(current) || skip.includes(current)) continue;

    if (line.startsWith('-')) {
      group.dels += 1;
      statFor(current).deleted += 1;
      continue;
    }
    if (!line.startsWith('+')) {
      // Context, or diff noise like "\ No newline at end of file".
      closeGroup();
      continue;
    }

    const text = line.slice(1);
    const st = statFor(current);
    group.adds += 1;
    st.added += 1;

    if (BORROW_END.test(text)) {
      borrowing = null;
      st.borrowed += 1;
      group.borrowedAdds += 1;
      continue;
    }
    const start = text.match(BORROW_START);
    if (start) {
      borrowing = start[1];
      st.prs.add(start[1]);
      st.borrowed += 1;
      group.borrowedAdds += 1;
      continue;
    }
    if (borrowing) {
      st.borrowed += 1;
      group.borrowedAdds += 1;
      continue;
    }
    if (isHook(text)) st.hooks.push(text.trim());
  }
  closeGroup();

  // A file that only appeared as a header, with nothing of ours in it, is noise.
  for (const [file, st] of stats) {
    if (st.added === 0 && st.deleted === 0) stats.delete(file);
  }
  return stats;
}
