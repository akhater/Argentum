/**
 * Which folder or album the library was asked for last. Listing a folder runs off the UI
 * thread, so a folder on a slow or sleeping drive can answer after one opened later: only
 * the request made last may fill the library. Shaped like `imageSelectGeneration` in
 * hooks/useAppNavigation.ts, which does the same for photos (RapidRAW ecf7730f).
 */
let latest = 0;

/** A check that stays true until a folder or album is asked for after this call. */
export function noLaterFolderRequest(): () => boolean {
  const seen = latest;
  return () => seen === latest;
}

/** Marks a new request for a folder or album; the check it returns is true while it is the latest. */
export function newFolderRequest(): () => boolean {
  latest += 1;
  return noLaterFolderRequest();
}
