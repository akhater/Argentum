/**
 * Release notes, for the person using the app. Ours.
 *
 * WHY NOT RENDER CHANGELOG.md
 *
 * It was tried, and it was wrong. `CHANGELOG.md` is an engineering record —
 * file paths, module names, mergeability budgets, "removed the Ko-Fi donate
 * link". All of it worth keeping, none of it worth reading if what you want to
 * know is whether your photos will look different.
 *
 * Filtering it did not work either. Every pass at hiding the internal parts
 * left more of them behind, because the split is not structural: a single
 * sentence can carry both. The two documents have different audiences and
 * different content, so they are two documents.
 *
 * THE RULE
 *
 * An entry here answers "what changed for me?" in one line, in plain language,
 * with no file names and no jargon. If a release changed nothing a person would
 * notice, it does not get an entry — a version number with nothing under it is
 * more honest than padding.
 *
 * `CHANGELOG.md` remains the full record, and is where the detail lives.
 */

export interface Release {
  version: string;
  date: string;
  /** One line each, in plain language. Empty means nothing user-visible. */
  notes: string[];
}

export const RELEASES: Release[] = [
  {
    version: '2026.37.16',
    date: '2026-09-10',
    notes: [
      'The clipping warning now steps through the channels — off, L, R, G, B. '
      + 'Red is a blown highlight and blue is a crushed shadow in every mode; '
      + 'only the channel being watched changes. If a highlight blows in one '
      + 'channel it can usually be saved, and if it blows in all three it cannot.',
    ],
  },
  {
    version: '2026.37.13',
    date: '2026-09-10',
    notes: [
      'Hover any name the layout has cut short — a file on a library card, a '
      + 'folder, a preset — and the full text now appears.',
      'Fixed "Find one" failing to reach RawTherapee on some networks, and it no '
      + 'longer offers to fetch a profile you already have.',
      'The roadmap now shows finished work last, with the release each one '
      + 'shipped in.',
    ],
  },
  {
    version: '2026.37.12',
    date: '2026-09-10',
    notes: [
      'Added camera profiles. A profile describes how your particular camera '
      + 'renders colour; pick one per photo under Color, or leave it on Built-in. '
      + 'Argentum can fetch one for your camera, or you can import your own.',
      'Added My Gear in Settings: the cameras you shoot with and the profiles you '
      + 'keep for each. Both fill themselves in as you work.',
    ],
  },
  {
    version: '2026.37.11',
    date: '2026-09-10',
    notes: [
      'Added this About section — credits, the roadmap, what is currently broken, '
      + 'and these notes.',
    ],
  },
  {
    version: '2026.37.10',
    date: '2026-09-09',
    notes: [
      'Fixed a green cast and heavy shadows on Canon sRAW and mRAW photos. These '
      + 'formats were being decoded wrongly, which darkened the picture and pushed '
      + 'colour towards green. Brightness and colour now match darktable closely.',
      'Shadow detail near black is no longer thrown away before editing starts.',
    ],
  },
  {
    version: '2026.37.6',
    date: '2026-09-09',
    notes: [
      'Lenses are now detected automatically on Canon bodies, so lens corrections '
      + 'apply without picking the lens by hand.',
      'Added a refresh button in the metadata panel for photos read before the fix.',
    ],
  },
  {
    version: '2026.37.5',
    date: '2026-09-09',
    notes: [
      'Added an RGB readout, so a colour under the cursor can be checked rather '
      + 'than guessed at.',
    ],
  },
  {
    version: '2026.37.2',
    date: '2026-09-08',
    notes: [
      'White balance rebuilt on real colour science, in Kelvin, replacing three '
      + 'fixed multipliers. Neutral surfaces now come out neutral.',
      'Added automatic white balance, and a picker that settles on one answer '
      + 'however many times the same spot is clicked.',
    ],
  },
  {
    version: '2026.37.1',
    date: '2026-09-08',
    notes: [
      'First build. A fork of RapidRAW that renders identically to it — the '
      + 'starting point everything since is measured against.',
    ],
  },
];
