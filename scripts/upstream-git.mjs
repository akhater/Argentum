// One git process per question.
//
// WHY THIS IS ITS OWN FILE
//
// The checker used to start about 390 git processes per run: two `git show` per
// upstream commit in two ranges, one per revision of the registry, one per
// shadowed file. On Windows each one costs tens of milliseconds before git does
// any work, and the run took 37-74 s. Every question is now asked of one
// streamed command and answered here, in JavaScript.
//
// Arguments are arrays, never a shell string, so a path or a ref cannot be
// misread by a shell on any platform.

import { spawnSync } from 'node:child_process';

/** A failed git command, with git's own message. */
export class GitError extends Error {}

/**
 * A git runner bound to one repository.
 *
 * run(args, { input, raw }) returns stdout: a string, or a Buffer when `raw`.
 * It throws GitError on a non-zero exit. stderr is captured, not inherited:
 * callers decide what a failure means, and git's "fatal: ..." above our own
 * message saying the same thing usefully is noise.
 */
export function gitIn(cwd) {
  return (args, { input, raw = false } = {}) => {
    const result = spawnSync('git', args, {
      cwd,
      input,
      // No encoding gives Buffers back (and still encodes `input` as UTF-8).
      encoding: raw ? undefined : 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      windowsHide: true,
    });
    if (result.error) throw result.error;
    if (result.status !== 0) {
      throw new GitError(`git ${args.join(' ')}: ${String(result.stderr).trim()}`);
    }
    return result.stdout;
  };
}

/**
 * Many files at many revisions, in one process: Map<'rev:path', text | null>.
 *
 * `git cat-file --batch` answers each line of its input with a header and the
 * object's bytes; a spec that names nothing comes back as `<spec> missing`.
 * Sizes are in bytes, so the output is read as a Buffer and decoded per blob.
 */
export function readBlobs(run, specs) {
  const found = new Map();
  if (specs.length === 0) return found;
  const out = run(['cat-file', '--batch'], { input: specs.join('\n') + '\n', raw: true });
  let at = 0;
  for (const spec of specs) {
    const eol = out.indexOf(0x0a, at);
    const header = out.subarray(at, eol).toString('utf8');
    at = eol + 1;
    const m = header.match(/^[0-9a-f]+ (\w+) (\d+)$/);
    if (!m) {
      // `<spec> missing`, or `<spec> ambiguous`.
      found.set(spec, null);
      continue;
    }
    const size = Number(m[2]);
    found.set(spec, m[1] === 'blob' ? out.subarray(at, at + size).toString('utf8') : null);
    at += size + 1;
  }
  return found;
}

/**
 * The commits in `from..to`, oldest first: { sha, parents, subject, body, files }.
 *
 * One `git log`. Merge commits list no files (git shows none for a merge by
 * default), which is what is wanted: a pull request's files belong to the
 * commits on its branch. Renames are off so a moved file shows both paths.
 */
export function commitsIn(run, from, to) {
  const out = run([
    'log', '--reverse', '--no-renames', '--name-only',
    '--format=%x1e%H%x1f%P%x1f%s%x1f%b%x1f',
    `${from}..${to}`,
  ]);
  const commits = [];
  for (const chunk of out.split('\x1e')) {
    if (!chunk.trim()) continue;
    const [sha, parents, subject, body, rest = ''] = chunk.split('\x1f');
    commits.push({
      sha,
      parents: parents ? parents.split(' ') : [],
      subject,
      body: body.trim(),
      files: rest.split('\n').map((f) => f.trim()).filter(Boolean),
    });
  }
  return commits;
}

/**
 * Every tag, as name -> commit sha (annotated tags peeled to their commit).
 * One `git for-each-ref`.
 */
export function tags(run) {
  const out = run(['for-each-ref', 'refs/tags', '--format=%(refname:short)%09%(objectname)%09%(*objectname)']);
  const map = new Map();
  for (const line of out.split('\n')) {
    if (!line) continue;
    const [name, object, peeled] = line.split('\t');
    map.set(name, peeled || object);
  }
  return map;
}

/** `git` itself is fine with this, but a typo in a register is not. */
export const isFullSha = (s) => typeof s === 'string' && /^[0-9a-f]{40}$/.test(s);
