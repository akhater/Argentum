// Which upstream commits touch something we have already touched.
//
// WHY THIS EXISTS
//
// `git merge` answers "do these texts collide". It cannot answer the question a
// fork lives or dies on: **has upstream now built, moved or renamed the thing we
// built around?** Those changes conflict with nothing. They merge in silence and
// leave our hook bypassing code that no longer does what we thought.
//
// So the overlaps are derived mechanically here, keyed to the exact upstream
// commit that caused them, and scripts/upstream-decisions.mjs records what was
// decided about each one. check-mergeability.mjs fails until every gated overlap
// in the range being recorded has a decision. The list is mechanical; the
// decision is not, and nothing here pretends otherwise — it only makes the
// decision impossible to skip silently.
//
// Both scripts import this so there is one definition of "overlap". The two
// used to keep their own copies of SHADOWED and they had already drifted.

/** Upstream code our hooks step around, and what we use instead. */
export const SHADOWED = [
  {
    file: 'src-tauri/src/image_processing.rs',
    symbol: 'apply_cpu_default_raw_processing',
    instead: 'mods::preview_encode',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    symbol: 'apply_white_balance',
    instead: 'ag_stage_scene_linear in shaders/modules.wgsl',
  },
];

/** Keys in the adjustments JSON we added or re-typed. A type change is silent. */
export const SIDECAR_KEYS = ['showClipping', 'cameraProfile'];

/** Areas where a clean merge proves least. Listed for a human, not gated. */
export const SENSITIVE = [
  { slug: 'raw-wb', what: 'RAW decode / white balance', re: /raw_processing|image_processing|auto_wb|white.?balance|demosaic|temperature/i },
  { slug: 'lens', what: 'lens correction', re: /lens_correction|lensfun|distortion|vignett/i },
  { slug: 'profiles', what: 'camera profiles', re: /dcp|profile|colou?r.?matrix|calibration/i },
  { slug: 'shaders', what: 'shaders', re: /\.wgsl|shader|gpu_processing/i },
  { slug: 'export', what: 'export', re: /export_processing|tiff|bit.?depth|encode/i },
  // Added 2026-09-13: upstream 8737fc4e rewrote the thumbnail/transform hashing
  // that a borrowed fix of ours sits inside, and nothing flagged it. Caching is
  // where a silent divergence shows up as "the wrong picture", not as a crash.
  { slug: 'cache', what: 'cache keys', re: /cache_utils|calculate_\w*hash|invalidat|thumbnail.*hash/i },
];

/** The gated kinds: a mechanical fact says their change lands on our change. */
export const GATED = ['shadow', 'borrow', 'sidecar'];

/**
 * Files carrying a `// upstream #NNNN` marker, and the PR numbers in them.
 *
 * A borrowed fix is upstream's own work carried early. Two things can happen to
 * it: they merge the pull request, or they fix the same bug themselves in a
 * commit that names no number at all. The second is the common case in RapidRAW,
 * where most commits are the maintainer's own with plain subjects, so matching
 * on "#1307" alone finds nothing and reports "still pending" with confidence it
 * has not earned. Watching the *file* catches both.
 */
export function borrowMarkers(run) {
  let out = '';
  try {
    out = run('git grep -nE "// upstream #[0-9]+" -- src src-tauri');
  } catch {
    return { files: [], prs: [] };
  }
  const files = new Set();
  const prs = new Set();
  for (const line of out.split('\n')) {
    const m = line.match(/^([^:]+):\d+:.*\/\/\s*upstream #(\d+)/);
    if (!m) continue;
    files.add(m[1]);
    prs.add(m[2]);
  }
  return { files: [...files], prs: [...prs] };
}

/**
 * Every overlap between `from..to` of upstream and the fork as it stands now.
 *
 * `run` executes a git command in the repository and returns stdout.
 */
export function detectOverlaps(run, from, to, { shadowed = SHADOWED, sidecarKeys = SIDECAR_KEYS, sensitive = SENSITIVE, borrow } = {}) {
  const range = `${from}..${to}`;
  const log = run(`git log --format="%H%x09%s" ${range}`).trim();
  if (!log) return [];
  const marks = borrow ?? borrowMarkers(run);
  const overlaps = [];

  for (const entry of log.split('\n')) {
    const [sha, ...rest] = entry.split('\t');
    const subject = rest.join('\t');
    const short = sha.slice(0, 8);
    const files = run(`git show --name-only --format= ${sha}`)
      .trim().split('\n').filter(Boolean);
    let patch = '';
    try {
      patch = run(`git show --format= ${sha}`, true);
    } catch { /* enormous commit: fall back to filenames only */ }

    const add = (kind, target, detail) => overlaps.push({
      key: `${short}:${kind}:${target}`,
      commit: short,
      subject,
      kind,
      target,
      detail,
      gated: GATED.includes(kind),
    });

    for (const s of shadowed) {
      const touched = files.includes(s.file);
      // A call or a definition, not a name in a `use` list. Import reshuffles
      // mention every symbol in the module and say nothing about any of them;
      // a rename is caught by the separate gate that checks the symbol still
      // exists upstream at all.
      const mentioned = new RegExp(
        '(' + /fn\s+/.source + '|[^A-Za-z0-9_])' + s.symbol + /\s*\(/.source,
      ).test(patch);
      if (!touched && !mentioned) continue;
      add('shadow', `${s.file}#${s.symbol}`,
        `we step around ${s.symbol} and use ${s.instead}; upstream `
        + `${mentioned ? 'changed lines mentioning it' : 'edited this file'}. `
        + 'Their fix will merge cleanly and never run here.');
    }

    for (const file of marks.files ?? []) {
      if (!files.includes(file)) continue;
      add('borrow', file,
        `we carry a borrowed upstream fix in this file and upstream edited it. `
        + 'Compare the marked block with theirs: if they have fixed it, drop our '
        + 'markers; if they restructured around it, our block may now be dead.');
    }
    for (const pr of marks.prs ?? []) {
      if (!new RegExp('#' + pr + '(?![0-9])').test(subject)) continue;
      add('borrow', `#${pr}`, `the pull request we borrowed appears to have landed.`);
    }

    for (const key of sidecarKeys) {
      if (!patch.includes(key)) continue;
      add('sidecar', key,
        `we changed the meaning of this key. A type change breaks silently — `
        + 'nothing fails to compile and nothing fails to merge.');
    }

    for (const area of sensitive) {
      if (!area.re.test(subject) && !area.re.test(files.join('\n'))) continue;
      add('area', area.slug, area.what);
    }
  }
  return overlaps;
}
