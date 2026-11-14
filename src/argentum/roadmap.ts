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
      'Real chromatic adaptation in Kelvin, with an auto mode and a picker — replacing '
      + 'three fixed multipliers. Since 26.41.6 the engine and picker are RapidRAW 1.6.5\x27s '
      + 'own; the auto mode is still darktable\x27s, and Argentum\x27s. Lightroom\x27s presets '
      + 'followed in 26.41.8.',
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
      'Pick a profile per photo in Edit > Profile, or leave it on the built-in matrix, '
      + 'and give each camera the one its new photos open with in My Gear. Profiles '
      + 'are found online or imported, and applied while the photo is drawn.',
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
      'The preview now converts for the screen it is on, using that display\x27s '
      + 'own profile (Windows only for now). Without it a wide-gamut display showed every photo more '
      + 'saturated than it was, and nothing on screen said so.',
  },
  {
    stage: 'done',
    what: 'Sharpening',
    release: '26.41.11',
    why:
      'RapidRAW\x27s sharpening was one slider with no way to see what it touched. Now '
      + 'RawTherapee\x27s capture sharpening for RAW photos, darktable\x27s Sharpen, an eye '
      + 'that shows where each one works, and a Sharpen slider on every mask.',
  },
  {
    stage: 'done',
    what: 'Restore',
    release: '26.41.12',
    why:
      'A lens used wide open is soft from its own aberrations, and sharpening only goes so far. '
      + 'Restore uses an AI model trained on thousands of simulated lenses to undo that softness '
      + 'at the photo\x27s own size, only where the photo has detail, before capture sharpening.',
  },
  {
    stage: 'done',
    what: 'A Lightroom-style starting look',
    release: '26.43.1',
    why:
      'RAW photos opened dark and flat next to Lightroom or the camera\x27s own JPEG. DNG Standard '
      + 'renders them as a DNG renderer does by default - the camera\x27s exposure offset and '
      + 'Adobe\x27s tone curve, hues kept - with Lightroom\x27s default sharpening, and every RAW '
      + 'now starts there.',
  },
  {
    stage: 'planned',
    what: 'White balance on RAW files only',
    why:
      'A JPEG\x27s colours were balanced in the camera, and nothing in the file says what '
      + 'light they were balanced for, so a white balance preset on one corrects twice. '
      + 'Take it away where it cannot mean anything.',
  },
  {
    stage: 'done',
    what: 'Fast on every graphics mode',
    release: '26.43.1',
    why:
      'Auto takes Vulkan on Windows, and on some laptops that was several times slower than '
      + 'OpenGL. The cause was in how the editor drew each pixel, not in the graphics mode: '
      + 'with that fixed, Vulkan and DirectX 12 are 20 to 45 times faster, and Auto is fast.',
  },
  {
    stage: 'done',
    what: 'Masks in one click',
    release: '26.43.1',
    why:
      'Subject, Sky and Background find themselves, and Object picks what you click, with '
      + 'more clicks to add or leave out a part. RAWmakase\x27s selection, on SAM 2.1.',
  },
  {
    stage: 'done',
    what: 'B&W, Looks and Snapshots',
    release: '26.43.1',
    why:
      'Black and white with Lightroom\x27s mix and an Auto mix per photo; presets, LUTs and '
      + 'film looks in one tab; named versions of an edit kept beside it.',
  },
  {
    stage: 'done',
    what: 'History that stays',
    release: '26.43.3',
    why:
      'Undo and redo carry on after opening another photo or restarting the app. Each '
      + 'photo keeps its History in its edit file, a step at a time.',
  },
  {
    stage: 'done',
    what: 'The camera\x27s own lens correction',
    release: '26.43.4',
    why:
      'Many lenses are made to be corrected in software, from tables the camera writes '
      + 'into the file. Read for DNG, Fujifilm, Sony, Nikon, Olympus, Panasonic, Samsung '
      + 'and two Canon bodies, and applied the way Lightroom applies them.',
  },
  {
    stage: 'done',
    what: 'Lens correction as Lensfun does it',
    release: '26.46.1',
    why:
      'A Lensfun lens profile is now applied with Lensfun\x27s own maths: the right '
      + 'distortion strength, on crop sensors and crop modes, colour fringes to the '
      + 'corners, every zoom length and the whole corner darkening. Fewer wrong lens picks.',
  },
  {
    stage: 'done',
    what: 'A faster RAW decoder',
    release: '26.44.1',
    why:
      'FlashRaw, as a choice in Settings > Processing: Canon, Fujifilm, Olympus, Panasonic '
      + 'and Pentax files open 1.5 to 3.4 times faster, with identical pixels.',
  },
  {
    stage: 'planned',
    what: 'Group by date, camera or lens',
    why:
      'The library is one flat list. You can sort it, filter it, and fold a RAW and '
      + 'its JPEG into one card, but not break it into days, or cameras, or lenses, '
      + 'which is how a shoot is actually looked for.',
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
      + 'TIFF now carries 16, shipped in 26.37.20, and since 26.38.1 you choose 8 '
      + 'or 16 — but the photo still reaches the renderer at half precision, so '
      + 'the file has room the pipeline cannot yet fill. Not done until it can.',
  },
  {
    stage: 'planned',
    what: 'Resize-aware export sharpening',
    why:
      'Give exported files back the edge contrast that resizing can soften, while '
      + 'leaving the editing preview alone. Compare a sharper resize filter with a '
      + 'mild post-resize sharpening pass, then expose a simple Off / Standard / '
      + 'Strong choice and avoid sharpening twice when no resize is requested.',
  },
  {
    stage: 'building',
    what: 'Use less memory',
    why:
      'Measured on a 32MP RAW in 26.41.10: the AI mask models kept 6 GB after one '
      + 'selection, and a photo nothing had changed was copied twice. Both are '
      + 'fixed, so editing with AI settles at 3-4 GB instead of 8-12, and the '
      + 'models unload when unused, and since 26.43.4 an open RAW takes a quarter less. '
      + 'Still to do: export, where the 16-bit target '
      + 'and keeping metadata on a TIFF cost the most, and the AI eraser, which '
      + 'works on the whole photo to fix one spot.',
  },
  {
    stage: 'done',
    what: 'EXIF in an exported TIFF',
    release: '26.38.1',
    why:
      'Camera, lens, exposure, date, copyright and GPS survive a TIFF export, and '
      + 'the Keep metadata switch is shown for TIFF rather than hidden while ticked.',
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
      'Masks are a click already; the next step is a list. Look at the photo once, '
      + 'name what is in it — sky, face, the dog, the tree on the left — and tick the '
      + 'ones to mask. The model behind Subject and Sky already labels much of what it '
      + 'finds. Everything stays on this machine, and the same names make the library '
      + 'searchable by what is in a photo rather than only by filename and EXIF.',
  },
  {
    stage: 'done',
    what: 'Feather on the linear mask',
    release: '26.41.9',
    why:
      'The linear mask is two lines, where the effect is full and where it has gone, '
      + 'and how far apart they are is how soft the edge is. Both it and the radial '
      + 'mask now fade smoothly, with no line where the fade starts or stops, and the '
      + 'radial mask shows where its full effect ends.',
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
    why:
      'darktable\x27s filmic or sigmoid curve for high-contrast scenes, as a second choice '
      + 'beside the AgX tone mapper that ships today.',
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
