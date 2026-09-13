// Everything Argentum has that upstream does not, and what of theirs it rests on.
//
// WHY THIS EXISTS
//
// Line counts are not evidence. The checker used to say a file was safe because
// every deleted line had an added line beside it — and `get_all_adjustments_from_json`
// is the counter-example sitting in this repository: we gave it a fifth parameter
// and rewrote five call sites one line for one, in four of their files. Nothing
// was "removed". The behaviour of every export path now depends on a signature
// upstream owns and can change without a conflict.
//
// So the counts are informational and this file is the gate. Each entry names an
// Argentum feature, a borrowed fix or a deliberate behaviour change, what of
// upstream's it depends on, and what proves it still works. An upstream commit
// touching a registered dependency needs a recorded decision in
// scripts/upstream-decisions.mjs before the merge can be called reviewed.
//
// WHAT THIS CANNOT DO
//
// It cannot tell you that upstream has independently built the same feature in
// files we have never touched. `keywords` is a hint over commit subjects, not
// detection, and a subject that says "improve colour handling" will match
// nothing. That is why every review entry must also carry a featureReview block
// stating, in a person's words, that the incoming batch was read for duplicates.
// Nothing verifies that sentence. It is there so the claim is made explicitly
// and by someone, rather than assumed by a regular expression.
//
// RETIRING AN ENTRY
//
// Entries are never deleted. Set `retired: { recordedIn, why }` where recordedIn
// is the `through` sha of the review that decided it. An entry retired in the
// review being written now still generates its review requirement for that
// window — retiring it is the decision under review, not a way to avoid one.
// Only a retirement recorded in an earlier review stops the requirement.
//
// `how` values:
//   shadows   their code is left in place and not called; ours runs instead
//   replaces  their code was removed and ours does the job
//   calls     their file calls into ours at an anchor
//   extends   we changed the shape of something of theirs (a signature, a struct)
//   retypes   we changed the meaning or type of a value they still read
//   borrows   we carry their own unmerged fix, marked with // upstream #NNNN
//   rebrands  identity only: a name, a URL, a file extension
//
// A dependency names one `file`, or a `pattern` when it is a family of them
// (thirteen locale files carry the same rebrand and the same feature strings;
// listing each would be noise pretending to be precision).

export const HOW = ['shadows', 'replaces', 'calls', 'extends', 'retypes', 'borrows', 'rebrands'];
export const KINDS = ['feature', 'borrowed-fix', 'behaviour-change'];

export const REGISTRY = [
  {
    id: 'auto-white-balance',
    kind: 'feature',
    what: 'Auto white balance, and an eyedropper that agrees with it.',
    ours: [
      'src/argentum/AutoWhiteBalanceButton.tsx',
      'src/argentum/whiteBalance.ts',
      'src-tauri/src/mods/auto_wb.rs',
      'src-tauri/src/shaders/modules.wgsl',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_white_balance', how: 'shadows',
        note: 'Left defined and uncalled. Upstream fixes to it merge cleanly and never run.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_scene_linear', how: 'calls',
        note: 'The single scene-linear anchor.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', how: 'replaces',
        note: 'Their eyedropper solved a temperature inline and wrote straight to setAdjustments, so a dormant copy would have overwritten ours.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'data-argentum="color-tools" mount point.' },
    ],
    tests: ['src-tauri/src/mods/auto_wb.rs #[cfg(test)]'],
    keywords: /white.?balance|temperature|tint|auto.?wb|grey.?world|gray.?world|eyedropper|wb.?picker/i,
  },
  {
    id: 'camera-profile',
    kind: 'feature',
    what: 'DCP camera profiles, and the matrix correction derived from them.',
    ours: [
      'src/argentum/CameraProfile.tsx',
      'src-tauri/src/mods/dcp.rs',
      'src-tauri/src/mods/profile_correction.rs',
      'src-tauri/src/mods/profile_matrix.rs',
      'src-tauri/src/mods/profiles.rs',
      'src-tauri/src/mods/profiles_online.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'Three profile rows added to their struct, filled by mods::profile_correction::rows_for.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'The same three rows on the GPU side. Their struct and ours must stay in step or every pixel is wrong.' },
      { file: 'src/utils/adjustments.ts', key: 'cameraProfile', how: 'extends',
        note: 'A key we added to their adjustments type and their defaults.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'data-argentum="camera-profile" mount point.' },
    ],
    tests: [
      'src-tauri/src/mods/dcp.rs #[cfg(test)]',
      'src-tauri/src/mods/profile_correction.rs #[cfg(test)]',
      'src-tauri/src/mods/profile_matrix.rs #[cfg(test)]',
    ],
    keywords: /dcp|camera.?profile|colou?r.?matrix|calibration|icc.?profile|forward.?matrix/i,
  },
  {
    id: 'clipping-view',
    kind: 'behaviour-change',
    what: 'The clipping indicator cycles off / all / R / G / B instead of on and off.',
    ours: ['src-tauri/src/mods/clipping.rs', 'src-tauri/src/shaders/modules.wgsl'],
    dependsOn: [
      { file: 'src/components/panel/editor/Waveform.tsx', key: 'showClipping', how: 'retypes',
        note: 'Their interface still declares it boolean-or-number. The day upstream writes `=== true`, our four-way control reads as off and nothing errors.' },
      { file: 'src/components/panel/right/ControlsPanel.tsx', key: 'showClipping', how: 'retypes' },
      { file: 'src/components/panel/right/MasksPanel.tsx', key: 'showClipping', how: 'retypes' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'show_clipping', how: 'retypes',
        note: 'Their u32 field, filled by mods::clipping::mode rather than by a boolean.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_display', how: 'replaces',
        note: 'Their inline clipping block assigned to final_rgb, so leaving it dormant would have overwritten our stage.' },
    ],
    tests: ['src-tauri/src/mods/clipping.rs #[cfg(test)]'],
    keywords: /clipping|blown|clipped|highlight.?warning|show_clipping|waveform/i,
  },
  {
    id: 'preview-encode',
    kind: 'behaviour-change',
    what: 'The CPU preview gets a toe instead of a cliff.',
    ours: ['src-tauri/src/mods/preview_encode.rs'],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'apply_cpu_default_raw_processing', how: 'shadows',
        note: 'Their function is intact and returned over. Upstream has edited it since.' },
    ],
    tests: ['src-tauri/src/mods/preview_encode.rs #[cfg(test)]'],
    keywords: /preview|thumbnail|cpu.?raw|tone.?curve|gamma|encode/i,
  },
  {
    id: 'raw-decode',
    kind: 'feature',
    what: 'One anchor after the raw decode, where sRAW levels and our own steps run.',
    ours: ['src-tauri/src/mods/decode.rs', 'src-tauri/src/mods/sraw_levels.rs'],
    dependsOn: [
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'on_raw_decoded', how: 'calls',
        note: 'The single decode anchor.' },
    ],
    tests: [
      'src-tauri/src/mods/decode.rs #[cfg(test)]',
      'src-tauri/src/mods/sraw_levels.rs #[cfg(test)]',
    ],
    keywords: /raw|decode|demosaic|rawler|black.?level|white.?level|sraw/i,
  },
  {
    id: 'display-transform',
    kind: 'feature',
    what: 'sRGB converted to whichever screen the window is actually on.',
    ours: [
      'src-tauri/src/mods/display_monitor.rs',
      'src-tauri/src/mods/display_profile.rs',
      'src-tauri/src/shaders/ag_display.wgsl',
    ],
    dependsOn: [
      { file: 'src-tauri/src/lib.rs', symbol: 'ag_display_matrix', how: 'calls',
        note: 'Where the transform reaches the GPU, plus a refresh when the window moves screen.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'ag_display_matrix', how: 'extends',
        note: 'A field on their uniform, and our shader concatenated in front of theirs.' },
      { file: 'src-tauri/src/shaders/display.wgsl', symbol: 'ag_stage_present', how: 'calls',
        note: 'The presentation anchor.' },
    ],
    tests: [
      'src-tauri/src/mods/display_monitor.rs #[cfg(test)]',
      'src-tauri/src/mods/display_profile.rs #[cfg(test)]',
    ],
    keywords: /display|monitor|screen|gamut|srgb|colou?r.?management|present|swapchain/i,
  },
  {
    id: 'my-gear',
    kind: 'feature',
    what: 'The lens and camera library, moved out of their settings panel into ours.',
    ours: [
      'src/argentum/MyGear.tsx',
      'src/argentum/MyCameras.tsx',
      'src-tauri/src/mods/lens_crop.rs',
      'src-tauri/src/mods/makernote_lens.rs',
    ],
    dependsOn: [
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'replaces',
        note: '207 lines of their lens UI removed rather than hidden. This is the uncomfortable one: upstream edits this file often and any change inside the block we deleted is a conflict resolved by hand.' },
      { file: 'src-tauri/src/lens_correction.rs', symbol: 'match_for_camera', how: 'calls' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'fill_lens_model', how: 'calls' },
      { file: 'src/components/panel/right/MetadataPanel.tsx', how: 'calls',
        note: 'data-argentum="camera-details" mount point.' },
    ],
    tests: [
      'src-tauri/src/mods/makernote_lens.rs #[cfg(test)]',
      'NONE for mods/lens_crop.rs — the crop-factor match is unproven by anything but use',
    ],
    keywords: /lens|lensfun|mount|crop.?factor|makernote|vignett|distortion|camera.?model/i,
  },
  {
    id: 'rgb-readout',
    kind: 'feature',
    what: 'A live RGB readout under the cursor, and colour comparison.',
    ours: [
      'src/argentum/RgbReadout.tsx',
      'src/argentum/RgbReadoutButton.tsx',
      'src/argentum/rgbReadoutStore.ts',
      'src-tauri/src/mods/colour_compare.rs',
    ],
    dependsOn: [
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'Shares the data-argentum="color-tools" mount with auto white balance.' },
    ],
    tests: ['src-tauri/src/mods/colour_compare.rs #[cfg(test)]'],
    keywords: /readout|pixel.?value|sample|colou?r.?pick|histogram/i,
  },
  {
    id: 'highlight-recovery',
    kind: 'feature',
    what: 'Highlight recovery, and the sigmoid that lands it.',
    ours: [
      'src/argentum/HighlightRecovery.tsx',
      'src-tauri/src/mods/highlights.rs',
      'src-tauri/src/mods/sigmoid.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_scene_linear', how: 'calls',
        note: 'Runs inside the same scene-linear anchor as white balance, in a fixed order.' },
    ],
    tests: [
      'src-tauri/src/mods/highlights.rs #[cfg(test)]',
      'src-tauri/src/mods/sigmoid.rs #[cfg(test)]',
    ],
    keywords: /highlight|recover|clip.*reconstruct|blown|rolloff|roll.?off/i,
  },
  {
    id: 'adjustments-path-argument',
    kind: 'behaviour-change',
    what: 'get_all_adjustments_from_json takes the photo it is adjusting.',
    ours: ['src-tauri/src/mods/profile_correction.rs'],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'extends',
        note: 'A fifth parameter on a function of theirs. Rendering has to know which photo it holds, because the profile correction comes from the matrix that photo was decoded with.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'extends',
        note: 'Five call sites, each rewritten one line for one. Zero lines removed by any count — and the whole export path now depends on a signature upstream owns.' },
      { file: 'src-tauri/src/lut_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'extends' },
      { file: 'src-tauri/src/image_loader.rs', how: 'extends',
        note: 'Threads the path through to the call.' },
    ],
    tests: ['src-tauri/src/mods/profile_correction.rs #[cfg(test)]'],
    keywords: /get_all_adjustments|adjustment.*signature|tonemapper|hydrate_adjustments/i,
  },
  {
    id: 'cache-keys',
    kind: 'behaviour-change',
    what: 'Our own cache key and a cache version that invalidates on our changes, not theirs.',
    ours: ['src-tauri/src/mods/cache_key.rs', 'src-tauri/src/mods/cache_version.rs'],
    dependsOn: [
      { file: 'src-tauri/src/cache_utils.rs', how: 'extends',
        note: 'Their hashing is where a divergence shows up as the wrong picture rather than a crash.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'cache_version', how: 'calls' },
    ],
    tests: [
      'src-tauri/src/mods/cache_key.rs #[cfg(test)]',
      'src-tauri/src/mods/cache_version.rs #[cfg(test)]',
    ],
    keywords: /cache|hash|invalidat|thumbnail.*key|calculate_\w*hash/i,
  },
  {
    id: 'sidecar-agdata',
    kind: 'behaviour-change',
    what: 'Sidecars are .agdata and .agexif, so RapidRAW cannot open an Argentum edit.',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/file_management.rs', how: 'rebrands',
        note: 'Thirty-one lines, each replaced on the line below. Their function names are untouched: read_rrexif_sidecar still exists and still reads, it just reads a different extension.' },
      { file: 'src-tauri/src/exif_processing.rs', how: 'rebrands' },
      { file: 'src-tauri/src/tagging.rs', how: 'rebrands' },
    ],
    tests: [
      'NONE — and the deliberate consequence is untested: an existing .rrdata file '
      + 'is not read, so edits made in RapidRAW do not carry over. That was the intent, '
      + 'not an oversight, but nothing proves it stays that way.',
    ],
    keywords: /sidecar|rrdata|agdata|\.xmp|metadata.?file/i,
  },
  {
    id: 'argentum-shell',
    kind: 'feature',
    what: 'The Argentum panels: about, roadmap, known issues, credits, our own locales.',
    ours: [
      'src/argentum/Argentum.tsx',
      'src/argentum/AboutPanel.tsx',
      'src/argentum/roadmap.ts',
      'src/argentum/knownIssues.ts',
      'src/argentum/releases.ts',
      'src/argentum/credits.ts',
      'src/argentum/locales/en.json',
      'src/argentum/RenderStatus.tsx',
      'src/argentum/useAutoDetectOnLoad.ts',
      'src/argentum/useThresholdPreview.ts',
      'src/argentum/useTruncatedTooltips.ts',
    ],
    dependsOn: [
      { file: 'src/App.tsx', how: 'calls', note: 'The single <Argentum /> mount.' },
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'calls',
        note: 'data-argentum slot for the about and gear categories.' },
      { file: 'src/components/panel/right/CropPanel.tsx', symbol: 'useAutoDetectOnLoad', how: 'calls' },
      { pattern: /^src\/i18n\/locales\/[\w-]+\.json$/, how: 'extends',
        note: 'Their locale files carry the rebrand and a few Argentum strings, because '
          + 'i18next only looks in its own files. When this outgrows its budget the answer '
          + 'is an Argentum namespace under src/argentum/locales, not a bigger budget.' },
    ],
    tests: ['NONE — frontend has no test runner in this repository'],
    keywords: /settings.?panel|about|credits|locale|i18n|translation/i,
  },
  {
    id: 'identity',
    kind: 'behaviour-change',
    what: 'The fork is Argentum: name, bundle id, update URL, no donation link.',
    ours: ['src-tauri/.identity', 'scripts/check-identity.mjs'],
    dependsOn: [
      { file: 'src/components/panel/MainLibrary.tsx', how: 'replaces',
        note: 'Their update check points at our releases, and the Ko-fi link is gone: Argentum must not raise money on its splash in another author’s name. The credit is in Special Thanks and CREDITS.md.' },
      { file: 'package.json', how: 'rebrands' },
      { file: 'src-tauri/Cargo.toml', how: 'rebrands' },
      { file: 'src-tauri/tauri.conf.json', how: 'rebrands' },
      { file: 'src-tauri/src/main.rs', how: 'rebrands' },
      { file: 'index.html', how: 'rebrands' },
      { file: 'src/window/TitleBar.tsx', how: 'rebrands' },
    ],
    tests: ['scripts/check-identity.mjs, run by npm start'],
    keywords: /rapidraw|branding|bundle.?identifier|update.?check|ko-?fi|donat/i,
  },
  {
    id: 'ci-desktop-only',
    kind: 'behaviour-change',
    what: 'No Android build, and the full matrix runs on release rather than every push.',
    ours: ['.github/workflows/upstream.yml'],
    dependsOn: [
      { file: '.github/workflows/ci.yml', how: 'replaces' },
      { file: '.github/workflows/pr-ci.yml', how: 'replaces' },
      { file: '.github/workflows/release.yml', how: 'replaces' },
      { file: '.github/workflows/lint.yml', how: 'extends',
        note: 'rustfmt runs over mods/ only: reformatting their files would cost lines against the anchors.' },
    ],
    tests: ['the workflows themselves'],
    keywords: /workflow|android|aarch64|matrix|github.?action|release.?build/i,
  },
  {
    id: 'borrow-1307',
    kind: 'borrowed-fix',
    what: 'Two AI patches of the same base64 length no longer share a cache entry.',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/cache_utils.rs', pr: '1307', how: 'borrows',
        note: 'Marked between // upstream #1307 and // end upstream #1307 inside calculate_transform_hash.' },
    ],
    tests: ['src-tauri/src/mods/cache_key.rs #[cfg(test)] covers the same collision on our side'],
    keywords: /cache|hash|ai.?patch|patch.?data|collision/i,
  },
  {
    id: 'borrow-1633',
    kind: 'borrowed-fix',
    what: 'sRGB decoding uses an exponent of 2.4, not 3.0.',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/raw_processing.rs', pr: '1633', how: 'borrows',
        note: 'Marked between // upstream #1633 and // end upstream #1633.' },
    ],
    tests: ['NONE — the constant is not covered by a test on either side'],
    keywords: /srgb|gamma|2\.4|transfer.?function|linear(ise|ize)/i,
  },
];

/**
 * Entries whose review requirement is live for the window closing at `through`.
 *
 * The first version compared `recordedIn` against the *previous* review's sha,
 * which made a retirement last exactly one window: an entry retired in R2 was
 * correctly dropped from R3's window and then came back, active again, for R4
 * and every review after it. Retirement has to be ordered against the review
 * history, not matched against one sha.
 *
 * An entry is live for a window iff that window is at or before the review that
 * retired it. At the retiring review it is still live, because retiring it is
 * the decision under review; after it, never again.
 *
 * `through` of null means the window being prepared but not yet recorded — the
 * one `npm run review:upstream` is printing. Every recorded retirement is behind
 * it, so retired entries are out.
 *
 * An unrecognised `recordedIn` keeps the entry live. It is not this function's
 * job to decide whether a retirement is real; validateRetirements says so, and
 * the safe reading in the meantime is that nothing has been retired.
 */
export function activeFor(entries, reviews, through) {
  const order = new Map(reviews.map((r, i) => [r.through, i]));
  const window = through === null || through === undefined
    ? reviews.length
    : order.get(through);
  return entries.filter((e) => {
    if (!e.retired) return true;
    const retiredAt = order.get(e.retired.recordedIn);
    if (retiredAt === undefined) return true;
    if (window === undefined) return true;
    return window <= retiredAt;
  });
}

/**
 * Retirements that do not hold up: an unknown review, no reasoning, or no
 * decision in that review recording why.
 *
 * A retirement is how an entry stops generating review requirements, so it is
 * the obvious thing to fake. It has to name a review that exists and carry a
 * `retire:<id>` decision in that same review, with a verdict and a reason, like
 * any other decision.
 */
export function validateRetirements(entries, reviews) {
  const byThrough = new Map(reviews.map((r) => [r.through, r]));
  const problems = [];
  for (const entry of entries) {
    if (!entry.retired) continue;
    const { recordedIn, why } = entry.retired;
    const review = byThrough.get(recordedIn);
    if (!review) {
      problems.push({
        id: entry.id,
        detail: `retired: { recordedIn: '${String(recordedIn).slice(0, 12)}' } names no review in REVIEWS`,
        fix: 'recordedIn is the `through` sha of the review that decided the retirement.',
      });
      continue;
    }
    if (!why || why.trim().length < 20) {
      problems.push({
        id: entry.id,
        detail: 'is retired with no reasoning',
        fix: 'Say what happened to it: upstream merged the fix, the feature was dropped, it moved.',
      });
    }
    const decision = (review.decisions ?? []).find((d) => d.overlap === `retire:${entry.id}`);
    if (!decision) {
      problems.push({
        id: entry.id,
        detail: `is retired in ${String(recordedIn).slice(0, 8)}, and that review records no retire:${entry.id} decision`,
        fix: `Add { overlap: 'retire:${entry.id}', verdict, why } to that review. `
          + 'Retiring an entry is a decision and is recorded like one.',
      });
    }
  }
  return problems;
}

/**
 * Every entry id this file has ever held, read out of our own git history.
 *
 * Retiring an entry keeps its requirement visible. Deleting the entry and its
 * markers in one commit did not: with nothing in the tree and nothing in the
 * registry, there was nothing left to disagree about and the requirement simply
 * stopped existing. So the inventory is append-only and git is what says so —
 * no second register to drift, and no way to edit the record of what we used to
 * depend on without rewriting history.
 *
 * The walk is over one small file's revisions. If that ever gets slow, the fix
 * is not to look at fewer of them.
 */
export function historicalIds(run, path = 'scripts/upstream-registry.mjs') {
  let revisions = [];
  try {
    revisions = run(`git log --format=%H -- ${path}`).trim().split('\n').filter(Boolean);
  } catch {
    return null;
  }
  const ids = new Set();
  for (const sha of revisions) {
    let text = '';
    try {
      text = run(`git show ${sha}:${path}`, true);
    } catch {
      continue;
    }
    for (const m of text.matchAll(/^\s{4}id: '([A-Za-z0-9-]+)',$/gm)) ids.add(m[1]);
  }
  return ids;
}

/** file -> entries, plus the symbols and keys those entries name. */
export function dependencyIndex(entries) {
  const byFile = new Map();
  const patterns = [];
  const prs = [];
  for (const entry of entries) {
    for (const dep of entry.dependsOn) {
      if (dep.pattern) patterns.push({ entry, dep });
      else {
        if (!byFile.has(dep.file)) byFile.set(dep.file, []);
        byFile.get(dep.file).push({ entry, dep });
      }
      if (dep.pr) prs.push({ entry, dep });
    }
  }
  /** Is this file of theirs claimed by anything? */
  const claims = (file) => byFile.has(file)
    || patterns.some(({ dep }) => dep.pattern.test(file));
  return { byFile, patterns, prs, claims };
}

/** Upstream code we leave in place and step around. */
export const shadowsOf = (entries) => entries.flatMap((entry) => entry.dependsOn
  .filter((dep) => dep.how === 'shadows' && dep.symbol)
  .map((dep) => ({ entry, file: dep.file, symbol: dep.symbol, note: dep.note })));

/** Borrowed fixes, as {entry, file, pr}. */
export const borrowsOf = (entries) => entries.flatMap((entry) => entry.dependsOn
  .filter((dep) => dep.how === 'borrows' && dep.pr)
  .map((dep) => ({ entry, file: dep.file, pr: dep.pr })));
