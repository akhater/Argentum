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
    allowDeleted: 31,
    deletedWhy: 'Every deleted line is one half of the .rrdata -> .agdata '
      + 'rename and was replaced one for one on the line below it. A rename, '
      + 'not a removal: nothing of theirs stopped existing, so an upstream edit '
      + 'still lands in a line that is there under a different name. Recorded '
      + '2026-09-13, when the checker learned to see deletions at all.',
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
    why: 'Same rename, plus one call into mods::sidecar so legacy .rrdata '
      + 'files are still read and nothing already edited is orphaned.',
  },
  {
    file: 'src/components/panel/SettingsPanel.tsx',
    allow: 6,
    allowDeleted: 209,
    date: '2026-09-13',
    why: 'The lens section moved out to src/argentum/MyGear.tsx, which is the '
      + 'architecture working: the panel is ours and their file keeps a tag.',
    deletedWhy: '209 lines of their lens UI removed rather than left dormant, '
      + 'and this is the uncomfortable one. It is written down so it stays '
      + 'uncomfortable: upstream edits this file often, and any change they '
      + 'make inside the block we deleted is a conflict resolved by hand. It '
      + 'was invisible until the checker learned to count deletions on '
      + '2026-09-13. If it conflicts twice, put their section back and hide it '
      + 'instead of removing it.',
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
  const exception = exceptionFor(file);
  return exception ? exception.allow : BUDGETS.find((b) => b.pattern.test(file)).limit;
};

let base;
try {
  base = execSync('git merge-base HEAD upstream/main', {
    cwd: root,
    encoding: 'utf8',
  }).trim();
} catch {
  console.log('  mergeability: no upstream/main ref, skipping');
  process.exit(0);
}

if (!base || !existsSync(join(root, '.git'))) {
  process.exit(0);
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
    limit: budgetFor(file),
    prs: [...st.prs],
  });
}

// --- Gate: deleting their lines ---------------------------------------------
//
// The rule CLAUDE.md states is "never delete their function — just stop calling
// it". It was never enforced, and it is the one that actually causes conflicts:
// any upstream edit inside a block we removed conflicts, every time.
for (const t of touched) {
  const allowed = exceptionFor(t.file)?.allowDeleted ?? DELETION_LIMIT;
  if (t.deleted > allowed) {
    errors.push({
      file: t.file,
      detail: `${t.deleted} of their lines deleted, the limit is ${allowed}`,
      fix: 'Stop calling their code rather than removing it — or record it in EXCEPTIONS with allowDeleted and the reason.',
    });
  }
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

// --- Gate: the recorded base must be the real one ---------------------------
//
// Accepting an upstream change only means anything against the commit it was
// judged at. Forcing this line to be rewritten is what makes the review happen
// at merge time rather than never. `git fetch` does not move the merge-base, so
// this fails once per merge, not every time upstream moves.
{
  const changelog = join(root, 'CHANGELOG.md');
  if (existsSync(changelog)) {
    const recorded = readFileSync(changelog, 'utf8').match(
      /Based on RapidRAW[^@]*@\s*`([0-9a-f]{7,40})`/,
    );
    if (!recorded) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: 'no "Based on RapidRAW ... @ `sha`" line found',
        fix: 'Record the upstream commit this fork is merged up to.',
      });
    } else if (!base.startsWith(recorded[1])) {
      errors.push({
        file: 'CHANGELOG.md',
        detail: `records base ${recorded[1]}, but the merge-base is ${base.slice(0, recorded[1].length)}`,
        fix: 'Update it as part of the merge, having reviewed what arrived with it: npm run review:upstream',
      });
    }
  }
}

// --- Gate: shadowed upstream code must still exist --------------------------
for (const { file, symbol, instead } of SHADOWED) {
  let upstreamCopy = '';
  try {
    upstreamCopy = execSync(`git show upstream/main:${file}`, {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 32 * 1024 * 1024,
    });
  } catch {
    errors.push({
      file,
      detail: `we step around ${symbol} here, but upstream no longer has this file`,
      fix: 'Our hook may now be bypassing nothing. Find what replaced it.',
    });
    continue;
  }
  if (!upstreamCopy.includes(symbol)) {
    errors.push({
      file,
      detail: `${symbol} is gone from upstream, and we step around it`,
      fix: `We use ${instead}. A rename is exactly the change that would otherwise slip through — check the hook still bypasses what we think it does.`,
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
  const deleted = used.reduce((n, t) => n + t.deleted, 0);
  const hooked = [...stats.values()].filter((st) => st.hooks.length > 0);
  const hooks = hooked.reduce((n, st) => n + st.hooks.length, 0);
  console.log(`  upstream: ${used.length} files touched, ${added} of our lines added, ${deleted} of theirs deleted`);
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
      console.log(`    ${e.file}  ${t?.added ?? 0}/${e.allow} lines, agreed ${e.date}`);
    }
  }
}

for (const w of warnings) console.log(`  warning: ${w}`);
process.exit(0);
