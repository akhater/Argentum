/** The file's own name: what follows the last slash or backslash of its path. */
export const fileName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

/** The folder a file is in: its path up to the last separator, a drive's root kept whole (`C:\`). */
export function folderOf(path: string): string {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  if (cut < 0) return path;
  const folder = path.slice(0, cut);
  return /^[A-Za-z]:$/.test(folder) || folder === '' ? path.slice(0, cut + 1) : folder;
}

/** The folders above a path, the outermost first: `C:\`, `C:\Users`, `C:\Users\you`. */
export function ancestorsOf(path: string): string[] {
  const out: string[] = [];
  for (let below = path, at = folderOf(path); at !== below; below = at, at = folderOf(at)) out.unshift(at);
  return out;
}

/**
 * Whether two paths name the same file: Windows paths (a drive letter or a
 * backslash) ignore case and which slash; others are compared as they are.
 */
export function samePath(a: string, b: string): boolean {
  if (a === b) return true;
  const windows = (p: string) => /^[A-Za-z]:|\\/.test(p);
  if (!windows(a) || !windows(b)) return false;
  const norm = (p: string) => p.replace(/\//g, '\\').toLowerCase();
  return norm(a) === norm(b);
}

/** A folder's name: its last part, or a drive's (`C:`), or the root's (`/`). */
export const folderName = (path: string): string => fileName(path.replace(/[\\/]+$/, '')) || path;

const byName = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/**
 * Two names in Windows Explorer's order: case ignored, numbers by their value
 * (IMG_2 before IMG_10), and `_`, `-` and other punctuation before digits and
 * letters, so `_inbox` comes first. Folders and photos are both sorted by it.
 */
export const compareNames = (a: string, b: string): number => byName.compare(a, b);
