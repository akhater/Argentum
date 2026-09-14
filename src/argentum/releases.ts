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
    version: '26.38.1',
    date: '2026-09-14',
    notes: [
      'An exported TIFF now keeps the camera details: camera, lens, exposure, date '
      + 'and copyright — and your location only if you leave that switch on. The '
      + 'Keep metadata switch was ticked by default, not shown for TIFF, and did '
      + 'nothing for it, so exactly the format you would hand to another editor was '
      + 'the one that arrived with nothing attached.',
      'You can now choose 8 or 16 bits when exporting a TIFF. Every TIFF was 16-bit '
      + 'before, whether that was wanted or not. A file going to a client is a '
      + 'delivery rather than a master, and 8 bits is the right size for it.',
      'Hold Ctrl and drag on the photo to move its crop, or Ctrl and scroll to '
      + 'resize it; Ctrl-double-click puts crop and rotation back. These work '
      + 'outside crop mode too.',
    ],
  },
  {
    version: '26.37.20',
    date: '2026-09-13',
    notes: [
      'Exporting a 16-bit TIFF now puts real high-precision data inside it. The file '
      + 'said 16-bit before and the picture in it was 8-bit, which showed up the moment '
      + 'you took it somewhere else and pushed it — skies and skin banding under a '
      + 'curve that should have had room to move.',
      'The rest of the export got the same treatment: a watermark no longer coarsens '
      + 'the photograph underneath it, and the per-mask images saved alongside a TIFF '
      + 'carry the same precision as the main file.',
    ],
  },
  {
    version: '26.37.18',
    date: '2026-09-12',
    notes: [
      'Fixed a case where the preview stopped converting colour for your screen and '
      + 'stayed that way until the app was restarted — if the screen’s profile could '
      + 'not be read for a moment, that answer was kept for good. It is retried now.',
      'The screen conversion is Windows-only, and needs a monitor profile built from '
      + 'primaries rather than a lookup table. That was always true and is now written '
      + 'down under Known issues.',
    ],
  },
  {
    version: '26.37.17',
    date: '2026-09-11',
    notes: [
      'Fixed the preview showing photos more saturated than they are. Argentum now '
      + 'converts colour for the screen it is on, read from that display’s own '
      + 'profile — so what you see matches what you export. On a normal sRGB screen '
      + 'nothing changes; on a wide-gamut one, quite a lot does.',
      'Added highlight recovery: when a bright area blows out in one colour channel, '
      + 'it is rebuilt from the two that survived. On by default, under Color.',
      'The clipping warning now steps through channels — off, L, R, G, B — and '
      + 'holding Ctrl while dragging Whites or Blacks shows only what is about to clip.',
    ],
  },
  {
    version: '26.37.16',
    date: '2026-09-10',
    notes: [
      'The clipping warning now steps through the channels — off, L, R, G, B. '
      + 'Red is a blown highlight and blue is a crushed shadow in every mode; '
      + 'only the channel being watched changes. If a highlight blows in one '
      + 'channel it can usually be saved, and if it blows in all three it cannot.',
    ],
  },
  {
    version: '26.37.13',
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
    version: '26.37.12',
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
    version: '26.37.11',
    date: '2026-09-10',
    notes: [
      'Added this About section — credits, the roadmap, what is currently broken, '
      + 'and these notes.',
    ],
  },
  {
    version: '26.37.10',
    date: '2026-09-09',
    notes: [
      'Fixed a green cast and heavy shadows on Canon sRAW and mRAW photos. These '
      + 'formats were being decoded wrongly, which darkened the picture and pushed '
      + 'colour towards green. Brightness and colour now match darktable closely.',
      'Shadow detail near black is no longer thrown away before editing starts.',
    ],
  },
  {
    version: '26.37.6',
    date: '2026-09-09',
    notes: [
      'Lenses are now detected automatically on Canon bodies, so lens corrections '
      + 'apply without picking the lens by hand.',
      'Added a refresh button in the metadata panel for photos read before the fix.',
    ],
  },
  {
    version: '26.37.5',
    date: '2026-09-09',
    notes: [
      'Added an RGB readout, so a colour under the cursor can be checked rather '
      + 'than guessed at.',
    ],
  },
  {
    version: '26.37.2',
    date: '2026-09-08',
    notes: [
      'White balance rebuilt on real colour science, in Kelvin, replacing three '
      + 'fixed multipliers. Neutral surfaces now come out neutral.',
      'Added automatic white balance, and a picker that settles on one answer '
      + 'however many times the same spot is clicked.',
    ],
  },
  {
    version: '26.37.1',
    date: '2026-09-08',
    notes: [
      'First build. A fork of RapidRAW that renders identically to it — the '
      + 'starting point everything since is measured against.',
    ],
  },
];
