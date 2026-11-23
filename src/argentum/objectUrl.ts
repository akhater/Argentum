// Object URLs keep their bytes alive until revoked. Every URL made with
// createObjectURL has one owner, and the owner calls these when it replaces or
// drops the URL.

/** Revoke `url` if it is a blob URL. Safe to call with null and with other URLs. */
export function revokeBlobUrl(url: string | null | undefined): void {
  if (url?.startsWith('blob:')) URL.revokeObjectURL(url);
}

/**
 * Revoke after a short delay, so an <img> that still shows `url` has time to
 * switch to its replacement. `isProtected` is asked again at revoke time (the
 * image cache can adopt a URL meanwhile) and a protected URL is left alone.
 */
export function revokeBlobUrlLater(
  url: string | null | undefined,
  isProtected: (url: string) => boolean = () => false,
  delayMs = 250,
): void {
  if (!url?.startsWith('blob:') || isProtected(url)) return;
  setTimeout(() => {
    if (!isProtected(url)) URL.revokeObjectURL(url);
  }, delayMs);
}

/** Owns one object URL at a time: `replace` releases the previous one, `release` drops the last. */
export function createBlobUrlSlot() {
  let current: string | null = null;
  return {
    replace(blob: Blob | null): string | null {
      const previous = current;
      current = blob ? URL.createObjectURL(blob) : null;
      revokeBlobUrlLater(previous);
      return current;
    },
    release(): void {
      revokeBlobUrl(current);
      current = null;
    },
  };
}

/** Revoke every blob URL in a list or record of URLs. */
export function revokeBlobUrls(urls: Iterable<string | null | undefined>): void {
  for (const url of urls) revokeBlobUrl(url);
}
