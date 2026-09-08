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

if (violations.length === 0) {
  if (touched.length > 0) {
    const total = touched.reduce((sum, t) => sum + t.added, 0);
    console.log(
      `  mergeability: ${touched.length} upstream files touched, ${total} lines, all within budget`,
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
