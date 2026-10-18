/**
 * Sizes as Windows Explorer shows them, in units of 1024, so the number here
 * matches the one in a file's or a disk's properties. Ours.
 */
export function formatSize(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024 * 1024) {
    return `${(mb / (1024 * 1024)).toFixed(1)} TB`;
  }
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(1)} GB`;
  }
  if (mb >= 10 || bytes === 0) {
    return `${Math.round(mb)} MB`;
  }
  if (mb >= 0.1) {
    return `${mb.toFixed(1)} MB`;
  }
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
