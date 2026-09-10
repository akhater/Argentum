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
    release: '2026.37.2',
    why:
      'Real chromatic adaptation from darktable, in Kelvin, with an auto mode and '
      + 'a picker — replacing three fixed multipliers.',
  },
  {
    stage: 'done',
    what: 'Correct RAW decoding',
    release: '2026.37.10',
    why:
      'Canon sRAW and mRAW were being black-subtracted twice, which caused both a '
      + 'green cast and crushed shadows.',
  },
  {
    stage: 'done',
    what: 'Camera colour profiles',
    release: '2026.37.12',
    why:
      'Pick a profile per photo under Color, or leave it on the built-in matrix. '
      + 'Profiles are found online or imported, and applied while the photo is drawn.',
  },
  {
    stage: 'planned',
    what: 'Highlight recovery',
    why: 'Rebuild detail in clipped highlights. The biggest visible rescue on real photos.',
  },
  {
    stage: 'planned',
    what: 'Offline catalogue',
    why: 'Browse and search photos with the drive unplugged, and relink folders that move.',
  },
  {
    stage: 'planned',
    what: '16-bit TIFF export',
    why:
      'Export currently writes 8 bits a channel, which throws away most of what '
      + 'a RAW holds and shows as banding in skies once anything is edited '
      + 'afterwards. Needed before Argentum can hand work to another editor.',
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
