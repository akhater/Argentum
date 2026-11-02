/**
 * The root or pinned folder a folder opened inside belongs to, so the tree expands down to it: the
 * longest one the path is inside, whole folder names only (D:\Photos2 is not inside D:\Photos).
 * RapidRAW d6d90783: src/hooks/useAppNavigation.ts.
 */
export function rootContaining(path: string, roots: readonly string[]): string | undefined {
  const isInsideRoot = (r: string) =>
    path === r || (path.startsWith(r) && (/[\\/]$/.test(r) || ['/', '\\'].includes(path[r.length])));
  return roots
    .filter(isInsideRoot)
    .reduce<string | undefined>((best, r) => (!best || r.length > best.length ? r : best), undefined);
}
