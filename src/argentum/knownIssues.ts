/**
 * What is known to be broken, right now. Ours.
 *
 * WHY NOT READ THEM OUT OF THE CHANGELOG
 *
 * Because a changelog records what was true at a release, and this has to say
 * what is true today. The `### Known issues` list in `2026.37.1` still claimed
 * lens auto-detection was broken long after `2026.37.6` fixed it — a stale
 * warning is worse than none, because a reader trusts it and stops looking.
 *
 * So this is a list with one job, and the rule that comes with it: **when
 * something here is fixed, delete the entry in the same commit as the fix.**
 * The changelog is where it goes to be remembered.
 */

export interface KnownIssue {
  what: string;
  detail: string;
  /** True when it came from RapidRAW rather than from anything Argentum did. */
  inherited?: boolean;
}

export const KNOWN_ISSUES: KnownIssue[] = [
  {
    what: 'Zoom and pan feel sluggish',
    detail:
      'Noticeably behind darktable, and the mask overlay lags the image while '
      + 'moving. Not yet diagnosed — it could be the overlay, the render pipeline '
      + 'or the settings.',
    inherited: true,
  },
  {
    what: 'Click-to-select masking cannot be refined',
    detail:
      'Each click makes its own sub-mask instead of refining the current one. The '
      + 'underlying model accepts several positive and negative points, but only '
      + 'one is ever sent.',
    inherited: true,
  },
  {
    what: 'Colour is close to darktable, not equal to it',
    detail:
      'Across a ten-photo test set the red and blue channels sit within about 4–5% '
      + 'of darktable, and brightness matches. Full RAW files are closer than sRAW. '
      + 'Camera colour profiles are the next step in closing this.',
  },
  {
    what: 'Tested against one camera',
    detail:
      'Every colour measurement so far comes from a Canon EOS 5D Mark II. The RAW '
      + 'fixes are written against file formats rather than specific bodies, but '
      + 'that has not been confirmed on other cameras yet.',
  },
];
