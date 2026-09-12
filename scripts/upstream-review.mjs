// What arrived with the last upstream fetch, and which of it touches us.
//
// WHY THIS EXISTS
//
// `git merge` answers "do these texts collide". It cannot answer the question
// that actually matters to a fork: **has upstream now built the thing we built?**
// On 2026-09-13 ten upstream commits merged with zero conflicts in application
// code, and that was reported as proof the architecture works. It was not. If
// upstream had shipped their own white balance, git would have merged it in
// silence and Argentum would have had two, or theirs would quietly have won.
//
// So this prints the review list. The decision is a person's; the list is not.
//
// Run:  npm run review:upstream

import { execSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** Upstream code our hooks step around. Kept in step with check-mergeability. */
const SHADOWED = [
  { file: 'src-tauri/src/image_processing.rs', symbol: 'apply_cpu_default_raw_processing' },
  { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_white_balance' },
];

/** Keys in the adjustments JSON we added or re-typed. */
const SIDECAR_KEYS = ['showClipping', 'cameraProfile'];

/** Areas where a clean merge proves least. */
const SENSITIVE = [
  { what: 'RAW decode / white balance', re: /raw_processing|image_processing|auto_wb|white.?balance|demosaic|temperature/i },
  { what: 'lens correction', re: /lens_correction|lensfun|distortion|vignett/i },
  { what: 'camera profiles', re: /dcp|profile|colou?r.?matrix|calibration/i },
  { what: 'shaders', re: /\.wgsl|shader|gpu_processing/i },
  { what: 'export', re: /export_processing|tiff|bit.?depth|encode/i },
];

const sh = (cmd, big = false) =>
  execSync(cmd, { cwd: root, encoding: 'utf8', maxBuffer: (big ? 96 : 8) * 1024 * 1024 });

if (!existsSync(join(root, '.git'))) process.exit(0);

let base;
try {
  base = sh('git merge-base HEAD upstream/main').trim();
} catch {
  console.log('  no upstream/main ref — run: git fetch upstream');
  process.exit(0);
}

const head = sh('git rev-parse upstream/main').trim();
if (base === head) {
  console.log(`  upstream: nothing new since ${base.slice(0, 8)} — already merged up to date`);
  process.exit(0);
}

const commits = sh(`git log --format="%h%x09%s" ${base}..upstream/main`).trim().split('\n').filter(Boolean);
const diffNames = sh(`git diff --name-only ${base}..upstream/main`).trim().split('\n').filter(Boolean);
const diff = sh(`git diff ${base}..upstream/main`, true);

console.log(`\n  ${commits.length} upstream commits to review, ${base.slice(0, 8)}..${head.slice(0, 8)}\n`);

// --- Shadowed symbols: the highest-value signal -------------------------------
const shadowHits = [];
for (const { file, symbol } of SHADOWED) {
  const touched = diffNames.includes(file);
  const mentioned = diff.includes(symbol);
  if (touched || mentioned) shadowHits.push({ file, symbol, touched, mentioned });
}
if (shadowHits.length > 0) {
  console.log('  SHADOWED CODE UPSTREAM CHANGED — read these first\n');
  for (const h of shadowHits) {
    console.log(`    ${h.file}`);
    console.log(`      we step around ${h.symbol}, and upstream ${h.mentioned ? 'changed lines mentioning it' : 'edited this file'}`);
    console.log('      their fix will merge cleanly and never run here. Decide: adopt, keep ours, or combine.\n');
  }
} else {
  console.log('  shadowed code: untouched by this batch\n');
}

// --- Sidecar keys: overlap in the data format, which no merge sees ------------
const keyHits = SIDECAR_KEYS.filter((k) => diff.includes(k));
if (keyHits.length > 0) {
  console.log(`  SIDECAR KEYS upstream touched: ${keyHits.join(', ')}`);
  console.log('    We changed the meaning of these. A type change breaks silently.\n');
}

// --- Borrowed fixes upstream may now have merged ------------------------------
let borrowed = [];
try {
  borrowed = [...sh('git grep -hoE "// upstream #[0-9]+" -- src src-tauri').matchAll(/#(\d+)/g)]
    .map((m) => m[1]);
} catch { /* none */ }
const pending = [...new Set(borrowed)];
for (const pr of pending) {
  const landed = sh(`git log --format=%h ${base}..upstream/main --grep="#${pr}"`).trim();
  if (landed) {
    console.log(`  BORROWED #${pr} MAY HAVE LANDED upstream (${landed.split('\n').join(', ')})`);
    console.log('    Compare our marked block with theirs. If identical, delete the markers;');
    console.log('    if not, they changed it after we copied it.\n');
  }
}
if (pending.length > 0 && !pending.some((pr) => sh(`git log --format=%h ${base}..upstream/main --grep="#${pr}"`).trim())) {
  console.log(`  borrowed and still pending upstream: ${pending.map((p) => '#' + p).join(', ')}\n`);
}

// --- The commits themselves, flagged by area ---------------------------------
console.log('  commits:\n');
for (const line of commits) {
  const [sha, ...rest] = line.split('\t');
  const subject = rest.join('\t');
  const files = sh(`git show --name-only --format= ${sha}`).trim();
  const areas = SENSITIVE.filter((a) => a.re.test(subject) || a.re.test(files)).map((a) => a.what);
  const mark = areas.length > 0 ? '  <-- ' + areas.join(', ') : '';
  console.log(`    ${sha}  ${subject}${mark}`);
}

console.log('\n  When the merge is done, record the new base in CHANGELOG.md:');
console.log(`    **Based on RapidRAW \`x.y.z\` @ \`${head.slice(0, 8)}\`**`);
console.log('  check:merge fails until that line matches, which is what makes this');
console.log('  review happen at merge time rather than never.\n');
