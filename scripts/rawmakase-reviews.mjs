// What we took, or chose not to take, from each RAWmakase release.
//
// ONE REVIEW PER RELEASE
//
// RAWmakase (github.com/pch/rawmakase, MIT) is a source Argentum copies from,
// never merges. Each release is read once, from its notes and its commits, and
// gets one entry at the end of REVIEWS:
//
//   {
//     release: 'v0.4.3',     the release read up to
//     from: 'v0.4.1',        where the review before it ended
//     on: 'YYYY-MM-DD',      the day it was read
//     features: [            one line per feature, not per commit
//       { what, commits: ['7-char sha', ...], verdict, why, into? },
//     ],
//   }
//
// Feature verdicts:
//
//   copy    worth copying into a file of ours, with its source commit; `into`
//           names that file, and `why` says whether it is done or still to do
//   skip    not for Argentum: the library and catalog, a fix inside their own
//           engine's tools that Argentum does not run, or something we have
//   decide  worth it only on evidence; `why` says what decides it
//
// The tables copied from RAWmakase (scripts/rawmakase-*.mjs) each name their
// own release and warn on their own; a review says when one is due.
//
// check:merge warns when RAWmakase has a release newer than the last review.
// Releases before v0.4.1 were read for the copies CHANGELOG.md names.

import { releases } from './rawmakase-cameras.mjs';

const VERDICTS = ['copy', 'skip', 'decide'];

export const REVIEWS = [
  {
    release: 'v0.4.3',
    from: 'v0.4.1',
    on: '2026-10-23',
    features: [
      {
        what: "Adobe's baseline exposure for 49 more bodies (camera table), released in v0.4.1",
        commits: ['696f8f0'],
        verdict: 'copy',
        into: 'src-tauri/src/mods/dng_baseline.tsv',
        why: 'To do. The table is still at v0.4.0. 21 Nikon and 28 Panasonic, Sony, Olympus, Pentax and '
          + 'Leica bodies get their own value, and the make median that unlisted bodies of those makes '
          + 'take moves with them. Existing rows keep their values; a few gain alias names. Its own PR, '
          + 'with the changed bodies shown on photos.',
      },
      {
        what: 'B&W chart and colour grading curves',
        commits: [],
        verdict: 'copy',
        into: 'src-tauri/engine/src/rm_black_white_chart.tsv, src-tauri/engine/src/rm_color_grade_curves.tsv',
        why: 'Done. Their sources did not change; both tables name v0.4.3 now, values identical.',
      },
      {
        what: 'DNG OpcodeList3 gain maps as a per-channel lens correction (drone DNGs)',
        commits: ['f0899e0'],
        verdict: 'copy',
        into: 'src-tauri/engine/src/rm_lens_builtin.rs, src-tauri/engine/src/dng_gain_map.rs',
        why: "To do. DJI and Hasselblad drone DNGs store lens shading there; RAWmakase's corners were "
          + 'up to 11 L* darker than Camera Raw without it. Argentum already reads OpcodeList3 for '
          + 'vignetting and warp (from v0.4.0) and applies OpcodeList2 gain maps; this is the one list '
          + 'missing. Additive on their side (optics::GainGrid), nothing we copied changed.',
      },
      {
        what: 'New X-Trans demosaic and stripe colour pass (colour moiré on fine patterns)',
        commits: ['6a660cf'],
        verdict: 'decide',
        why: "Argentum demosaics X-Trans with rawler's four-pass neighbour average. Theirs is new and "
          + "changed in two releases running; RawTherapee's and darktable's Markesteijn are the "
          + 'proven alternatives. Decided on X-Trans RAWs side by side, all three.',
      },
      {
        what: '.cube LUTs as looks: 1D cubes, 1D shapers, Amount to 200%, grouped by pack',
        commits: ['9a18066', '1a18f67', 'd2610c0', '4373b4b', 'af8718a', '0d5049e'],
        verdict: 'decide',
        why: "lut_processing.rs reads 3D cubes only, and Intensity stops at 100. Their reader is worth "
          + 'copying when 1D or shaper cubes are in use; Amount above 100 changes the interface.',
      },
      {
        what: "Lightroom's HDR merge and panorama DNGs (JPEG XL tiles, half floats)",
        commits: ['0b2f4cb'],
        verdict: 'decide',
        why: 'rawler decodes JPEG XL tiles with jxl-oxide, 8- and 16-bit integer only. Not tried in '
          + 'Argentum; needs a sample of each to see what fails before copying anything.',
      },
      {
        what: 'Library file management: move, rename, Trash, folders, other drives, crash journal',
        commits: ['e2d23d3', 'd1642fb', '46c6fb9', '96e5325'],
        verdict: 'skip',
        why: 'Library and catalog, outside the editor.',
      },
      {
        what: 'Highlights -100 in dark scenes, Clarity halos, Dehaze with negative Highlights, local maps after lens correction',
        commits: ['94714f6', 'f50cc76', '24336fd', '74ea667'],
        verdict: 'skip',
        why: "Fixes inside RAWmakase's own tone, Clarity and Dehaze, which Argentum does not run.",
      },
      {
        what: "Camera Raw look Amount and UUID read from XMP elements",
        commits: ['3c563a7'],
        verdict: 'skip',
        why: "preset_converter.rs reads no look Amount in either form.",
      },
      {
        what: 'Mouse wheel scrolls the filmstrip',
        commits: ['299727c'],
        verdict: 'skip',
        why: 'Filmstrip.tsx already does.',
      },
      {
        what: 'Selection models (rawmakase-inference)',
        commits: [],
        verdict: 'skip',
        why: 'Unchanged; src-tauri/rm_selection stays as it is.',
      },
    ],
  },
];

/** Problems with REVIEWS itself: a broken chain or an unknown verdict. */
export function reviewProblems(reviews = REVIEWS) {
  const problems = [];
  reviews.forEach((review, i) => {
    if (i > 0 && review.from !== reviews[i - 1].release) {
      problems.push(`${review.release} starts at ${review.from}, not where ${reviews[i - 1].release} ended`);
    }
    for (const feature of review.features) {
      if (!VERDICTS.includes(feature.verdict)) problems.push(`${review.release}: "${feature.what}" has verdict ${feature.verdict}`);
    }
  });
  return problems;
}

/** For check:merge: a warning when RAWmakase has released past the last review, else null. */
export async function reviewWarning() {
  const problems = reviewProblems();
  if (problems.length > 0) return `scripts/rawmakase-reviews.mjs: ${problems.join('; ')}`;
  const last = REVIEWS.at(-1).release;
  const names = [...(await releases()).keys()];
  if (!names.includes(last)) return `scripts/rawmakase-reviews.mjs ends at ${last}, which is not a RAWmakase release`;
  const waiting = names.slice(names.indexOf(last) + 1);
  if (waiting.length === 0) return null;
  return `RAWmakase ${waiting.join(', ')} ${waiting.length === 1 ? 'is' : 'are'} out and not reviewed - `
    + 'add a review to scripts/rawmakase-reviews.mjs';
}
