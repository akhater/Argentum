// HARD RULE: our code lives in our own files.
//
// A fork dies when your changes and upstream's land in the same lines of the
// same files. Every update becomes an argument, you stop updating, and now
// you're maintaining a whole photo editor alone.
//
// So there is a budget on how much of upstream's code we're allowed to touch,
// and this script enforces it instead of trusting anyone to remember. It runs
// on `npm start`, and it fails the build when the budget is blown.
//
// If you need more than the budget in one of their files, that is the signal to
// move the code into a file of ours and leave a single call behind.
//
// Run standalone:  node scripts/check-mergeability.mjs

import { execSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { accountDiff } from './upstream-diff.mjs';
import { borrowMarkers, borrowStatus, detectOverlaps } from './upstream-overlaps.mjs';
import {
  REGISTRY, activeFor, borrowsOf, dependencyIndex, historicalIds, shadowsOf,
  validateRetirements,
} from './upstream-registry.mjs';
import { REVIEWS, VERDICTS, newestRange, reviewedThrough } from './upstream-decisions.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Paths that are ours. Anything under these is unlimited. */
const OURS = [
  'src-tauri/src/mods/',
  'src-tauri/src/shaders/modules.wgsl',
  'src-tauri/src/shaders/ag_display.wgsl',
  'src/argentum/',
  'scripts/',
  'docs/',
  'CLAUDE.md',
  'CHANGELOG.md',
  'CREDITS.md',
  'README.md',
  'setup.ps1',
  // Branding: regenerated wholesale from our own source art, never merged.
  'src-tauri/icons/',
  // Ours by adoption - it now carries both rule sets, and upstream changes to
  // it are additive lines we simply keep.
  '.gitignore',
  // Ours by creation: upstream has no such workflow. Their three are still
  // theirs, and the Android matrix entry we removed from each is recorded in
  // EXCEPTIONS below.
  '.github/workflows/upstream.yml',
];

/**
 * Per-file budgets for upstream code, in added lines.
 *
 * These are deliberately tight. Registering a command or calling a harvested
 * shader function costs one or two lines; anything more means the logic
 * belongs on our side of the line.
 */
const BUDGETS = [
  // Module + command registration, one line each per feature.
  { pattern: /^src-tauri\/src\/lib\.rs$/, limit: 12 },
  // One call line per harvested tool, plus the odd struct field.
  { pattern: /^src-tauri\/src\/shaders\/shader\.wgsl$/, limit: 25 },
  { pattern: /^src-tauri\/src\/image_processing\.rs$/, limit: 40 },
  { pattern: /^src-tauri\/src\/gpu_processing\.rs$/, limit: 10 },
  // One <OurComponent /> per panel.
  { pattern: /^src\/components\/.*\.tsx$/, limit: 6 },
  // Locales are the awkward case: i18next only looks in its own files, and the
  // one-time rebrand already spends ~10 lines of this. Budget covers that plus
  // a modest number of feature strings.
  //
  // When this starts failing, do NOT just raise it — give Argentum its own
  // i18n namespace under src/argentum/locales/ and merge it at init. That
  // takes our locale footprint in their files to zero, permanently.
  { pattern: /^src\/i18n\/locales\/.*\.json$/, limit: 40 },
  // Anything else of theirs we haven't thought about.
  { pattern: /.*/, limit: 8 },
];

/**
 * Trespasses we approved on purpose, with the reason and the day we agreed it.
 *
 * These are NOT a bigger budget. They're a list — printed on every run, so the
 * cost stays visible instead of quietly becoming the new normal. Each one is a
 * place a future upstream merge may fight us, and we chose it anyway.
 *
 * Adding to this list requires a real reason: the change genuinely cannot live
 * on our side of the line. "It was easier" is not a reason.
 */
const EXCEPTIONS = [
  {
    file: 'src-tauri/src/lib.rs',
    allow: 14,
    date: '2026-09-11',
    why: 'Twelve lines of structure — `mod mods`, startup, the single command, '
      + 'and the photo each render names — plus one on 2026-09-11: the display '
      + 'conversion for the screen the window is on. The native preview handed '
      + 'sRGB numbers straight to the panel, which is right only if the panel is '
      + 'sRGB; on a wide-gamut display every photo was shown over-saturated. '
      + 'This is where the transform reaches the GPU, so it is where the screen '
      + 'has to be named. A second line refreshes that conversion when the window '
      + 'moves: the frontend only sends a transform when its own idea of one '
      + 'changes, which does not include which monitor the window is on.',
  },
  {
    file: 'src-tauri/src/file_management.rs',
    allow: 31,
    // The removal bookkeeping that briefly lived here is in
    // scripts/upstream-registry.mjs now, under sidecar-agdata. It was never the
    // right place: a count of deleted lines cannot say whether behaviour was
    // preserved, and here it said "31 replaced, nothing missing" about a rename
    // that deliberately orphans every file RapidRAW has ever written.
    date: '2026-09-10',
    why: 'Sidecar rename .rrdata -> .agdata (30 lines, agreed 2026-09-08). An '
      + 'extension permeates their code by nature; no amount of moving logic to '
      + 'mods/ avoids it. Taken so RapidRAW cannot open an Argentum edit and '
      + 'silently render it wrong once our colour maths diverges. '
      + 'One more line on 2026-09-10: the photo argument on the render call. '
      + 'Rendering has to know which photo it is holding, because the camera '
      + 'profile correction is derived from the matrix that photo was decoded '
      + 'with. It is an argument rather than something inferred so the compiler '
      + 'asks the question at every call site — an earlier version guessed from '
      + 'thread history and was silently wrong wherever a render reused pixels '
      + 'somebody else had decoded.',
  },
  {
    file: 'src-tauri/src/exif_processing.rs',
    allow: 10,
    date: '2026-09-08',
    // Corrected 2026-09-13. This said "one call into mods::sidecar so legacy
    // .rrdata files are still read and nothing already edited is orphaned".
    // There is no mods::sidecar, there never was, and nothing reads a legacy
    // sidecar: read_rrexif_sidecar keeps its name and reads .agexif. The
    // orphaning is real and intended, and is registered as such.
    why: 'The .rrdata -> .agdata and .rrexif -> .agexif rename, plus one call '
      + 'into mods::makernote_lens, which fills the lens model from the '
      + 'manufacturer note when the standard tag is empty.',
  },
  {
    file: 'src/components/panel/SettingsPanel.tsx',
    allow: 6,
    date: '2026-09-13',
    why: 'The lens section moved out to src/argentum/MyGear.tsx, which is the '
      + 'architecture working: the panel is ours and their file keeps a tag.',
  },
];


/**
 * THE ANCHORS — the real rule, of which the budgets above are only a backstop.
 *
 * A line budget answers "how much of their code have we touched". It does not
 * answer the question that decides whether this fork is still alive in two
 * years: **what does the next feature cost?**
 *
 * If every feature adds a line to one of their files, the budget is a countdown.
 * Four commands is four lines of `lib.rs` and looks free; a hundred is a hundred
 * lines in the file upstream also edits every release.
 *
 * So each of their files gets a *fixed* number of hooks into our code — an
 * anchor — and everything after that is routed through it on our side. The
 * count below is the number of lines in that file that may mention Argentum.
 * It is not a budget to spend. It does not go up when a feature is added,
 * because a feature must not need it to.
 *
 * Rebrand lines (`.agdata`, "Argentum" inside their own sentences) are counted
 * separately and ignored here: they happened once and do not grow.
 */
const ANCHORS = [
  {
    file: 'src-tauri/src/lib.rs',
    hooks: 5,
    what:
      '`mod mods`, the cache-version check, the single `ag` command, the display '
      + 'conversion for the screen the window is on, and the refresh when the window '
      + 'moves to another screen',
    instead: 'add a match arm to mods/dispatch.rs — commands cost nothing here',
  },
  {
    file: 'src-tauri/src/shaders/display.wgsl',
    hooks: 1,
    what: 'one call to ag_stage_present, the presentation stage',
    instead: 'add your tool inside ag_stage_present in shaders/ag_display.wgsl',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    hooks: 2,
    what: 'one call to ag_stage_scene_linear, one to ag_stage_display',
    instead: 'add your tool inside one of those two stages in shaders/modules.wgsl',
  },
  {
    file: 'src-tauri/src/raw_processing.rs',
    hooks: 1,
    what: 'one call to mods::decode::on_raw_decoded',
    instead: 'add a step to mods/decode.rs',
  },
  {
    // One import, and it has to stay one. Everything the export render does
    // differently is reachable from the Precision value it carries: the storage
    // format, the shader text, the pipeline constant that silences the dither,
    // the bytes per pixel of the readback. A second hook here would mean some of
    // that decision had been written into their file instead of ours.
    file: 'src-tauri/src/gpu_processing.rs',
    hooks: 1,
    what: 'the import of Precision, which the processor carries and reads from',
    instead: 'add a method to mods/export_precision.rs - the processor already has a Precision',
  },
  {
    file: 'src-tauri/src/export_processing.rs',
    hooks: 1,
    what: 'the import of Precision and render_high_precision',
    instead: 'which format gets which precision is decided in Precision::for_path - change it there',
  },
  {
    file: 'src-tauri/src/image_processing.rs',
    hooks: 4,
    what: 'the CPU preview encode interception, and the clipping view mode',
    instead: 'change mods/preview_encode.rs, or mods/clipping.rs',
  },
  {
    file: 'src/App.tsx',
    hooks: 2,
    what: 'the single <Argentum /> mount',
    instead: 'add a portal in src/argentum/Argentum.tsx',
  },
  // UI markers. One per panel, never one per feature: everything Argentum shows
  // in that panel portals into the same marker.
  {
    file: 'src/components/panel/right/MetadataPanel.tsx',
    hooks: 1,
    what: 'the data-argentum="camera-details" marker',
    instead: 'portal into [data-argentum="camera-details"] from Argentum.tsx',
  },
  {
    // Two, and two is the ceiling: a panel has an inline slot (in a heading
    // row, for buttons) and a block slot (below the controls, for a section).
    // Those are positions, not features — every Argentum colour control mounts
    // into one of them. A third would mean a feature bought its own, which is
    // the growth this whole file exists to prevent.
    file: 'src/components/adjustments/Color.tsx',
    hooks: 2,
    what: 'the color-tools (inline) and camera-profile (block) markers',
    instead: 'portal into one of the two existing markers from Argentum.tsx',
  },
  {
    // One marker, and their file renders it only while TIFF is the chosen
    // format — so the control appears and disappears without Argentum reading
    // any state of theirs, and without a second hook to tell it when to show.
    // Upstream's own version of this feature (#1466) instead puts the setting on
    // ExportSettings and threads it through six of their files; this is the same
    // feature for one line.
    file: 'src/components/panel/right/ExportPanel.tsx',
    hooks: 2,
    what:
      'the data-argentum="export-precision" marker rendered only for TIFF, and the '
      + 'useTiffDepth import that lets their size estimate see the depth change',
    instead:
      'portal into the marker from Argentum.tsx — every future export control mounts '
      + 'there. The second hook is not a second feature and does not go up again: '
      + 'estimatedSize is useState inside their component and the debounced estimator '
      + 'is a useMemo beside it, so an effect dependency is the only way in. Any later '
      + 'control publishes into src/argentum/exportDepth.ts and rides the same one.',
  },
  {
    file: 'src/components/panel/SettingsPanel.tsx',
    hooks: 1,
    what: 'the About tab, an empty div Argentum fills',
    instead: 'add a section to src/argentum/AboutPanel.tsx — or a card of its own '
      + 'inside [data-argentum="settings-about"]',
  },
  // Behaviour, not UI. A portal can add a control; it cannot change what
  // happens when the user clicks one of theirs. These replace the body of an
  // existing handler, so they are one-time replacements rather than additions —
  // if one of these ever needs a *second* hook, the injection is in the wrong
  // place and should become an event our code listens for.
  {
    file: 'src/components/panel/editor/ImageCanvas.tsx',
    hooks: 2,
    what: 'the white balance picker solving in Rust',
    instead: 'change src/argentum/whiteBalance.ts',
  },
  {
    file: 'src/components/panel/right/CropPanel.tsx',
    hooks: 1,
    what: 'lens auto-detection running on load, not only on click',
    instead: 'change src/argentum/useAutoDetectOnLoad.ts',
  },
  {
    file: 'src-tauri/src/exif_processing.rs',
    hooks: 1,
    what: 'reading the lens name out of the maker note',
    instead: 'change mods/makernote_lens.rs',
  },
  {
    file: 'src-tauri/src/lens_correction.rs',
    hooks: 1,
    what: 'matching a lens profile for the body that shot the frame',
    instead: 'change mods/lens_crop.rs',
  },
];

/**
 * Files that carry the fork's identity rather than its features.
 *
 * The name, the bundle id, the crate. These say "this is Argentum, not
 * RapidRAW" — they were written once, they will fight an upstream merge once,
 * and no feature will ever add to them. Counting them as anchors would make the
 * number meaningless.
 */
const IDENTITY = [
  'package.json',
  'src-tauri/Cargo.toml',
  // Generated from Cargo.toml, which is identity. It changes when the crate is
  // renamed and when a dependency moves, and neither is a feature.
  'src-tauri/Cargo.lock',
  'src-tauri/tauri.conf.json',
  'src-tauri/.identity',
  'src-tauri/src/main.rs',
];

/**
 * Does this added line reach into Argentum's code?
 *
 * A hook is a call, an import or a mount point — a place their code depends on
 * ours. The rebrand is not: `.agdata`, or "Argentum" appearing inside a
 * sentence they already had. Those are edits to text, and text does not grow
 * with the number of features.
 *
 * The shader pattern used to be `ag_stage_` alone, which meant a call to any
 * other `ag_` function in their WGSL was not counted at all — a hole found by
 * walking through it. It matches any `ag_…(` call now.
 */
const isHook = (line) => {
  if (/\.agdata|\.agexif/.test(line)) return false;
  return /\bmods::|^\s*mod mods;|\bag_[a-z0-9_]*\(|argentum\/|'\.\/argentum|data-argentum|<Argentum\b|\bArgentum from\b/.test(line);
};

const anchorFor = (file) => ANCHORS.find((a) => a.file === file);

const isOurs = (file) => OURS.some((prefix) => file.startsWith(prefix));
const exceptionFor = (file) => EXCEPTIONS.find((e) => e.file === file);
const budgetFor = (file) => {
  // `allow` is optional: an exception that only records removed lines leaves the
  // added-line budget where it was.
  const exception = exceptionFor(file);
  return exception?.allow ?? BUDGETS.find((b) => b.pattern.test(file)).limit;
};

// stderr is captured rather than inherited: every probe here is allowed to
// fail, and git's "fatal: Not a valid object name" is noise above our own
// message saying the same thing in a useful way.
const git = (cmd, big = false) =>
  execSync(cmd, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: (big ? 96 : 8) * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

/**
 * Prerequisites.
 *
 * Everything below is measured against upstream/main. Without that ref there is
 * nothing to compare against and this script used to exit 0 — the right answer
 * on a contributor's fresh clone, and the wrong one in CI, where a vacuous pass
 * is indistinguishable from a real one and the check is a required one.
 *
 * So: skip locally, fail in CI. The workflow is responsible for adding the
 * remote and fetching enough history; if it stops doing so, the build says so
 * instead of going green.
 */
const inCI = process.env.CI === 'true' || process.env.CI === '1';
const prerequisite = (detail, fix) => {
  if (inCI) {
    console.error('');
    console.error('  UPSTREAM CHECK CANNOT RUN');
    console.error('');
    console.error(`    ${detail}`);
    console.error(`    ${fix}`);
    console.error('');
    process.exit(1);
  }
  console.log(`  mergeability: ${detail} — skipping`);
  process.exit(0);
};

if (!existsSync(join(root, '.git'))) {
  prerequisite('not a git repository', 'Nothing to compare against.');
}

let shallow = 'false';
try {
  shallow = git('git rev-parse --is-shallow-repository').trim();
} catch { /* old git: assume full */ }
if (shallow === 'true') {
  prerequisite(
    'the clone is shallow, so no merge-base with upstream can be computed',
    'actions/checkout needs fetch-depth: 0.',
  );
}

let base = '';
try {
  base = git('git merge-base HEAD upstream/main').trim();
} catch {
  prerequisite(
    'no upstream/main ref',
    'git remote add upstream https://github.com/CyberTimon/RapidRAW.git && git fetch upstream',
  );
}
if (!base) {
  prerequisite('merge-base with upstream/main is empty', 'Fetch upstream: git fetch upstream');
}


// SHADOWED, SIDECAR_KEYS and the borrow markers moved to
// scripts/upstream-overlaps.mjs on 2026-09-13. Both this script and
// upstream-review.mjs need them, both kept their own copy, and the copies had
// already drifted: one of them knew what we use instead of each shadowed
// symbol and the other did not.

// One pass over the real diff. Deliberately NOT `-w`: git merge does not
// ignore whitespace, so a number that does is not the number to look at. It
// reported Color.tsx as 4 lines where git sees 17 added and 13 deleted.
//
// The accounting itself is in scripts/upstream-diff.mjs so it can be tested
// against throwaway repositories. Inline and untestable, it missed deleted
// files entirely: `+++ /dev/null` did not match its header pattern, so removing
// a whole upstream component changed this script's output by nothing at all.
const diff = execSync(`git diff ${base} -- .`, {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 96 * 1024 * 1024,
});

const stats = accountDiff(diff, { isOurs, skip: IDENTITY, isHook });

const warnings = [];
const errors = [];
const touched = [];

for (const [file, st] of stats) {
  // Borrowed lines are upstream's own fix carried early. They are not our
  // divergence: when upstream merges the pull request they become identical to
  // their copy, so they are not charged against the file's budget.
  const ours = st.added - st.borrowed;
  touched.push({
    file,
    added: ours,
    borrowed: st.borrowed,
    deleted: st.deleted,
    removed: st.removed,
    replaced: st.replaced,
    borrowedRemoved: st.borrowedRemoved,
    limit: budgetFor(file),
    prs: [...st.prs],
  });
}

// --- Gate: every file of theirs we touch must be claimed --------------------
//
// Line counts do not prove behaviour was preserved, and the counter-example is
// sitting in this repository. `get_all_adjustments_from_json` gained a fifth
// parameter and five call sites across four of their files were rewritten one
// line for one. By any arithmetic nothing was added and nothing was removed, and
// the whole export path now depends on a signature upstream owns.
//
// So the counts are warnings, and this is the gate: if we changed a file of
// theirs, some entry in scripts/upstream-registry.mjs must name it as a
// dependency. Otherwise nobody has said what we are relying on, and no upstream
// change to it will ever be reviewed.
{
  const index = dependencyIndex(REGISTRY);
  for (const t of touched) {
    if (t.added === 0 && t.deleted === 0 && t.borrowed === 0) continue;
    if (index.claims(t.file) || exceptionFor(t.file)) continue;
    errors.push({
      file: t.file,
      detail: 'we changed this file of theirs and no registry entry claims it',
      fix: 'Name it in the dependsOn of whichever feature needs it, in '
        + 'scripts/upstream-registry.mjs, with a how and a note. If nothing needs '
        + 'it, revert the change.',
    });
  }
}

// --- Gate: registry integrity ----------------------------------------------
//
// The requirement to review a borrowed fix must not be erasable by deleting the
// marker that creates it. Detection reads the working tree; a merge that removed
// `// upstream #1307` would, without this, remove the reason anyone had to think
// about it. So the tree and the registry have to agree, and retiring an entry is
// a decision recorded in a review rather than an edit that quietly happens.
{
  const marks = borrowMarkers(git);
  const registered = borrowsOf(REGISTRY);
  for (const { file, pr } of marks.pairs) {
    if (registered.some((b) => b.file === file && b.pr === pr)) continue;
    errors.push({
      file,
      detail: `carries a // upstream #${pr} marker that no registry entry declares`,
      fix: 'Add a borrowed-fix entry in scripts/upstream-registry.mjs, or remove the marker.',
    });
  }
  const live = new Set(activeFor(REGISTRY, REVIEWS, null).map((e) => e.id));
  for (const { entry, file, pr } of registered) {
    // Retired and behind us: the marker is expected to be gone.
    if (entry.retired && !live.has(entry.id)) continue;
    if (marks.pairs.some((m) => m.file === file && m.pr === pr)) continue;
    errors.push({
      file: 'scripts/upstream-registry.mjs',
      detail: `${entry.id} claims a // upstream #${pr} marker in ${file}, and there is none`,
      fix: 'If upstream merged it and the block is gone, retire the entry with '
        + 'retired: { recordedIn, why } in the review that decided it - which keeps '
        + 'its review requirement for that window, where it belongs.',
    });
  }
}

// --- Warning: what the line counts say --------------------------------------
//
// Informational, deliberately. A removal is a signal worth printing and never
// again a gate on its own: a line replaced in place can change everything, and a
// line removed can change nothing.
for (const t of touched) {
  if (t.removed > 0) {
    warnings.push(
      `${t.file}: ${t.removed} of their lines removed unreplaced, ${t.replaced} replaced `
      + '- see the registry entry that claims this file for what it means',
    );
  }
}

// --- Gate: the inventory is append-only -------------------------------------
//
// Retiring an entry keeps its requirement in sight. Deleting the entry and its
// markers in the same commit did not: with nothing in the tree and nothing in
// the registry there was nothing left to disagree about, and the requirement
// stopped existing. The thing under review could delete its own review.
//
// So every id this file has ever held, read back out of our own history, must
// still be here. git is the previous inventory - there is no second register to
// drift, and no way to edit what we used to depend on without rewriting history.
{
  const ever = historicalIds(git);
  if (ever === null) {
    warnings.push('cannot read the history of scripts/upstream-registry.mjs, so '
      + 'a deleted entry would go unnoticed in this run');
  } else {
    const now = new Set(REGISTRY.map((e) => e.id));
    for (const id of ever) {
      if (now.has(id)) continue;
      errors.push({
        file: 'scripts/upstream-registry.mjs',
        detail: `${id} was in the inventory and has been deleted from it`,
        fix: 'Entries are never deleted. Put it back and retire it with '
          + 'retired: { recordedIn, why }, plus a retire: decision in that review.',
      });
    }
  }
}

// --- Gate: retirements must hold up ----------------------------------------
//
// A retirement is how an entry stops generating requirements, so it is the
// obvious thing to fake. It names a review that exists, and that review records
// the decision, with a reason, like any other.
for (const problem of validateRetirements(REGISTRY, REVIEWS)) {
  errors.push({
    file: 'scripts/upstream-registry.mjs',
    detail: `${problem.id} ${problem.detail}`,
    fix: problem.fix,
  });
}

// --- Gate: anchors ----------------------------------------------------------
for (const [file, st] of stats) {
  if (st.hooks.length === 0) continue;
  const anchor = anchorFor(file);
  const allowed = anchor ? anchor.hooks : 0;
  if (st.hooks.length > allowed) {
    errors.push({
      file,
      detail: `${st.hooks.length} calls into our code, the anchor allows ${allowed}`,
      fix: anchor
        ? `${anchor.what} — do this instead: ${anchor.instead}`
        : 'This file has no anchor at all, and should have none.',
      lines: st.hooks,
    });
  }
}

// --- Gate: overlap review ---------------------------------------------------
//
// The question git cannot answer is whether upstream has now built, moved or
// renamed the thing we built around. Those changes conflict with nothing.
//
// The first version of this gate asked only that CHANGELOG.md name the current
// merge-base, on the theory that having to rewrite the line would make somebody
// look. It would not: the line can be corrected with one `sed`, and the review
// script it pointed at started from the merge-base, so after the merge it said
// "nothing new" whether or not anybody had looked. Merging erased the window.
//
// So the reviewed-through point and the decisions live together in
// scripts/upstream-decisions.mjs, the overlaps are re-derived here from git and
// the registry, and the sha cannot advance until each one has a decision beside
// it. CHANGELOG.md is checked against the register rather than written
// independently - one register, which was the whole objection to having one.
const short = (sha) => sha.slice(0, 8);
{
  const declared = reviewedThrough();
  let through = '';
  try {
    through = git(`git rev-parse --verify ${declared}`).trim();
  } catch {
    errors.push({
      file: 'scripts/upstream-decisions.mjs',
      detail: `REVIEWS records through ${short(declared)}, which is not a commit here`,
      fix: 'Fetch upstream, or correct the sha. In CI this means fetch-depth: 0.',
    });
  }

  if (through && through !== base) {
    errors.push({
      file: 'scripts/upstream-decisions.mjs',
      detail: `reviewed through ${short(through)}, but upstream is merged in up to ${short(base)}`,
      fix: 'npm run review:upstream - then add a REVIEWS entry whose through is the new sha, carrying a decision for every overlap it lists.',
    });
  }

  // Only the newest entry's range is re-derived. Older entries are records of
  // what was known then; re-deriving them against today's registry would invent
  // overlaps nobody could have seen.
  const { from, to, entry } = newestRange();
  if (from) {
    // The inventory as it stood BEFORE this window. An entry retired during the
    // window still owes its review: retiring it is the decision under review.
    const entries = activeFor(REGISTRY, REVIEWS, to);
    let overlaps = [];
    try {
      overlaps = detectOverlaps(git, from, to, { entries, index: dependencyIndex(entries) });
    } catch {
      errors.push({
        file: 'scripts/upstream-decisions.mjs',
        detail: `cannot re-derive the overlaps for ${short(from)}..${short(to)}`,
        fix: 'The history for that range is missing. Fetch upstream with full depth.',
      });
    }
    const decisions = entry.decisions ?? [];
    const recorded = new Map(decisions.map((d) => [d.overlap, d]));

    for (const o of overlaps.filter((x) => x.gated)) {
      const decision = recorded.get(o.key);
      if (!decision) {
        errors.push({
          file: 'scripts/upstream-decisions.mjs',
          detail: `no decision for ${o.key}`,
          lines: [`${o.commit}  ${o.subject}`, o.detail],
          fix: 'Read it, decide, and add { overlap, verdict, why } to the newest REVIEWS entry.',
        });
        continue;
      }
      if (!VERDICTS.includes(decision.verdict)) {
        errors.push({
          file: 'scripts/upstream-decisions.mjs',
          detail: `${o.key} has verdict "${decision.verdict}"`,
          fix: `One of: ${VERDICTS.join(', ')}.`,
        });
      }
      if (!decision.why || decision.why.trim().length < 20) {
        errors.push({
          file: 'scripts/upstream-decisions.mjs',
          detail: `${o.key} has no reasoning`,
          fix: 'A verdict with no why is a rubber stamp. Say what you read and what you concluded.',
        });
      }
    }

    // The part no detector can do. File matching finds upstream changing
    // something we registered; it cannot find upstream building the same feature
    // somewhere we have never touched. This is the human claim, and nothing here
    // verifies it beyond insisting that it was made.
    const fr = entry.featureReview;
    if (!fr || !fr.why || fr.why.trim().length < 40) {
      errors.push({
        file: 'scripts/upstream-decisions.mjs',
        detail: 'the newest review has no featureReview',
        fix: 'Read the batch for features upstream may have built independently of our '
          + 'files, and record featureReview: { verdict, why } saying what you looked at '
          + 'and what you concluded. No detector does this part.',
      });
    } else if (!['none', 'overlap-found'].includes(fr.verdict)) {
      errors.push({
        file: 'scripts/upstream-decisions.mjs',
        detail: `featureReview verdict "${fr.verdict}" is not none or overlap-found`,
        fix: 'none: nothing upstream duplicates a feature of ours. overlap-found: it does, and the decisions above say what happened.',
      });
    }

    for (const d of decisions) {
      if (!overlaps.some((o) => o.key === d.overlap)) {
        warnings.push(
          `scripts/upstream-decisions.mjs: decision for ${d.overlap} matches no overlap in `
          + `${short(from)}..${short(to)} - the registry moved under it`,
        );
      }
    }
  }

  // CHANGELOG.md is documentation of the register, checked against it.
  const changelog = join(root, 'CHANGELOG.md');
  if (existsSync(changelog)) {
    const recorded = readFileSync(changelog, 'utf8').match(
      /Based on RapidRAW[^@]*@\s*`([0-9a-f]{7,40})`/,
    );
    if (!recorded) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: 'no "Based on RapidRAW ... @ `sha`" line found',
        fix: 'Record the upstream commit this fork is reviewed up to.',
      });
    } else if (through && !through.startsWith(recorded[1])) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: `says ${recorded[1]}, the review register says ${short(through)}`,
        fix: 'The register is the source. Make this line agree with it.',
      });
    }
  }
}

// --- Warning: upstream commits nobody has merged yet ------------------------
//
// Overlaps in commits we have NOT merged are a reading list, not a failure.
// Failing on them would break `npm start` every time upstream pushes something
// unrelated, and a check that fires on other people's schedule is a check that
// gets switched off - which is the exact end this whole file exists to avoid.
try {
  const head = git('git rev-parse upstream/main').trim();
  if (head !== base) {
    // The window nobody has recorded yet: every retirement on the books is
    // behind it, so retired entries are out.
    const entries = activeFor(REGISTRY, REVIEWS, null);
    const ahead = detectOverlaps(git, base, head, { entries, index: dependencyIndex(entries) })
      .filter((o) => o.gated);
    if (ahead.length > 0) {
      const one = ahead.length === 1;
      warnings.push(
        `${ahead.length} unmerged upstream change${one ? '' : 's'} `
        + `${one ? 'lands' : 'land'} on code of ours - npm run review:upstream`,
      );
    }
  }
} catch { /* no upstream ref: handled above */ }

// --- Gate: shadowed upstream code must still exist --------------------------
for (const { entry, file, symbol } of shadowsOf(REGISTRY)) {
  let upstreamCopy = '';
  try {
    upstreamCopy = git(`git show upstream/main:${file}`, true);
  } catch {
    errors.push({
      file,
      detail: `${entry.id} steps around ${symbol} here, but upstream no longer has this file`,
      fix: 'Our hook may now be bypassing nothing. Find what replaced it.',
    });
    continue;
  }
  if (!upstreamCopy.includes(symbol)) {
    errors.push({
      file,
      detail: `${symbol} is gone from upstream, and ${entry.id} steps around it`,
      fix: 'A rename is exactly the change that would otherwise slip through - check the hook still bypasses what we think it does.',
    });
  }
}


// --- Warning: the budget ----------------------------------------------------
//
// Demoted from a gate on 2026-09-13. It counts additions only, so it cannot see
// the deletions that actually conflict, and it charges a line in a file upstream
// touches once a year the same as one it touches weekly. Kept as a warning
// because it is still the only thing that notices logic quietly growing inside
// one of their functions, which the anchor rule cannot see.
for (const t of touched) {
  if (t.added > t.limit) {
    warnings.push(`${t.file}: ${t.added} of our lines added, the budget is ${t.limit}`);
  }
}

// --- Warning: is upstream/main even current? --------------------------------
try {
  const when = execSync('git log -1 --format=%ct upstream/main', { cwd: root, encoding: 'utf8' }).trim();
  const days = Math.floor((Date.now() / 1000 - Number(when)) / 86400);
  if (days > 14) {
    warnings.push(`upstream/main is ${days} days old — this check passes vacuously against a stale ref. Run: git fetch upstream`);
  }
} catch {
  /* no upstream ref: handled above */
}

// --- Report -----------------------------------------------------------------
if (errors.length > 0) {
  console.error('\n  UPSTREAM CHECK FAILED\n');
  for (const e of errors) {
    console.error(`    ${e.file}`);
    console.error(`      ${e.detail}`);
    if (e.lines) for (const l of e.lines) console.error(`        + ${l}`);
    console.error(`      ${e.fix}\n`);
  }
  console.error('  See CLAUDE.md, "The one architectural rule".\n');
  process.exit(1);
}

const used = touched.filter((t) => t.added > 0 || t.deleted > 0 || t.borrowed > 0);
if (used.length > 0) {
  const added = used.reduce((n, t) => n + t.added, 0);
  const removed = used.reduce((n, t) => n + t.removed, 0);
  const replaced = used.reduce((n, t) => n + t.replaced, 0);
  const hooked = [...stats.values()].filter((st) => st.hooks.length > 0);
  const hooks = hooked.reduce((n, st) => n + st.hooks.length, 0);
  console.log(`  upstream: ${used.length} files touched, ${added} of our lines added`);
  console.log(`  their lines: ${removed} removed unreplaced, ${replaced} replaced in place - counts, not evidence`);
  console.log(`  anchors: ${hooks} calls into our code across ${hooked.length} of their files — a new feature should add none`);

  const borrowed = used.filter((t) => t.borrowed > 0);
  if (borrowed.length > 0) {
    console.log(`  borrowed from upstream, pending their merge (${borrowed.length}):`);
    for (const b of borrowed) {
      console.log(`    ${b.file}  ${b.borrowed} lines, ${b.prs.map((n) => `#${n}`).join(', ')}`);
    }
  }

  const active = EXCEPTIONS.filter((e) => used.some((t) => t.file === e.file));
  if (active.length > 0) {
    console.log(`  approved exceptions (${active.length}):`);
    for (const e of active) {
      const t = used.find((x) => x.file === e.file);
      const parts = [];
      if (e.allow !== undefined) parts.push(`${t?.added ?? 0}/${e.allow} added`);
      if (e.allowDeleted !== undefined) parts.push(`${t?.removed ?? 0}/${e.allowDeleted} removed`);
      console.log(`    ${e.file}  ${parts.join(', ')}, agreed ${e.date}`);
    }
  }
}

for (const w of warnings) console.log(`  warning: ${w}`);
process.exit(0);
