/**
 * What is planned, in a few lines. Ours.
 *
 * WHY THIS IS NOT `docs/ROADMAP.md`
 *
 * That file is the working plan — effort estimates, ordering arguments, notes
 * to whoever is building it. This is the short version someone running the app
 * would want: what works, what is next, and roughly in what order. Different
 * audience, different document.
 *
 * It is short on purpose. A roadmap nobody can read in ten seconds is a wish
 * list, and every line here is a promise that has to be kept or removed.
 *
 * WHY DONE ITEMS STAY, AND STAY AT THE BOTTOM
 *
 * A roadmap that deletes what it delivered reads as though nothing has been
 * delivered. Keeping them, each stamped with the release it shipped in, turns
 * the list into a record as well as a plan — and puts a date against a promise,
 * which is the part that costs something to get wrong.
 *
 * They sort to the bottom because the interesting half of a roadmap is the part
 * that has not happened yet. `MILESTONES` below is written in the order the work
 * is meant to happen; `ROADMAP` is that list with the finished ones moved down,
 * so marking something done needs one word changed and nothing moved.
 *
 * Keep it in step with `docs/ROADMAP.md` when that changes.
 */

export type Stage = 'done' | 'building' | 'planned';

export interface Milestone {
  stage: Stage;
  what: string;
  why: string;
  /** The release it shipped in. Only meaningful once `stage` is `done`. */
  release?: string;
}

const MILESTONES: Milestone[] = [
  {
    stage: 'done',
    what: 'White balance',
    release: '26.37.2',
    why:
      'Real chromatic adaptation from darktable, in Kelvin, with an auto mode and '
      + 'a picker — replacing three fixed multipliers.',
  },
  {
    stage: 'done',
    what: 'Correct RAW decoding',
    release: '26.37.10',
    why:
      'Canon sRAW and mRAW were being black-subtracted twice, which caused both a '
      + 'green cast and crushed shadows.',
  },
  {
    stage: 'done',
    what: 'Camera colour profiles',
    release: '26.37.12',
    why:
      'Pick a profile per photo under Color, or leave it on the built-in matrix. '
      + 'Profiles are found online or imported, and applied while the photo is drawn.',
  },
  {
    stage: 'done',
    what: 'Highlight recovery',
    release: '26.37.17',
    why:
      'When a bright area clips, one channel usually blows before the others. '
      + 'Rebuilds the missing one from the two that survived. On by default, and '
      + 'it does nothing at all to a photo with nothing clipped — which, measured '
      + 'across 400 photos, is nearly all of them.',
  },
  {
    stage: 'done',
    what: 'Clipping preview',
    release: '26.37.17',
    why:
      'The warning button steps through off, L, R, G and B. Hold Ctrl while '
      + 'dragging Whites or Blacks and the picture empties to show only what is '
      + 'about to go — which is how those two are actually set.',
  },
  {
    stage: 'done',
    what: 'Display colour management',
    release: '26.37.17',
    why:
      'The preview now converts for the screen it is on, read from the display profile '
      + 'own profile. Without it a wide-gamut display showed every photo more '
      + 'saturated than it was, and nothing on screen said so.',
  },
  {
    stage: 'planned',
    what: 'Group by date, camera or lens',
    why:
      'The library is one flat list. You can sort it and filter it, but not break '
      + 'it into days, or cameras, or lenses, which is how a shoot is actually '
      + 'looked for. And sorting by date uses the file’s modified time — which '
      + 'copying or re-editing changes — while the moment the shutter fired sits '
      + 'in the EXIF, read for display and never used for ordering.',
  },
  {
    stage: 'planned',
    what: 'Offline catalogue',
    why: 'Browse and search photos with the drive unplugged, and relink folders that move.',
  },
  {
    stage: 'building',
    what: '16-bit TIFF export',
    why:
      'Export wrote 8 bits a channel, which threw away most of what a RAW holds '
      + 'and showed as banding in skies once anything was edited afterwards. A '
      + 'TIFF now carries 16, and that half shipped in 26.37.20 — but the photo '
      + 'still reaches the renderer at half precision, so the file has room the '
      + 'pipeline cannot yet fill. Not done until it can.',
  },
  {
    stage: 'planned',
    what: 'Use less memory',
    why:
      'Nobody has measured what Argentum costs on a large RAW, and the pipeline '
      + 'has been gaining full-resolution copies of the photo rather than losing '
      + 'them — the newest of which, the 16-bit export target, is twice the size '
      + 'of the one it sits beside. Measure it on a 45MP file first; a machine '
      + 'that swaps is slower than any shader is fast.',
  },
  {
    stage: 'building',
    what: 'EXIF in an exported TIFF',
    why:
      'Camera, lens, exposure, date, copyright and GPS now survive a TIFF export, '
      + 'and the Keep metadata switch is shown for TIFF rather than hidden while '
      + 'ticked. Written and tested; not yet released.',
  },
  {
    stage: 'planned',
    what: 'Stack burst shots',
    why:
      'A run of frames taken in continuous mode is one moment, not eight, and the '
      + 'library shows it as eight. Group them so a burst takes one slot and opens '
      + 'to the rest. The timestamps are the obvious signal; whether they are enough '
      + 'on their own is still to be worked out.',
  },
  {
    stage: 'planned',
    what: 'Name what is in the picture, then mask it',
    why:
      'An AI mask needs you to drag a box around the thing first. Instead: look at '
      + 'the photo once, list what is in it — sky, face, the dog, the tree on the '
      + 'left — and tick the ones to mask. RAM++ names things but does not say '
      + 'where they are, so a locating step sits between it and the mask. Everything '
      + 'stays on this machine, and the same names make the library searchable by '
      + 'what is in a photo rather than only by filename and EXIF.',
  },
  {
    stage: 'planned',
    what: 'Feather on the linear mask',
    why:
      'A linear gradient has a hard-ish edge and no way to soften it. The falloff '
      + 'is already there — mask_generation.rs takes a range, fixed at 50 — it has '
      + 'simply never been put on screen.',
  },
  {
    stage: 'planned',
    what: 'Skin tones',
    why:
      'The colour nobody forgives getting wrong, and the one a measurement against '
      + 'another program cannot settle. Specifics still to come.',
  },
  {
    stage: 'planned',
    what: 'Filmic tone mapping',
    why: 'A modern tone curve for high-contrast scenes, once the colour work underneath it is right.',
  },
];

/**
 * The same list with everything finished moved to the end, each side keeping
 * the order it was written in.
 */
export const ROADMAP: Milestone[] = [
  ...MILESTONES.filter((m) => m.stage !== 'done'),
  ...MILESTONES.filter((m) => m.stage === 'done'),
];
