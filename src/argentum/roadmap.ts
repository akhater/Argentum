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
 * Keep it in step with `docs/ROADMAP.md` when that changes.
 */

export type Stage = 'done' | 'building' | 'planned';

export interface Milestone {
  stage: Stage;
  what: string;
  why: string;
}

export const ROADMAP: Milestone[] = [
  {
    stage: 'done',
    what: 'White balance',
    why:
      'Real chromatic adaptation from darktable, in Kelvin, with an auto mode and '
      + 'a picker — replacing three fixed multipliers.',
  },
  {
    stage: 'done',
    what: 'Correct RAW decoding',
    why:
      'Canon sRAW and mRAW were being black-subtracted twice, which caused both a '
      + 'green cast and crushed shadows.',
  },
  {
    stage: 'building',
    what: 'Camera colour profiles',
    why:
      'Per-camera calibration, so a body renders as itself rather than generically. '
      + 'The largest remaining gap against darktable.',
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
