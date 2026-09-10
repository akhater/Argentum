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
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Paths that are ours. Anything under these is unlimited. */
const OURS = [
  'src-tauri/src/mods/',
  'src-tauri/src/shaders/modules.wgsl',
  'src/argentum/',
  'scripts/',
  'docs/',
  'CLAUDE.md',
  'CHANGELOG.md',
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
    file: 'src-tauri/src/file_management.rs',
    allow: 30,
    date: '2026-09-08',
    why: 'Sidecar rename .rrdata -> .agdata. An extension permeates their code '
      + 'by nature; no amount of moving logic to mods/ avoids it. Taken so '
      + 'RapidRAW cannot open an Argentum edit and silently render it wrong '
      + 'once our colour maths diverges.',
  },
  {
    file: 'src-tauri/src/exif_processing.rs',
    allow: 10,
    date: '2026-09-08',
    why: 'Same rename, plus one call into mods::sidecar so legacy .rrdata '
      + 'files are still read and nothing already edited is orphaned.',
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
    hooks: 3,
    what: '`mod mods`, the cache-version check, and the single `ag` command',
    instead: 'add a match arm to mods/dispatch.rs — commands cost nothing here',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    hooks: 1,
    what: 'one call to ag_stage_scene_linear',
    instead: 'add your tool inside ag_stage_scene_linear in shaders/modules.wgsl',
  },
  {
    file: 'src-tauri/src/raw_processing.rs',
    hooks: 1,
    what: 'one call to mods::decode::on_raw_decoded',
    instead: 'add a step to mods/decode.rs',
  },
  {
    file: 'src-tauri/src/image_processing.rs',
    hooks: 3,
    what: 'the CPU preview encode interception',
    instead: 'change mods/preview_encode.rs',
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
    file: 'src/components/adjustments/Color.tsx',
    hooks: 1,
    what: 'the data-argentum="color-tools" marker',
    instead: 'portal into [data-argentum="color-tools"] from Argentum.tsx',
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
 */
const isHook = (line) => {
  if (/\.agdata|\.agexif/.test(line)) return false;
  return /\bmods::|^\s*mod mods;|\bag_stage_|argentum\/|'\.\/argentum|data-argentum|<Argentum\b|\bArgentum from\b/.test(line);
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
  base = execSync('git rev-parse --verify --quiet upstream/main', {
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

// Added lines per file, ours vs upstream's tree.
//
// -w ignores whitespace, because re-indenting their code to nest it in a
// container is not divergence — git counts every moved line as added, but a
// conflict there resolves itself. What we care about is real added logic.
const numstat = execSync(`git diff -w --numstat ${base} -- .`, {
  cwd: root,
  encoding: 'utf8',
  maxBuffer: 32 * 1024 * 1024,
});

/** Added lines that call into our code, per upstream file. */
const hooksByFile = (() => {
  const diff = execSync(`git diff -w ${base} -- .`, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  const found = new Map();
  let current = null;
  for (const line of diff.split('\n')) {
    const header = line.match(/^\+\+\+ b\/(.+)$/);
    if (header) {
      current = header[1];
      continue;
    }
    if (!current || !line.startsWith('+') || line.startsWith('+++')) continue;
    if (isOurs(current) || IDENTITY.includes(current)) continue;
    const text = line.slice(1);
    if (!isHook(text)) continue;
    if (!found.has(current)) found.set(current, []);
    found.get(current).push(text.trim());
  }
  return found;
})();

const violations = [];
const touched = [];

for (const line of numstat.split('\n').filter(Boolean)) {
  const [addedRaw, , file] = line.split('\t');
  if (!file || addedRaw === '-') continue; // binary
  if (isOurs(file)) continue;

  const added = Number(addedRaw);
  if (added === 0) continue;

  const limit = budgetFor(file);
  touched.push({ file, added, limit });
  if (added > limit) violations.push({ file, added, limit });
}

// The anchor check: a file may only reach into our code a fixed number of
// times, and adding a feature must not be one of them.
const anchorBreaks = [];
for (const [file, hooks] of hooksByFile) {
  const anchor = anchorFor(file);
  const allowed = anchor ? anchor.hooks : 0;
  if (hooks.length > allowed) {
    anchorBreaks.push({ file, hooks, allowed, anchor });
  }
}

if (anchorBreaks.length > 0) {
  console.error('\n  ANCHOR ADDED TO AN UPSTREAM FILE\n');
  for (const { file, hooks, allowed, anchor } of anchorBreaks) {
    console.error(`    ${file}`);
    console.error(`      ${hooks.length} calls into our code, the anchor allows ${allowed}`);
    if (anchor) {
      console.error(`      anchor: ${anchor.what}`);
      console.error(`      do this instead: ${anchor.instead}`);
    } else {
      console.error('      this file has no anchor at all — it should have none');
    }
    for (const h of hooks) console.error(`        + ${h}`);
    console.error('');
  }
  console.error('  An anchor is a fixed cost, not a budget. If a feature needs a');
  console.error('  new one, that is the design being wrong, not the number.\n');
  console.error('  See docs/ARCHITECTURE.md, \"What the next feature costs\".\n');
  process.exit(1);
}

if (violations.length === 0) {
  if (touched.length > 0) {
    const total = touched.reduce((sum, t) => sum + t.added, 0);
    const hookCount = [...hooksByFile.values()].reduce((n, h) => n + h.length, 0);
    console.log(
      `  mergeability: ${touched.length} upstream files touched, ${total} lines, all within budget`,
    );
    console.log(
      `  anchors: ${hookCount} calls into our code across ${hooksByFile.size} of their files — a new feature should add none`,
    );
  }

  // Keep the approved trespasses visible. The point of writing them down is
  // that they stay uncomfortable, not that they get forgotten.
  const active = EXCEPTIONS.filter((e) => touched.some((t) => t.file === e.file));
  if (active.length > 0) {
    console.log(`  approved exceptions (${active.length}):`);
    for (const e of active) {
      const used = touched.find((t) => t.file === e.file)?.added ?? 0;
      console.log(`    ${e.file}  ${used}/${e.allow} lines, agreed ${e.date}`);
    }
  }
  process.exit(0);
}

console.error('\n  MERGEABILITY BUDGET EXCEEDED\n');
for (const { file, added, limit } of violations) {
  console.error(`    ${file}`);
  console.error(`      ${added} lines added, budget is ${limit}\n`);
}
console.error('  Our code belongs in our own files. Move the logic into');
console.error('    src-tauri/src/mods/   ·   src/argentum/   ·   shaders/modules.wgsl');
console.error('  and leave a single call behind in theirs.\n');
console.error('  See CLAUDE.md, "The one architectural rule".\n');
process.exit(1);
