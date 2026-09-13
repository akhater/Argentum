/**
 * Who Argentum is built on. Ours.
 *
 * Grouped rather than listed flat, because the list will grow unevenly. Two
 * projects are the reason this one exists; anything harvested later is a
 * different kind of debt and belongs under its own heading rather than diluting
 * theirs.
 *
 * WHAT DOES NOT GO HERE
 *
 * The models and libraries that came with RapidRAW. Those are RapidRAW's to
 * credit and it does, inside the app, in its own list. Repeating them under
 * Argentum's name would be taking credit for assembling something we inherited.
 * A dependency earns an entry here when *Argentum* brought it in.
 */

export interface Credit {
  name: string;
  href: string;
  what: string;
}

export interface CreditGroup {
  heading: string;
  blurb?: string;
  entries: Credit[];
}

export const CREDITS: CreditGroup[] = [
  {
    heading: 'Built on',
    blurb: 'Argentum is not written from scratch. These two are the reason it exists at all.',
    entries: [
      {
        name: 'RapidRAW',
        href: 'https://github.com/CyberTimon/RapidRAW',
        what:
          'By Timon Käch. Argentum is a fork of it. The interface, the GPU pipeline, '
          + 'the catalogue and very nearly all of the application are his work, along '
          + 'with the libraries and models he credits inside it.',
      },
      {
        name: 'darktable',
        href: 'https://github.com/darktable-org/darktable',
        what:
          'The reference Argentum is measured against, and where its colour science '
          + 'comes from — chromatic adaptation, and the raw-level behaviour its '
          + 'decoder gets right.',
      },
    ],
  },
  {
    heading: 'Carried early',
    blurb:
      "Work by RapidRAW's contributors that Argentum runs before upstream has "
      + 'merged it. Each is marked in the source between `// upstream #NNNN` and '
      + '`// end upstream #NNNN`, so when their pull request lands the two '
      + 'converge instead of colliding — and so it stays obvious whose it is.',
    entries: [
      {
        name: 'dimafa — #1466, high-precision export',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1466',
        what:
          'Argentum’s 16-bit TIFF export is built on their idea: make the export '
          + 'pipeline by rewriting the storage format in the shader source, and switch '
          + 'the 8-bit dither off with a pipeline constant. The idea is theirs; the code '
          + 'was written here, and the 32-bit target and the tests are ours.',
      },
      {
        name: '#1307, the AI patch cache key',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1307',
        what:
          'Two AI patches of the same length no longer share a cache entry, so the '
          + 'wrong one is not rendered from cache.',
      },
      {
        name: '#1633, the sRGB exponent',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1633',
        what: 'sRGB decoding uses an exponent of 2.4, which is what the standard says.',
      },
    ],
  },
  {
    heading: 'Camera profiles',
    blurb:
      'Argentum reads camera profiles but ships none — the free collections are '
      + "published with their authors' individual permission rather than under a "
      + 'licence that lets anyone else redistribute them.',
    entries: [
      {
        name: 'RawTherapee',
        href: 'https://github.com/RawTherapee/RawTherapee',
        what:
          'Publishes a collection of hand-made DCP camera profiles, contributed by '
          + 'their authors. When Argentum fetches a profile for your camera, it '
          + 'comes from there — from the project that published it, not from us.',
      },
    ],
  },
];
