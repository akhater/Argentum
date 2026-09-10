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
