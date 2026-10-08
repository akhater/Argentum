// What we decided about each upstream change that landed on top of ours.
//
// WHY THIS EXISTS, AND WHY IT IS THE ONLY REGISTER
//
// Merging used to erase the review window. The review ran from the git
// merge-base, so the moment the merge landed the base moved and the script said
// "nothing new" — whether or not anybody had looked. The CHANGELOG line meant to
// force the review could be satisfied with one `sed`.
//
// So the reviewed-through point lives here, with the decisions attached to it,
// and the CHANGELOG line is checked against this file rather than written
// independently. There is one register, not two, which is the whole objection to
// keeping a register at all: two of them drift, and the stale CHANGELOG line was
// the proof.
//
// HOW IT IS ENFORCED
//
// check-mergeability.mjs re-derives the overlaps in the range the newest entry
// covers — `REVIEWS[n-1].through .. REVIEWS[n].through` — from git and from
// scripts/upstream-registry.mjs, and fails until every gated one has a decision
// here. You cannot advance `through` without the script asking again for each
// overlap it finds, and you cannot make an overlap go away by deleting the thing
// that caused it: a registry entry retired in the review being written still
// generates its requirement for that window.
//
// Only the newest entry's range is re-derived. Older ones are records of what was
// known then; re-deriving them against today's registry would invent overlaps
// nobody could have seen at the time.
//
// VERDICTS
//
//   adopt           take theirs, drop ours
//   keep-ours       ours stands; theirs is knowingly not used here
//   combine         both, reconciled by hand — say how
//   not-applicable  the overlap is mechanical noise; say why it cannot bite
//
// THE FEATURE REVIEW
//
// Every entry that covers a real range must carry `featureReview`. Overlap
// detection is file-based: it finds upstream changing something we registered a
// dependency on. It cannot find upstream building the same feature somewhere we
// have never touched — no amount of pattern matching does that, and claiming
// otherwise would be the more dangerous kind of wrong. So the batch is read for
// duplicate features by a person, and that claim is written down as a sentence
// with their reasoning in it. Nothing verifies the sentence. It is required so
// that the claim is made rather than assumed.
//
// Run `npm run review:upstream` — it prints the overlap keys ready to paste.

export const VERDICTS = ['adopt', 'keep-ours', 'combine', 'not-applicable'];

// The v1.6.4 review has many repeated checks against the same shared shader
// anchors. Keep each exact overlap as its own decision, while sharing the
// explanation for a commit/anchor pair so the register stays readable.
const review164Decision = (commit, overlap, verdict, why, kind = 'dep') => ({
  overlap: `${commit}:${kind}:${overlap}`,
  verdict,
  why: `${why} Exact overlap: ${overlap}.`,
});

const review164Group = (commit, overlaps, verdict, why, kind = 'dep') =>
  overlaps.map((overlap) => review164Decision(commit, overlap, verdict, why, kind));

const review164ShaderOverlaps = [
  'auto-white-balance:src-tauri/src/shaders/shader.wgsl#apply_white_balance',
  'auto-white-balance:src-tauri/src/shaders/shader.wgsl#ag_stage_scene_linear',
  'camera-profile:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments',
  'clipping-view:src-tauri/src/shaders/shader.wgsl#ag_stage_display',
  'highlight-recovery:src-tauri/src/shaders/shader.wgsl#ag_stage_scene_linear',
  'high-precision-export:src-tauri/src/shaders/shader.wgsl',
  'high-precision-export:src-tauri/src/shaders/shader.wgsl#output_texture',
];

const review164ShaderAnchorWhy = {
  'auto-white-balance:src-tauri/src/shaders/shader.wgsl#apply_white_balance':
    'Keep Argentum’s white-balance implementation and combine the upstream shader change around it; compile and test the shared shader.',
  'auto-white-balance:src-tauri/src/shaders/shader.wgsl#ag_stage_scene_linear':
    'Keep Argentum’s scene-linear processing order and combine the upstream adjustment at its own stage; test that order.',
  'camera-profile:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments':
    'Keep Argentum’s camera-profile fields and matching GPU layout while integrating the upstream adjustment; compile the shader and Rust code together.',
  'clipping-view:src-tauri/src/shaders/shader.wgsl#ag_stage_display':
    'Keep Argentum’s clipping display stage alongside the upstream adjustment; verify the display overlay still works.',
  'highlight-recovery:src-tauri/src/shaders/shader.wgsl#ag_stage_scene_linear':
    'Keep Argentum’s RAW recovery at its existing decode stage; this editor-shader overlap does not authorize replacing that recovery.',
  'high-precision-export:src-tauri/src/shaders/shader.wgsl':
    'Keep Argentum’s shared shader assembly and 32-bit-float export output while integrating the upstream pixel adjustment; test preview and TIFF output.',
  'high-precision-export:src-tauri/src/shaders/shader.wgsl#output_texture':
    'Keep the rgba8 preview declaration that Argentum’s checked export rewrite converts to rgba32float; do not replace it with RapidRAW’s separate half-float target.',
};

const review164ShaderPolicies = [
  {
    commit: '429bd0dc',
    verdict: 'combine',
    why: 'Adopt RapidRAW’s RGB-curve calculation while preserving Argentum’s custom shader stages.',
  },
  {
    commit: '21773eb2',
    verdict: 'combine',
    why: 'Adopt RapidRAW’s revised Vibrance calculation while preserving Argentum’s camera-profile and processing order.',
  },
  {
    commit: 'afa9104e',
    verdict: 'combine',
    why: 'Adopt the local-contrast-aware editor Highlights slider, separately from RAW highlight recovery.',
  },
  {
    commit: '86884cc9',
    verdict: 'combine',
    why: 'Adopt this final v1.6.4 Brightness formula and its after-tone-mapping placement; do not use the earlier formula as the final behavior.',
  },
  {
    commit: '3869c546',
    verdict: 'keep-ours',
    why: 'Do not install this intermediate Brightness formula by itself; it is superseded by the final 86884cc9 implementation.',
  },
  {
    commit: 'bca29312',
    verdict: 'keep-ours',
    why: 'Do not install this intermediate Brightness formula by itself; use only the final 86884cc9 implementation and the final Android workflow.',
  },
  {
    commit: '85bf424a',
    verdict: 'combine',
    why: 'Adopt the independent editor Highlights-slider shader change, but keep Argentum’s RAW recovery algorithm active and unchanged.',
  },
  {
    commit: '0e8cd159',
    verdict: 'keep-ours',
    why: 'Keep Argentum’s existing shared-shader and 32-bit-float TIFF-output design rather than replacing it with RapidRAW’s half-float export pipeline.',
  },
  {
    commit: 'ad179a45',
    verdict: 'keep-ours',
    why: 'Keep Argentum’s existing shared-shader and 32-bit-float TIFF-output design rather than replacing it with RapidRAW’s half-float export pipeline.',
  },
];

const review164ShaderDecisions = review164ShaderPolicies.flatMap((policy) =>
  review164ShaderOverlaps.map((overlap) => review164Decision(
    policy.commit,
    overlap,
    policy.verdict,
    `${policy.why} ${review164ShaderAnchorWhy[overlap]}`,
  )),
);

const review164RawOverlaps = [
  'raw-decode:src-tauri/src/raw_processing.rs#on_raw_decoded',
  'canon-old-wb:src-tauri/src/raw_processing.rs#on_raw_decoded',
  'borrow-1633:src-tauri/src/raw_processing.rs',
];

const review164GpuOverlaps = [
  'display-transform:src-tauri/src/gpu_processing.rs#ag_display_matrix',
  'high-precision-export:src-tauri/src/gpu_processing.rs#GpuProcessor::new',
  'high-precision-export:src-tauri/src/gpu_processing.rs#read_texture_data_roi',
  'high-precision-export:src-tauri/src/gpu_processing.rs#to_rgba_f16',
  'high-precision-export:src-tauri/src/gpu_processing.rs#GpuProcessor::run',
  'high-precision-export:src-tauri/src/gpu_processing.rs#process_and_get_dynamic_image_inner',
];

const review164ExportOverlaps = [
  'adjustments-path-argument:src-tauri/src/export_processing.rs#get_all_adjustments_from_json',
  'high-precision-export:src-tauri/src/export_processing.rs#process_image_for_export',
  'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image',
  'high-precision-export:src-tauri/src/export_processing.rs#apply_watermark',
  'high-precision-export:src-tauri/src/export_processing.rs#encode_image_to_bytes',
  'export-precision-selector:src-tauri/src/export_processing.rs',
  'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes',
  'tiff-export-metadata:src-tauri/src/export_processing.rs#save_image_with_metadata',
];

const review164ImageProcessingOverlaps = [
  'camera-profile:src-tauri/src/image_processing.rs#GlobalAdjustments',
  'clipping-view:src-tauri/src/image_processing.rs#show_clipping',
  'preview-encode:src-tauri/src/image_processing.rs#apply_cpu_default_raw_processing',
  'adjustments-path-argument:src-tauri/src/image_processing.rs#get_all_adjustments_from_json',
];

const review164LibraryOverlaps = [
  'display-transform:src-tauri/src/lib.rs#ag_display_matrix',
  'cache-keys:src-tauri/src/lib.rs#cache_version',
];

const review164OtherDecisions = [
  ...review164Group(
    '85bf424a',
    review164RawOverlaps,
    'combine',
    'Adopt the independent LinearRaw above-white conversion and remove the old post-demosaic compression calculation, while excluding RapidRAW’s post-demosaic recovery. Keep Argentum’s pre-demosaic recovery and borrowed sRGB correction.',
  ),
  ...review164Group(
    'f00145c1',
    review164RawOverlaps,
    'combine',
    'Adopt v1.6.4’s fixed 1000.0 full-quality ceiling and hide the obsolete compression control; retain old saved values for compatibility, but do not let them change the new policy.',
  ),
  ...review164Group(
    'f00145c1',
    [
      'my-gear:src/components/panel/SettingsPanel.tsx',
      'argentum-shell:src/components/panel/SettingsPanel.tsx',
    ],
    'combine',
    'Hide the obsolete RAW compression control without removing Argentum’s Settings shell or gear sections; leave saved values readable for compatibility.',
  ),
  ...review164Group(
    '40a112e1',
    [
      'my-gear:src/components/panel/SettingsPanel.tsx',
      'argentum-shell:src/components/panel/SettingsPanel.tsx',
    ],
    'combine',
    'Add the neutral-grey canvas setting inside Argentum’s existing Settings and gear layout; preserve its custom shell.',
  ),
  review164Decision(
    '1e2b3924',
    'identity:src-tauri/tauri.conf.json',
    'keep-ours',
    'Keep Argentum’s application name, branding, versioning and splash screen; do not import RapidRAW release identity.',
  ),
  ...review164Group(
    'e196e795',
    [
      'display-transform:src-tauri/src/gpu_processing.rs#ag_display_matrix',
      ...review164GpuOverlaps.filter((x) => x.startsWith('high-precision-export:')),
      ...review164ExportOverlaps,
    ],
    'keep-ours',
    'This upstream commit removes its TIFF-PR tests rather than changing production behavior. Keep Argentum’s implementation and its existing precision, metadata, settings, and regression tests.',
  ),
  ...review164Group(
    'e9d6cc74',
    review164ImageProcessingOverlaps.slice(0, 4).concat(review164LibraryOverlaps),
    'not-applicable',
    'This TIFF-PR merge does not replace this separate Argentum RAW-processing, display, cache, or photo-path integration; keep the Ag behavior and review the TIFF changes at their export seams.',
  ),
  ...review164Group(
    '0e8cd159',
    review164ImageProcessingOverlaps.concat(review164LibraryOverlaps),
    'not-applicable',
    'The final TIFF sync does not change this separate Argentum RAW-preview, display, cache, or photo-path behavior; preserve it and reconcile TIFF handling in the export/GPU decisions below.',
  ),
  ...review164Group(
    '0e8cd159',
    review164GpuOverlaps,
    'combine',
    'Keep Argentum’s 32-bit-float output and custom display transform, and remove only the redundant readback flag in favor of the existing output_to_display decision; test preview and TIFF readback.',
  ),
  ...review164Group(
    '0e8cd159',
    review164ExportOverlaps,
    'keep-ours',
    'Keep Argentum’s tested TIFF renderer, 8/16-bit global preference, size estimate, photo-path argument, and metadata-preserving encoder. Add the upstream headless bit-depth option separately; do not add a preset field.',
  ),
  ...review164Group(
    '0e8cd159',
    [
      'export-precision-selector:src/components/panel/right/ExportPanel.tsx',
      'tiff-export-metadata:src/components/panel/right/ExportPanel.tsx',
    ],
    'keep-ours',
    'Keep Argentum’s global 8/16-bit preference and its existing TIFF export UI; explicitly do not store bit depth in export presets.',
  ),
  ...review164Group(
    '8f60ec4b',
    review164GpuOverlaps,
    'combine',
    'Remove the duplicate skip_readback/skip_cpu_readback flag because it repeats output_to_display; preserve Argentum’s high-precision return type and display matrix.',
  ),
  ...review164Group(
    '88415dcc',
    [
      'clipping-view:src/components/panel/editor/Waveform.tsx:showClipping',
      'clipping-view:src/components/panel/right/ControlsPanel.tsx:showClipping',
      'clipping-view:src/components/panel/right/MasksPanel.tsx:showClipping',
    ],
    'combine',
    'Preserve Argentum’s clipping controls and overlays while taking the separately reviewed upstream folder/UI fixes; verify clipping remains visible in each affected panel.',
  ),
  ...review164Group(
    '88415dcc',
    review164LibraryOverlaps,
    'keep-ours',
    'Keep Argentum’s display transform and cache-version logic; the TIFF-PR branch does not replace those fork-specific behaviors.',
  ),
  ...review164Group(
    '88415dcc',
    review164GpuOverlaps,
    'keep-ours',
    'Keep Argentum’s 32-bit-float TIFF render path and custom GPU integration; do not import RapidRAW’s separate half-float TIFF pipeline.',
  ),
  ...review164Group(
    'ad179a45',
    review164ImageProcessingOverlaps,
    'not-applicable',
    'The TIFF export feature does not replace this separate Argentum RAW-preview, clipping, or photo-path behavior; retain it unchanged.',
  ),
  ...review164Group(
    'ad179a45',
    review164LibraryOverlaps,
    'not-applicable',
    'The TIFF export feature does not change Argentum’s display transform or cache-version logic; keep both as-is.',
  ),
  ...review164Group(
    'ad179a45',
    review164GpuOverlaps,
    'keep-ours',
    'Keep Argentum’s existing 32-bit-float output pipeline and custom display transform instead of importing RapidRAW’s half-float target; retain and test the Ag precision path.',
  ),
  review164Decision(
    'ad179a45',
    'tiff-export-metadata:src-tauri/src/exif_processing.rs#write_image_with_metadata',
    'keep-ours',
    'Keep Argentum’s TIFF-specific metadata merge, which starts from the encoded TIFF and preserves its dimensions, strips, and bit depth; upstream’s generic EXIF writer is not safe for TIFF structure.',
  ),
  ...review164Group(
    'ad179a45',
    review164ExportOverlaps,
    'keep-ours',
    'Keep Argentum’s own tested 8/16-bit TIFF pipeline, global preference, photo-path-aware adjustments, and TIFF-safe metadata merge. Add only the missing headless flag; do not add per-preset depth.',
  ),
  ...review164Group(
    'ad179a45',
    [
      'export-precision-selector:src/components/panel/right/ExportPanel.tsx',
      'tiff-export-metadata:src/components/panel/right/ExportPanel.tsx',
    ],
    'keep-ours',
    'Keep Argentum’s existing global depth selector and TIFF metadata UI; do not move depth into presets.',
  ),
];

const review164FeatureDecisions = [
  ['7bbd1ffd', 'ci-desktop-only', 'combine', 'Adopt the final Android workflow fix by adding the needed platform tools while preserving Argentum’s existing build and release steps.'],
  ['d98be296', 'ci-desktop-only', 'keep-ours', 'Do not retain this failed intermediate Android workaround; the later 7bbd1ffd commit is the final workflow change.'],
  ['bca29312', 'ci-desktop-only', 'keep-ours', 'Do not retain this intermediate Android CI workaround; use the final 7bbd1ffd workflow change.'],
  ['f00145c1', 'highlight-recovery', 'combine', 'Keep Argentum’s active pre-demosaic recovery; separately adopt the fixed full-quality ceiling and removal of the obsolete compression calculation/control.'],
  ['40a112e1', 'mask-stage-size-guard', 'combine', 'Add the neutral-grey canvas setting while retaining and testing Argentum’s mask-canvas lifecycle and size guard.'],
  ['e9d6cc74', 'tif-raw-sniffing', 'keep-ours', 'Keep Argentum’s own guarded .tif RAW detection and tests; the TIFF export feature does not replace file sniffing.'],
  ['e9d6cc74', 'high-precision-export', 'keep-ours', 'Keep Argentum’s distinct 32-bit-float TIFF output path and its tests; do not import the half-float render implementation.'],
  ['e9d6cc74', 'export-precision-selector', 'keep-ours', 'Keep Argentum’s global 8/16-bit preference rather than storing bit depth in each export preset.'],
  ['e9d6cc74', 'tiff-export-metadata', 'keep-ours', 'Keep Argentum’s TIFF-safe metadata merge and structural-tag tests.'],
  ['0e8cd159', 'tif-raw-sniffing', 'keep-ours', 'Keep Argentum’s own guarded .tif RAW detection and tests; this sync concerns TIFF export, not file identification.'],
  ['88415dcc', 'tif-raw-sniffing', 'keep-ours', 'Keep Argentum’s own guarded .tif RAW detection and tests; the merge does not replace that implementation.'],
  ['88415dcc', 'export-precision-selector', 'keep-ours', 'Keep Argentum’s global 8/16-bit preference; do not adopt RapidRAW’s preset field.'],
  ['88415dcc', 'tiff-export-metadata', 'keep-ours', 'Keep Argentum’s TIFF-safe metadata merge and tests.'],
  ['ad179a45', 'tif-raw-sniffing', 'keep-ours', 'Keep Argentum’s own guarded .tif RAW detection and tests; TIFF export and RAW file identification are separate.'],
].map(([commit, overlap, verdict, why]) => review164Decision(commit, overlap, verdict, why, 'feature'));

const review164CatchupDecisions = [
  review164Decision('71a07921', 'rapidraw-164-catchup:data/io.github.CyberTimon.RapidRAW.metainfo.xml', 'keep-ours', 'Keep the manifest file installed by Argentum’s existing Flatpak recipe; upstream’s deletion would leave a broken install command. Its pre-existing RapidRAW label remains a separate packaging-identity cleanup.'),
  review164Decision('429bd0dc', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'adopt', 'Adopt RapidRAW’s independent RGB-curve shader correction.'),
  review164Decision('8c5aa1e4', 'rapidraw-164-catchup:src/components/panel/right/FolderTree.tsx', 'adopt', 'Adopt the folder-tree control-height correction.'),
  review164Decision('21773eb2', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'adopt', 'Adopt the revised Vibrance shader.'),
  review164Decision('afa9104e', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'adopt', 'Adopt the local-contrast-aware Highlights adjustment; it is separate from RAW pixel recovery.'),
  review164Decision('7bbd1ffd', 'rapidraw-164-catchup:.github/workflows/build.yml', 'combine', 'Keep the final Android platform-tools setup in Argentum’s existing workflow.'),
  review164Decision('86884cc9', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'adopt', 'Use the final post-tone-map Brightness implementation from this commit.'),
  review164Decision('86884cc9', 'rapidraw-164-catchup:src/components/adjustments/Basic.tsx', 'adopt', 'Adopt the final Basic-panel ordering and Brightness control placement.'),
  review164Decision('798a734d', 'rapidraw-164-catchup:src/components/adjustments/Basic.tsx', 'adopt', 'Use the corrected Exposure and Brightness labels.'),
  review164Decision('798a734d', 'rapidraw-164-catchup:src/i18n/update_translations.py', 'adopt', 'Adopt the translation extraction updates that supply those labels.'),
  review164Decision('d98be296', 'rapidraw-164-catchup:.github/workflows/build.yml', 'not-applicable', 'This is the failed intermediate Android workflow workaround; only the later final fix is retained.'),
  review164Decision('3869c546', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'not-applicable', 'This intermediate Brightness implementation is superseded by the final 86884cc9 implementation.'),
  review164Decision('bca29312', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'not-applicable', 'This intermediate Brightness implementation is superseded by the final 86884cc9 implementation.'),
  review164Decision('bca29312', 'rapidraw-164-catchup:.github/workflows/build.yml', 'not-applicable', 'This intermediate Android workflow change is superseded by the final 7bbd1ffd fix.'),
  review164Decision('85bf424a', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'combine', 'Adopt its local-contrast-aware Highlights shader, but exclude its separate post-demosaic RAW recovery algorithm.'),
  review164Decision('85bf424a', 'rapidraw-164-catchup:src-tauri/src/raw_processing.rs', 'combine', 'Keep Argentum’s pre-demosaic recovery; adopt the above-white conversion boundary and the final 1000.0 full-quality ceiling.'),
  review164Decision('e50cfb6b', 'rapidraw-164-catchup:src/components/panel/BottomBar.tsx', 'adopt', 'Persist Quick Filter visibility rather than keeping it as component-local state.'),
  review164Decision('e50cfb6b', 'rapidraw-164-catchup:src/components/ui/AppProperties.tsx', 'adopt', 'Add the Quick Filter visibility field to the shared UI state type.'),
  review164Decision('e50cfb6b', 'rapidraw-164-catchup:src/store/useUIStore.ts', 'adopt', 'Store Quick Filter visibility and initialize it when switching workspaces.'),
  review164Decision('f00145c1', 'rapidraw-164-catchup:src-tauri/src/raw_processing.rs', 'combine', 'Use the fixed 1000.0 full-quality ceiling and stop applying old compression, while preserving the legacy setting for migration and leaving fast decode at 1.0.'),
  review164Decision('40a112e1', 'rapidraw-164-catchup:src-tauri/src/app_settings.rs', 'adopt', 'Adopt the neutral-grey canvas preference and retain backward-compatible settings serialization.'),
  review164Decision('40a112e1', 'rapidraw-164-catchup:src/components/panel/Editor.tsx', 'adopt', 'Adopt the optional neutral-grey background in the editor only.'),
  review164Decision('40a112e1', 'rapidraw-164-catchup:src/components/ui/AppProperties.tsx', 'adopt', 'Add the neutral-grey preference to the shared settings type.'),
  review164Decision('40a112e1', 'rapidraw-164-catchup:src/i18n/update_translations.py', 'adopt', 'Adopt the translation extraction update for the neutral-grey setting.'),
  review164Decision('e196e795', 'rapidraw-164-catchup:src-tauri/src/gpu_processing.rs', 'combine', 'Remove the redundant readback flag while keeping Argentum’s 32-bit-float export target.'),
  review164Decision('e196e795', 'rapidraw-164-catchup:src-tauri/src/export_processing.rs', 'combine', 'Keep Argentum’s TIFF encoder path; independently set the requested headless depth in the headless process.'),
  review164Decision('e196e795', 'rapidraw-164-catchup:src-tauri/src/launch_request.rs', 'combine', 'Keep the CLI option as validated 8/16-bit input with a 16-bit default, using a plain u8 at the parser boundary.'),
  review164Decision('0e8cd159', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'combine', 'Keep Argentum’s 32-bit output target and precision dither while reconciling upstream’s final shader changes.'),
  review164Decision('0e8cd159', 'rapidraw-164-catchup:src-tauri/src/gpu_processing.rs', 'combine', 'Keep Argentum’s high-precision path and fold in the non-duplicated readback logic.'),
  review164Decision('0e8cd159', 'rapidraw-164-catchup:src-tauri/src/export_processing.rs', 'combine', 'Retain Argentum’s TIFF renderer and encoder; add only the independent headless depth override.'),
  review164Decision('8f60ec4b', 'rapidraw-164-catchup:src-tauri/src/gpu_processing.rs', 'adopt', 'Remove the redundant readback flag because output_to_display already expresses the same condition.'),
  review164Decision('88415dcc', 'rapidraw-164-catchup:src-tauri/src/gpu_processing.rs', 'keep-ours', 'Keep Argentum’s existing precision-aware output allocation and readback when reconciling this upstream merge.'),
  review164Decision('ad179a45', 'rapidraw-164-catchup:src-tauri/src/shaders/shader.wgsl', 'keep-ours', 'Do not replace Argentum’s 32-bit output-target implementation with RapidRAW’s half-float target.'),
  review164Decision('ad179a45', 'rapidraw-164-catchup:src-tauri/src/gpu_processing.rs', 'keep-ours', 'Retain Argentum’s separate precision-aware GPU render path.'),
  review164Decision('ad179a45', 'rapidraw-164-catchup:src-tauri/src/export_processing.rs', 'keep-ours', 'Retain Argentum’s TIFF encoder and metadata-safe export implementation.'),
  review164Decision('ad179a45', 'rapidraw-164-catchup:src-tauri/src/launch_request.rs', 'combine', 'Adopt a headless bit-depth option, but validate 8/16 and default to 16 without moving depth into export presets.'),
];

const review164PresetDecisions = [
  review164Decision('0e8cd159', 'export-precision-selector:src/components/ui/ExportImportProperties.tsx', 'keep-ours', 'Reject TIFF depth on export presets; retain Argentum’s global preference.'),
  review164Decision('0e8cd159', 'export-precision-selector:src/hooks/useExportSettings.ts', 'keep-ours', 'Reject per-export depth state; the global Argentum control remains the single setting.'),
  review164Decision('0e8cd159', 'export-precision-selector:src/hooks/useExternalEditSession.ts', 'keep-ours', 'External edits do not override the saved global TIFF depth.'),
  review164Decision('ad179a45', 'export-precision-selector:src/components/ui/ExportImportProperties.tsx', 'keep-ours', 'Reject RapidRAW’s preset field and keep TIFF depth as Argentum’s existing global preference.'),
  review164Decision('ad179a45', 'export-precision-selector:src/hooks/useExportSettings.ts', 'keep-ours', 'Do not duplicate the global TIFF-depth preference in per-export UI state.'),
  review164Decision('ad179a45', 'export-precision-selector:src/hooks/useExternalEditSession.ts', 'keep-ours', 'Do not force external edit exports to 16-bit; keep the global preference authoritative.'),
];

const review164CompositionDecisions = [
  review164Decision('e50cfb6b', 'import-dialogue-1714:src/components/ui/AppProperties.tsx#ImportSettings', 'combine', 'Keep both additive UI-state extensions: RapidRAW persists Quick Filter visibility, while import-dialogue-1714 adds the persisted import settings shape.'),
  review164Decision('40a112e1', 'import-dialogue-1714:src-tauri/src/app_settings.rs#last_import_settings', 'combine', 'Keep both persisted settings extensions: RapidRAW adds the neutral-grey canvas preference, while import-dialogue-1714 stores the last import choices.'),
  review164Decision('40a112e1', 'import-dialogue-1714:src/components/ui/AppProperties.tsx#ImportSettings', 'combine', 'Keep both shared UI-property extensions: the neutral-grey editor setting and the import dialog settings remain separate fields.'),
  review164Decision('e9d6cc74', 'import-dialogue-1714:src-tauri/src/image_processing.rs#calculate_auto_adjustments', 'combine', 'Keep the upstream TIFF/export compatibility changes and import-dialogue-1714’s reuse of calculate_auto_adjustments; the import feature calls the existing analysis rather than replacing its processing behavior.'),
  review164Decision('0e8cd159', 'import-dialogue-1714:src-tauri/src/image_processing.rs#calculate_auto_adjustments', 'combine', 'Keep the upstream processing changes and the import feature’s call into calculate_auto_adjustments; the two changes occupy separate responsibilities in the same function family.'),
  review164Decision('ad179a45', 'import-dialogue-1714:src-tauri/src/image_processing.rs#calculate_auto_adjustments', 'combine', 'Retain Argentum’s precision-aware processing and import-dialogue-1714’s reuse of calculate_auto_adjustments; neither feature replaces the other.'),
  review164Decision('40a112e1', 'ai-super-resolution:src/components/panel/Editor.tsx', 'combine', 'Keep both independent editor changes: RapidRAW adds the optional neutral-grey canvas, while super-resolution adjusts the minimum zoom bound so enlarged images can fit back into the viewport.'),
];

// Added 2026-10-05, when highlight-recovery registered the dependency it had
// all along. The catch-up took the removal of the compression pass and declined
// the post-demosaic recovery that replaced it, and nothing else made a fully
// blown block white: from 2026-09-23 those came out magenta. Recorded against
// the commits that caused it, in the window they landed in.
const review164HighlightCorrection = [
  review164Decision('85bf424a', 'highlight-recovery:src-tauri/src/raw_processing.rs#develop_internal', 'combine', 'Its post-demosaic colour recovery stays excluded: it judges by colour after demosaic and would stack on Argentum’s pre-demosaic reconstruction. What it also did — make fully blown highlights white once the compression pass was gone — is now done by highlights::settle_blown, in the CFA before demosaic, from the photosites that are actually at their ceiling. The exclusion without that replacement is what made blown highlights magenta from 2026-09-23.'),
  review164Decision('f00145c1', 'highlight-recovery:src-tauri/src/raw_processing.rs#develop_internal', 'combine', 'The compression pass this commit switched off was the only thing turning fully blown highlights white. settle_blown takes over that effect before demosaic; the legacy raw_highlight_compression setting stays readable for migration only, as decided above.'),
];

// Added 2026-10-06, when object-brush registered the upstream code it reads.
// The feature edits none of their files, but naming its dependencies puts the
// commits in this window that touched them up for review. Each was read
// against what the brush relies on.
const review164ObjectBrush = [
  review164Decision('ad179a45', 'object-brush:src-tauri/src/lib.rs#get_cached_full_warped_image', 'not-applicable', 'The TIFF export commit changes headless start-up in run(): an InvalidHeadless exit and a match on the launch request. get_cached_full_warped_image is not touched, so the warped image the brush segments is produced exactly as before.'),
  review164Decision('88415dcc', 'object-brush:src-tauri/src/lib.rs#get_cached_full_warped_image', 'not-applicable', 'The merge adds one handler to the command list (handle_import_presets_from_files). Nothing near the warped-image cache.'),
  review164Decision('0e8cd159', 'object-brush:src-tauri/src/lib.rs#get_cached_full_warped_image', 'not-applicable', 'A merge of upstream main into the TIFF branch; its lib.rs changes are the same headless start-up and command-list edits. No line of get_cached_full_warped_image or its geometry hash changes.'),
  review164Decision('e9d6cc74', 'object-brush:src-tauri/src/lib.rs#get_cached_full_warped_image', 'not-applicable', 'Upstream merging PR 1466 brings in the same headless start-up change as ad179a45 and nothing else in lib.rs.'),
  review164Decision('40a112e1', 'object-brush:src/components/panel/Editor.tsx', 'not-applicable', 'The neutral-grey canvas toggle adds a colour preference and its plumbing. isPanningDisabled, which keeps a brush stroke from panning the photo while an ai-subject component is active, is untouched.'),
];

// Added 2026-10-07, when the object brush was removed. Recorded here because
// this is the newest review: the brush's readings above stand for this window,
// and no later window owes it anything, since the code is gone before any of
// them is merged. Nothing upstream did caused it.
const review164ObjectBrushRetired = [
  {
    overlap: 'retire:object-brush',
    verdict: 'adopt',
    why: 'Their Subject mask does the job: a box drawn round one eye selected that eye, where a stroke along the other took its lids and brow. Ours is dropped; existing masks made with it are ordinary Subject masks and keep rendering.',
  },
];

// Added 2026-10-06, when model-manager moved from a section of About to a tab
// inside Settings > Processing and so began depending on their settings panel:
// the existing data-argentum slot, now rendered just before the Processing page.
const review164ModelsTab = [
  review164Decision('f00145c1', 'model-manager:src/components/panel/SettingsPanel.tsx', 'not-applicable', 'Comments out the highlight compression control inside a Processing card. The page still follows the slot as one element, which is all ProcessingTabs relies on to hide it while AI Models is picked.'),
  review164Decision('40a112e1', 'model-manager:src/components/panel/SettingsPanel.tsx', 'not-applicable', 'Adds the neutral-grey canvas switch to the General page. The Processing page and the slot are untouched, and upstream has no models page of its own.'),
];

// Added 2026-10-05, when ai-gpu-runtime registered its step around
// ORT_DYLIB_PATH in lib.rs. The four commits in this window that touch lib.rs
// are the TIFF-precision chain (#1466) and its merges; none of them changes how
// or when ONNX Runtime is located or loaded.
const review164GpuRuntime = [
  review164Decision('ad179a45', 'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH', 'not-applicable', 'Adds the headless TIFF bit-depth option to launch handling in lib.rs. The ORT_DYLIB_PATH setup is untouched and still runs after startup::init, so the DirectML pin still takes precedence.'),
  review164Decision('88415dcc', 'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH', 'not-applicable', 'A one-line merge resolution in lib.rs for the TIFF branch; nothing near the ONNX Runtime path.'),
  review164Decision('0e8cd159', 'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH', 'not-applicable', 'The TIFF-precision sync reworks builder setup and headless launch handling in lib.rs. No ORT_DYLIB_PATH or ONNX Runtime line is in its diff, and nothing loads ONNX Runtime before setup.'),
  review164Decision('e9d6cc74', 'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH', 'not-applicable', 'The merge of #1466 carries the same TIFF launch-option change as ad179a45; the ONNX Runtime setup is untouched.'),
];

// Added 2026-10-05, when ai-super-resolution began framing photos with the
// export's own apply_all_transformations before enlarging them.
const review164SuperResolutionFraming = [
  review164Decision('ad179a45', 'ai-super-resolution:src-tauri/src/adjustment_utils.rs#apply_all_transformations', 'not-applicable', 'The TIFF-precision export reworks the code around its apply_all_transformations call in export_processing.rs; the call, the function and the order of its geometric steps are unchanged, so enlargement still frames a photo exactly as an export does.'),
];

export const REVIEW_164_DECISIONS = [
  ...review164ShaderDecisions,
  ...review164OtherDecisions,
  ...review164FeatureDecisions,
  ...review164CatchupDecisions,
  ...review164PresetDecisions,
  ...review164CompositionDecisions,
  ...review164HighlightCorrection,
  ...review164ObjectBrush,
  ...review164ObjectBrushRetired,
  ...review164ModelsTab,
  ...review164GpuRuntime,
  ...review164SuperResolutionFraming,
];

// 219 from the catch-up itself, the two highlight corrections, the five
// object-brush readings and its retirement, the two Processing > AI Models
// readings, the four ai-gpu-runtime decisions and the framing decision above.
if (REVIEW_164_DECISIONS.length !== 234) {
  throw new Error(`RapidRAW 1.6.4 review should account for 234 decisions, found ${REVIEW_164_DECISIONS.length}`);
}

// ============================================================================
// RapidRAW 1.6.5: 71a07921..79c2a46b, reviewed 2026-10-07
// ============================================================================
//
// 818 overlaps from 149 commits. Upstream built a white balance of its
// own (9ba20c02 and after), and it replaces ours; most of the rest is upstream
// editing a file one of our dependencies sits in without reaching it.
//
// Those are not asserted. For a dependency on a symbol or a key, the reason
// says where the commit's hunks are, read from the diff (the functions git
// labels them with), and that no changed line names the symbol and no hunk is
// labelled with it. Where one does, the decision is written out by hand below. A merge
// commit with no change of its own points at the commits inside it. A
// dependency on a whole file gets one sentence for what we rely on in that
// file, after a line saying what the commit did.

const review165 = (commit, overlap, verdict, why, kind = 'dep') => ({
  overlap: `${commit}:${kind}:${overlap}`,
  verdict,
  why,
});

// What each commit does, from its diff.
const DID_165 = {
  'd14887fd': 'Advances the selection when the active image is filtered out.',
  '83fc86a8': 'Adds rating operators to the filters.',
  '48a124f5': 'Adds AI-Free mode, which hides the AI controls.',
  'c91e0bf7': 'Rewrites the Whites adjustment and bumps the version to 1.6.5.',
  '1cc99d56': 'Improves shadows and highlights with mid-scale detail reinjection.',
  '66fa1600': 'Implements a log-space guided filter for the edge-aware adjustments: build_guided_coeffs, two new shader bindings.',
  '2641891c': 'Scopes Cloud to desktop: a desktop-cloud capability, the Clerk plugin behind cfg(desktop).',
  '3f3e7df1': 'Removes their white balance tests.',
  '667f2e4d': 'Restores a sensible size and position after unmaximising.',
  'b66691ba': 'Integrates tauri-plugin-clerk with RapidRAW\'s production key: Clerk sign-in at launch, the Cloud provider, getrapidraw.com.',
  '89020724': 'Prevents NaN in vibrance and skips an inactive HSL panel.',
  'e4d6fd16': 'Returns a Kelvin white balance from the area sampler.',
  '9fe2ee29': 'Shares the sRGB decode and the sample outline in the picker code (srgb_channel_to_linear).',
  '3ba2fcae': 'Makes the white balance picker sample an area of the original image in Rust (sample_white_balance).',
  '8aa523bc': 'Spaces the Kelvin slider logarithmically.',
  '73bc73f4': 'Keeps an extreme warm and tint white balance from going black.',
  '28fa5120': 'Removes duplicated white balance code between Rust, the shader and the frontend.',
  '11e20e77': 'Interpolates the A and D65 camera matrices for the as-shot white balance.',
  '9ba20c02': 'Adds the Kelvin white balance mode: their own engine (log-LMS Bradford gains, white_balance.rs), as-shot white balance read from the RAW, temperature and tint replaced in GlobalAdjustments by wb_log_gain_*, per-mask white balance, a Kelvin/relative switch.',
  '13611ab8': 'Reviews Czech: fixes mistranslations, adds missing keys.',
  'f98d68a4': 'Adds the Czech translation baseline and registers it.',
  '476389ad': 'Persists TIFF bit depth and the file timestamp option in export presets.',
  'c4ba9ac9': 'Adds pick and reject flags for culling: a flag field in the sidecar, badges, filters, shortcuts, deleting rejects, XMP rating -1.',
  'cf6813f1': 'Fixes a stale export cache.',
  'e8834210': 'Computes the HSL mixer hue in a perceptual space.',
  'c11c7a5c': 'Reads a Nikon lens from the MakerNote when EXIF has none.',
  '43248097': 'Shares that embedded-preview fallback between thumbnails and the image loader.',
  '4e45e620': 'Uses rawler\'s embedded preview for non-TIFF RAW thumbnails (extract_embedded_preview).',
  '9671795e': 'Runs a custom Escape handler in the masks panel instead of storing a wrapper.',
  '078c90a8': 'Builds colour and luminance mask sources from the exported image rather than the preview.',
  'e99082ad': 'Makes the post-demosaic magenta highlight correction continuous (recover_clipped_pixel).',
  'e3022f65': 'Embeds an sRGB ICC profile in JPEG and PNG exports.',
  'b64adfc6': 'Stops the app aborting or hanging when stdout or stderr is a closed pipe: println! becomes cli_println!.',
  'd6cda855': 'Avoids float image copies when uploading to the GPU (to_rgba_f16 converts in place).',
  '3e186ce2': 'Renames Super-Focus mode to Tool Focus mode.',
  '181a7e32': 'Shares the section tool id lookup.',
  '290818d5': 'Adds Super-Focus mode, which collapses the other tools in a panel when one opens.',
  '66b800f5': 'Adds the Dutch translation and registers it.',
  'b4994ed4': 'Lets the unrated filter rating be saved.',
  'b4a9d1a3': 'Hides chromatic aberration and calibration by default.',
  '6d348a0d': 'Restores a Button import in the crop panel.',
  '454c33d6': 'Keeps a custom crop ratio editable when it matches a preset.',
  '7514c8f6': 'Removes the old adjustment_visibility setting in favour of adjustment_layout.',
  '2f907e36': 'Groups the adjustment layout settings into one adjustment_layout entry.',
  'd94777fe': 'Makes adjustment sub-sections collapsible (AdjustmentSubSection).',
  'f4a4c8d0': 'Lets tools be reordered within an adjustment section.',
  'c7c42306': 'Moves adjustment visibility into the Customise Panels submenu.',
  'f6220886': 'Moves Customise Panels into a section context submenu.',
  '45fa2ca1': 'Makes white the default export pad and border colour.',
  '7ce629fc': 'Remembers which adjustment sections are open across restarts.',
  '0f564b59': 'Adds a customisable order and visibility for adjustment sections, stored as adjustment_layout.',
  'edb8c82c': 'Guards the export resize against a zero-size target.',
  '9a17644d': 'Folds duplicated export geometry (resize, border, pad) into shared helpers.',
  'd151849b': 'Adds a border to export: settings, panel controls, and the border in the export geometry.',
  'ad25d2ba': 'Adds pad-to-aspect-ratio to export: settings, panel controls, and padding in the export geometry.',
  '9b7e3368': 'Adds Apple RAW 9 (macOS Core Image) as an opt-in RAW decode and denoise path, use_apple_raw9.',
  'cbca858f': 'Lowers container padding in the Color, Details and Effects panels.',
  '706026a6': 'Adds custom crop ratios saved in settings (custom_aspect_ratios).',
  '5c4b7800': 'Clippy fixes in app_state.rs, inpainting.rs and the settings panel.',
  '06c52a49': 'Moves cloud usage into useCloudStore and adapts the AI panel for cloud inpainting.',
  'f62a8365': 'Makes AI tasks cancellable: AiTaskGuard and a cancel_ai_task command in Rust, progress in the UI.',
  'f3e50211': 'Adds a centre mark to the crop overlay.',
  '79c2a46b': 'Adds the 1.6.5 release notes to the Flatpak metainfo.',
  '7e6a38c2': 'Adds flags to the quick filters.',
  '1e6777ed': 'Uses vendored OpenSSL on Android for CI.',
  '1732175a': 'Bumps Tauri to 2.12 (tao 0.37, wry 0.57).',
  'd4429f1f': 'Another Android CI fix.',
  '63472b86': 'Fixes Android OpenSSL in CI.',
  '7cc0b674': 'Restyles a cancel button and adds its strings.',
  '5572f96a': 'Always shows the white balance mode toggle.',
  'f2c3473b': 'Fixes white balance slider design bugs.',
  'bcc6e1e9': 'Remounts the white balance sliders when the mode switches.',
  'fe5e2cc2': 'Fixes the colour grading panel padding.',
  '4227dcbd': 'Removes an unused isWgpuEnabled prop from the Color panel.',
  'aff9b3f2': 'Polishes UI and uses the medium thumbnail for the culling preview.',
  '8155513d': 'Avoids string concatenation in the picker\'s swatch labels.',
  '41bdbaad': 'Stops the bottom bar clipping out of the window.',
  '367145a1': 'Respects the maximum AI tag count.',
  'ef6e64c4': 'Removes the border ordering hint from the export panel.',
  'e8d0d2c3': 'Shares input handling between the export panel fields.',
  '3c864270': 'Resizes the crop from its centre while Ctrl is held on a handle.',
  '65472097': 'Fixes the crop frame jumping on Ctrl-drag in crop mode.',
  // Added with compact-sliders, registered after this review: the first
  // dependency of ours these two commits reach.
  '9b822ef1': 'Shares curve channel and gradient helpers; in Slider.tsx a getFraction helper replaces three inline fractions.',
  'b5802863': 'Draws coloured markers on the slider bar for the inactive curve channels.',
};

// The diff does not reach what we rely on: [commit, file, where its hunks are,
// [entry#symbol or entry:key]]. A null 'where' is a commit that reaches the
// file only by naming the symbol elsewhere.
const UNREACHED_165 = [
  ['d14887fd', 'src/App.tsx', 'App', ['no-cloud#initAuth']],
  ['83fc86a8', 'src-tauri/src/app_settings.rs', 'FilterCriteria, Default', ['import-dialogue-1714#last_import_settings']],
  ['83fc86a8', 'src/components/ui/AppProperties.tsx', 'FlagStatus, FilterCriteria', ['import-dialogue-1714#ImportSettings']],
  ['48a124f5', 'src/components/panel/right/MasksPanel.tsx', 'MasksPanel', ['clipping-view:showClipping']],
  ['c91e0bf7', 'src-tauri/src/image_processing.rs', 'SCALES', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'adjustments-path-argument#get_all_adjustments_from_json', 'import-dialogue-1714#calculate_auto_adjustments', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['c91e0bf7', 'src-tauri/src/shaders/shader.wgsl', 'apply_tonal_adjustments, main', ['camera-profile#GlobalAdjustments', 'camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture', 'raw-tone#GlobalAdjustments']],
  ['c91e0bf7', 'src-tauri/src/gpu_processing.rs', 'GpuProcessor', ['display-transform#ag_display_matrix', 'high-precision-export#GpuProcessor::new', 'high-precision-export#read_texture_data_roi', 'high-precision-export#to_rgba_f16', 'high-precision-export#GpuProcessor::run', 'high-precision-export#process_and_get_dynamic_image_inner']],
  ['1cc99d56', 'src-tauri/src/shaders/shader.wgsl', 'SHARPEN_DARK_SCALE, apply_tonal_adjustments, apply_highlights_adjustment, apply_glow_bloom and 2 more', ['camera-profile#GlobalAdjustments', 'camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture', 'raw-tone#GlobalAdjustments']],
  ['66fa1600', 'src-tauri/src/app_state.rs', 'GpuImageCache', ['auto-white-balance#as_shot_white_balance']],
  ['66fa1600', 'src-tauri/src/shaders/shader.wgsl', 'HSL_RANGES, apply_tonal_adjustments, apply_highlights_adjustment, apply_color_grading and 8 more', ['camera-profile#GlobalAdjustments', 'camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture', 'raw-tone#GlobalAdjustments']],
  ['66fa1600', 'src-tauri/src/gpu_processing.rs', 'wgpu, BlurParams, GpuProcessor, process_and_get_dynamic_image_inner', ['display-transform#ag_display_matrix', 'high-precision-export#GpuProcessor::new', 'high-precision-export#read_texture_data_roi', 'high-precision-export#to_rgba_f16', 'high-precision-export#GpuProcessor::run']],
  ['2641891c', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['2641891c', 'src/store/useCloudStore.ts', 'import { fetch } from \'@tauri-, CloudUsage, useCloudStore', ['no-cloud#initAuth']],
  ['3f3e7df1', 'src-tauri/src/image_processing.rs', 'sample_white_balance', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'adjustments-path-argument#get_all_adjustments_from_json', 'import-dialogue-1714#calculate_auto_adjustments', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['667f2e4d', 'src-tauri/src/lib.rs', 'MonitorBounds, available_monitor_bounds, frontend_ready, run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['b66691ba', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['89020724', 'src-tauri/src/shaders/shader.wgsl', 'apply_creative_color, apply_hsl_panel', ['camera-profile#GlobalAdjustments', 'camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture', 'raw-tone#GlobalAdjustments']],
  ['e4d6fd16', 'src-tauri/src/white_balance.rs', 'from_adjustments', ['auto-white-balance#pick_white_balance', 'auto-white-balance#adaptation_log_gains', 'wb-legacy#shifted']],
  ['e4d6fd16', 'src/utils/whiteBalance.ts', null, ['auto-white-balance#withRelativeWhiteBalance']],
  ['e4d6fd16', 'src-tauri/src/image_processing.rs', 'point_in_convex_quad, compute_white_balance_sample, sample_white_balance, mod white_balance_sample_tests', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'adjustments-path-argument#get_all_adjustments_from_json', 'import-dialogue-1714#calculate_auto_adjustments', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['e4d6fd16', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['e4d6fd16', 'src/components/ui/AppProperties.tsx', 'Invokes', ['import-dialogue-1714#ImportSettings']],
  ['9fe2ee29', 'src-tauri/src/image_processing.rs', 'apply_cpu_default_raw_processing, apply_srgb_to_linear, MAX_WB_SAMPLES', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'adjustments-path-argument#get_all_adjustments_from_json', 'import-dialogue-1714#calculate_auto_adjustments', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['3ba2fcae', 'src/utils/whiteBalance.ts', null, ['auto-white-balance#withRelativeWhiteBalance']],
  ['3ba2fcae', 'src-tauri/src/image_processing.rs', 'calculate_auto_adjustments', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'adjustments-path-argument#get_all_adjustments_from_json', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['3ba2fcae', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['3ba2fcae', 'src/components/ui/AppProperties.tsx', 'Invokes', ['import-dialogue-1714#ImportSettings']],
  ['8aa523bc', 'src/utils/whiteBalance.ts', 'kelvinSliderScale', ['auto-white-balance#withRelativeWhiteBalance']],
  ['8aa523bc', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['73bc73f4', 'src-tauri/src/white_balance.rs', 'ILLUMINANT_D65_TEMPERATURE, WhiteBalance', ['auto-white-balance#pick_white_balance', 'auto-white-balance#adaptation_log_gains', 'wb-legacy#shifted']],
  ['28fa5120', 'src-tauri/src/white_balance.rs', 'crate, TINT_SCALE, ILLUMINANT_D65_TEMPERATURE, WhiteBalance and 2 more', ['wb-legacy#shifted']],
  ['28fa5120', 'src/utils/whiteBalance.ts', 'resolveWhiteBalance, toRelativeWhiteBalance', ['auto-white-balance#withRelativeWhiteBalance']],
  ['28fa5120', 'src-tauri/src/image_processing.rs', 'GlobalAdjustments, convert_points_to_aligned, xy_to_xyz, is_image_edited and 3 more', ['clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'import-dialogue-1714#calculate_auto_adjustments']],
  ['28fa5120', 'src-tauri/src/shaders/shader.wgsl', 'GlobalAdjustments, apply_color_calibration, apply_white_balance', ['camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture']],
  ['28fa5120', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['28fa5120', 'src-tauri/src/export_processing.rs', null, ['adjustments-path-argument#get_all_adjustments_from_json']],
  ['28fa5120', 'src-tauri/src/lut_processing.rs', null, ['adjustments-path-argument#get_all_adjustments_from_json']],
  ['11e20e77', 'src-tauri/src/white_balance.rs', 'D65_XY, WhiteBalance', ['auto-white-balance#pick_white_balance', 'auto-white-balance#adaptation_log_gains', 'wb-legacy#shifted']],
  ['11e20e77', 'src-tauri/src/raw_processing.rs', 'read_as_shot_white_balance', ['raw-decode#on_raw_decoded', 'canon-old-wb#on_raw_decoded', 'highlight-recovery#develop_internal', 'raw-tone#develop_raw_image']],
  ['11e20e77', 'src-tauri/src/multi_exposure.rs', null, ['highlight-recovery#neutralize_wb_if_multiexposure']],
  ['9ba20c02', 'src-tauri/src/image_processing.rs', 'crate, GlobalAdjustments, MaskAdjustments, AdjustmentScales and 5 more', ['clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'import-dialogue-1714#calculate_auto_adjustments']],
  ['9ba20c02', 'src-tauri/src/shaders/shader.wgsl', 'GlobalAdjustments, MaskAdjustments, apply_color_calibration, main', ['camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture']],
  ['9ba20c02', 'src/utils/adjustments.ts', 'import type { AdjustmentLayout, ColorAdjustment, Adjustments, INITIAL_ADJUSTMENTS and 2 more', ['camera-profile:cameraProfile', 'raw-tone:rawToneRendering']],
  ['9ba20c02', 'src/components/panel/right/ControlsPanel.tsx', 'Controls', ['clipping-view:showClipping']],
  ['9ba20c02', 'src-tauri/src/raw_processing.rs', 'crate, rawler, develop_internal', ['raw-decode#on_raw_decoded', 'canon-old-wb#on_raw_decoded', 'raw-tone#develop_raw_image']],
  ['9ba20c02', 'src-tauri/src/image_loader.rs', 'crate, LoadImageResult, load_image', ['raw-decode#load_base_image_from_bytes', 'highlight-recovery#load_base_image_from_bytes', 'raw-tone#embedded_preview_fallback']],
  ['9ba20c02', 'src-tauri/src/formats.rs', null, ['tif-raw-sniffing#is_raw_file']],
  ['9ba20c02', 'src-tauri/src/lib.rs', 'mod tagging_utils;, crate, process_preview_job, generate_uncropped_preview and 4 more', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['9ba20c02', 'src-tauri/src/multi_exposure.rs', null, ['highlight-recovery#neutralize_wb_if_multiexposure']],
  ['9ba20c02', 'src-tauri/src/export_processing.rs', 'crate, process_image_for_export_pipeline, export_masks_for_image, export_adjustments_as_lut and 1 more', ['high-precision-export#process_image_for_export', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'tiff-export-metadata#save_image_with_metadata']],
  ['9ba20c02', 'src-tauri/src/file_management.rs', 'generate_thumbnail_data', ['import-dialogue-1714#ImportSettings', 'wb-legacy#load_metadata']],
  ['9ba20c02', 'src-tauri/src/app_settings.rs', 'all_available_adjustments, AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['9ba20c02', 'src/components/ui/AppProperties.tsx', 'import { ToolType } from \'../p, Invokes, AppSettings, SelectedImage', ['import-dialogue-1714#ImportSettings']],
  ['13611ab8', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['f98d68a4', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['f98d68a4', 'src/components/panel/editor/Waveform.tsx', null, ['clipping-view:showClipping']],
  ['f98d68a4', 'src/components/panel/right/ControlsPanel.tsx', null, ['clipping-view:showClipping']],
  ['f98d68a4', 'src/components/panel/right/MasksPanel.tsx', null, ['clipping-view:showClipping']],
  ['476389ad', 'src-tauri/src/app_settings.rs', 'ExportPreset, default_export_presets', ['import-dialogue-1714#last_import_settings']],
  ['c4ba9ac9', 'src/App.tsx', 'App', ['no-cloud#initAuth']],
  ['c4ba9ac9', 'src-tauri/src/image_processing.rs', 'std, ImageMetadata, Default', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'adjustments-path-argument#get_all_adjustments_from_json', 'import-dialogue-1714#calculate_auto_adjustments', 'wb-legacy#get_all_adjustments_from_json', 'raw-tone#GlobalAdjustments']],
  ['c4ba9ac9', 'src-tauri/src/formats.rs', null, ['tif-raw-sniffing#is_raw_file']],
  ['c4ba9ac9', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['c4ba9ac9', 'src-tauri/src/file_management.rs', 'crate, ImageFileMetadata, resolve_image_metadata, emit_image_metadata_loaded and 12 more', ['import-dialogue-1714#ImportSettings', 'wb-legacy#load_metadata']],
  ['c4ba9ac9', 'src-tauri/src/app_settings.rs', 'FilterCriteria, Default', ['import-dialogue-1714#last_import_settings']],
  ['c4ba9ac9', 'src/components/modals/AppModals.tsx', 'import CollageModal from \'./Co, AppModalsProps, AppModals', ['import-dialogue-1714#ImportSettingsModal']],
  ['c4ba9ac9', 'src/components/ui/AppProperties.tsx', 'Invokes, EditedStatus, FilterCriteria, ImageFile', ['import-dialogue-1714#ImportSettings']],
  ['c4ba9ac9', 'src/hooks/useFileOperations.ts', 'import { toast } from \'react-t, import { useSettingsStore } fr, useFileOperations', ['import-dialogue-1714#handleStartImport']],
  ['cf6813f1', 'src-tauri/src/export_processing.rs', 'export_images_impl', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#export_masks_for_image', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['e8834210', 'src-tauri/src/shaders/shader.wgsl', 'apply_hsl_panel', ['camera-profile#GlobalAdjustments', 'camera-profile#ag_stage_scene_linear', 'clipping-view#ag_stage_display', 'highlight-recovery#ag_stage_scene_linear', 'high-precision-export#output_texture', 'raw-tone#GlobalAdjustments']],
  ['c11c7a5c', 'src-tauri/src/exif_processing.rs', 'rawler, format_min_max, format_lens_specification, read_raw_metadata and 1 more', ['my-gear#fill_lens_model', 'tiff-export-metadata#write_image_with_metadata']],
  ['43248097', 'src-tauri/src/image_loader.rs', 'largest_tiff_jpeg_preview, embedded_preview_fallback', ['raw-decode#load_base_image_from_bytes', 'highlight-recovery#load_image', 'highlight-recovery#load_base_image_from_bytes']],
  ['43248097', 'src-tauri/src/file_management.rs', 'try_load_embedded_raw_preview', ['import-dialogue-1714#ImportSettings', 'wb-legacy#load_metadata']],
  ['4e45e620', 'src-tauri/src/raw_processing.rs', 'rawler, develop_raw_image, develop_internal', ['raw-decode#on_raw_decoded', 'canon-old-wb#on_raw_decoded', 'canon-old-wb#read_as_shot_white_balance']],
  ['4e45e620', 'src-tauri/src/file_management.rs', 'apply_exif_orientation, try_load_embedded_raw_preview', ['import-dialogue-1714#ImportSettings', 'wb-legacy#load_metadata']],
  ['9671795e', 'src/components/panel/right/MasksPanel.tsx', 'MasksPanel', ['clipping-view:showClipping']],
  ['078c90a8', 'src-tauri/src/image_processing.rs', 'get_all_adjustments_from_json', ['camera-profile#GlobalAdjustments', 'clipping-view#show_clipping', 'preview-encode#apply_cpu_default_raw_processing', 'import-dialogue-1714#calculate_auto_adjustments', 'raw-tone#GlobalAdjustments']],
  ['078c90a8', 'src-tauri/src/lib.rs', 'crate, get_cached_full_warped_image, process_preview_job, generate_uncropped_preview and 2 more', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['078c90a8', 'src-tauri/src/export_processing.rs', 'crate, process_image_for_export_pipeline, export_masks_for_image, export_images_impl and 1 more', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'tiff-export-metadata#save_image_with_metadata']],
  ['078c90a8', 'src-tauri/src/lut_processing.rs', null, ['adjustments-path-argument#get_all_adjustments_from_json']],
  ['078c90a8', 'src-tauri/src/file_management.rs', 'generate_thumbnail_data', ['import-dialogue-1714#ImportSettings', 'wb-legacy#load_metadata']],
  ['e99082ad', 'src-tauri/src/raw_processing.rs', 'recover_clipped_pixel', ['raw-decode#on_raw_decoded', 'canon-old-wb#on_raw_decoded', 'canon-old-wb#read_as_shot_white_balance', 'highlight-recovery#develop_internal', 'raw-tone#develop_raw_image']],
  ['e3022f65', 'src-tauri/src/export_processing.rs', 'image, encode_grayscale_to_png, encode_image_to_bytes', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#export_masks_for_image', 'high-precision-export#apply_watermark', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['b64adfc6', 'src-tauri/src/lib.rs', 'GLOBAL, setup_logging, run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version']],
  ['b64adfc6', 'src-tauri/src/export_processing.rs', 'run_headless_export', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#export_masks_for_image', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['d6cda855', 'src-tauri/src/gpu_processing.rs', 'to_rgba_f16', ['display-transform#ag_display_matrix', 'high-precision-export#GpuProcessor::new', 'high-precision-export#read_texture_data_roi', 'high-precision-export#GpuProcessor::run', 'high-precision-export#process_and_get_dynamic_image_inner']],
  ['3e186ce2', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['3e186ce2', 'src/components/ui/AppProperties.tsx', 'AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['181a7e32', 'src/utils/adjustments.ts', 'ADJUSTMENT_SECTION_TOOLS, getAdjustmentToolOrder', ['camera-profile:cameraProfile', 'wb-legacy:whiteBalance', 'raw-tone:rawToneRendering']],
  ['290818d5', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['290818d5', 'src/components/ui/AppProperties.tsx', 'AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['66b800f5', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['66b800f5', 'src/components/panel/editor/Waveform.tsx', null, ['clipping-view:showClipping']],
  ['66b800f5', 'src/components/panel/right/ControlsPanel.tsx', null, ['clipping-view:showClipping']],
  ['66b800f5', 'src/components/panel/right/MasksPanel.tsx', null, ['clipping-view:showClipping']],
  ['b4994ed4', 'src-tauri/src/app_settings.rs', 'FilterCriteria', ['import-dialogue-1714#last_import_settings']],
  ['b4a9d1a3', 'src/utils/adjustments.ts', 'getVisibleAdjustmentSections', ['camera-profile:cameraProfile', 'wb-legacy:whiteBalance', 'raw-tone:rawToneRendering']],
  ['b4a9d1a3', 'src-tauri/src/app_settings.rs', 'AdjustmentLayout', ['import-dialogue-1714#last_import_settings']],
  ['6d348a0d', 'src/components/panel/right/CropPanel.tsx', 'import Dropdown from \'../../ui', ['argentum-shell#useAutoDetectOnLoad']],
  ['454c33d6', 'src/components/panel/right/CropPanel.tsx', 'import Dropdown from \'../../ui, CropPanel', ['argentum-shell#useAutoDetectOnLoad']],
  ['7514c8f6', 'src/utils/adjustments.ts', 'AdjustmentSectionTool, ADJUSTMENT_SECTION_TOOLS', ['camera-profile:cameraProfile', 'raw-tone:rawToneRendering']],
  ['7514c8f6', 'src-tauri/src/app_settings.rs', 'AdjustmentLayout, default_tagging_shortcuts_option, AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['7514c8f6', 'src/components/ui/AppProperties.tsx', 'AppSettings, AdjustmentLayout', ['import-dialogue-1714#ImportSettings']],
  ['2f907e36', 'src/utils/adjustments.ts', 'import { SubMask, SubMaskMode , getAdjustmentSectionOrder', ['camera-profile:cameraProfile', 'wb-legacy:whiteBalance', 'raw-tone:rawToneRendering']],
  ['2f907e36', 'src/components/panel/right/ControlsPanel.tsx', 'Controls', ['clipping-view:showClipping']],
  ['2f907e36', 'src/components/panel/right/MasksPanel.tsx', 'SettingsPanel', ['clipping-view:showClipping']],
  ['2f907e36', 'src-tauri/src/app_settings.rs', 'default_export_presets, AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['2f907e36', 'src/components/ui/AppProperties.tsx', 'AppSettings, CollapsibleSectionsState', ['import-dialogue-1714#ImportSettings']],
  ['d94777fe', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['d94777fe', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['d94777fe', 'src/components/ui/AppProperties.tsx', 'AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['f4a4c8d0', 'src/utils/adjustments.ts', 'ADJUSTMENT_SECTIONS, getAdjustmentSectionOrder, ADJUSTMENT_SECTION_TOOLS', ['camera-profile:cameraProfile', 'wb-legacy:whiteBalance', 'raw-tone:rawToneRendering']],
  ['f4a4c8d0', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['f4a4c8d0', 'src/components/ui/AppProperties.tsx', 'AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['c7c42306', 'src/utils/adjustments.ts', 'getVisibleAdjustmentSections', ['camera-profile:cameraProfile', 'raw-tone:rawToneRendering']],
  ['f6220886', 'src/components/panel/right/ControlsPanel.tsx', 'import Resizer from \'../../ui/, Controls', ['clipping-view:showClipping']],
  ['f6220886', 'src/components/panel/right/MasksPanel.tsx', 'import {, import { DepthRangePicker } fr, SettingsPanel', ['clipping-view:showClipping']],
  ['45fa2ca1', 'src-tauri/src/app_settings.rs', 'default_export_presets', ['import-dialogue-1714#last_import_settings']],
  ['7ce629fc', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['7ce629fc', 'src/components/ui/AppProperties.tsx', 'AppSettings, UiVisibility', ['import-dialogue-1714#ImportSettings']],
  ['0f564b59', 'src/utils/adjustments.ts', 'ADJUSTMENT_SECTIONS', ['camera-profile:cameraProfile', 'wb-legacy:whiteBalance', 'raw-tone:rawToneRendering']],
  ['0f564b59', 'src/components/panel/right/ControlsPanel.tsx', 'import Resizer from \'../../ui/, Controls', ['clipping-view:showClipping']],
  ['0f564b59', 'src/components/panel/right/MasksPanel.tsx', 'import {, SettingsPanel', ['clipping-view:showClipping']],
  ['0f564b59', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['0f564b59', 'src/components/ui/AppProperties.tsx', 'AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['edb8c82c', 'src-tauri/src/export_processing.rs', 'compute_fused_geometry', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#export_masks_for_image', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['9a17644d', 'src-tauri/src/export_processing.rs', 'BorderOptions, PadOptions, parse_hex_color, calculate_pad_target and 11 more', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['d151849b', 'src-tauri/src/export_processing.rs', 'ResizeOptions, ExportSettings, MAX_PAD_DIMENSION, ExportGeometry and 9 more', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#apply_watermark', 'high-precision-export#encode_image_to_bytes', 'export-precision-selector#estimate_export_sizes', 'tiff-export-metadata#save_image_with_metadata']],
  ['d151849b', 'src-tauri/src/app_settings.rs', 'ExportPreset, default_export_presets', ['import-dialogue-1714#last_import_settings']],
  ['ad25d2ba', 'src-tauri/src/export_processing.rs', 'ResizeOptions, ExportSettings, apply_watermark, calculate_resize_target and 5 more', ['adjustments-path-argument#get_all_adjustments_from_json', 'high-precision-export#process_image_for_export', 'high-precision-export#encode_image_to_bytes', 'tiff-export-metadata#save_image_with_metadata']],
  ['ad25d2ba', 'src-tauri/src/app_settings.rs', 'ExportPreset, default_export_presets', ['import-dialogue-1714#last_import_settings']],
  ['ad25d2ba', 'src-tauri/src/adjustment_utils.rs', null, ['ai-super-resolution#apply_all_transformations']],
  ['9b7e3368', 'src-tauri/src/raw_processing.rs', null, ['raw-tone#develop_raw_image']],
  ['9b7e3368', 'src-tauri/src/image_loader.rs', 'load_base_image_from_bytes', ['highlight-recovery#load_image', 'raw-tone#embedded_preview_fallback']],
  ['9b7e3368', 'src-tauri/src/formats.rs', null, ['tif-raw-sniffing#is_raw_file']],
  ['9b7e3368', 'src-tauri/src/lib.rs', 'mod app_state;, run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['9b7e3368', 'src-tauri/src/app_settings.rs', 'AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['9b7e3368', 'src/components/modals/AppModals.tsx', 'import DenoiseModal from \'./De, AppModalsProps', ['import-dialogue-1714#ImportSettingsModal']],
  ['9b7e3368', 'src/components/ui/AppProperties.tsx', 'Invokes', ['import-dialogue-1714#ImportSettings']],
  ['cbca858f', 'src/utils/adjustments.ts', null, ['wb-legacy:whiteBalance']],
  ['706026a6', 'src-tauri/src/app_settings.rs', 'MyLens, AppSettings, Default', ['import-dialogue-1714#last_import_settings']],
  ['706026a6', 'src/components/ui/AppProperties.tsx', 'WorkspaceState, AppSettings', ['import-dialogue-1714#ImportSettings']],
  ['706026a6', 'src/components/panel/right/CropPanel.tsx', 'import {, import clsx from \'clsx\';, import { useContextMenu } from, parseExifNumber and 1 more', ['argentum-shell#useAutoDetectOnLoad']],
  ['5c4b7800', 'src-tauri/src/app_state.rs', 'impl<\'a> Drop for AiTaskGuard<', ['auto-white-balance#as_shot_white_balance']],
  ['06c52a49', 'src/store/useCloudStore.ts', 'only top-level lines (imports and declarations)', ['no-cloud#initAuth']],
  ['f62a8365', 'src-tauri/src/app_state.rs', 'std, tokio, crate, AppState', ['auto-white-balance#as_shot_white_balance']],
  ['f62a8365', 'src/components/panel/right/MasksPanel.tsx', 'MasksPanel, DraggableGridItem, ContainerRow, SubMaskRow', ['clipping-view:showClipping']],
  ['f62a8365', 'src-tauri/src/lib.rs', 'run', ['display-transform#ag_display_matrix', 'cache-keys#cache_version', 'ai-gpu-runtime#ORT_DYLIB_PATH']],
  ['f62a8365', 'src/components/ui/AppProperties.tsx', 'Invokes', ['import-dialogue-1714#ImportSettings']],
  ['f3e50211', 'src/components/panel/right/CropPanel.tsx', 'RATIO_TOLERANCE, CropPanel', ['argentum-shell#useAutoDetectOnLoad']],
  // object-label was registered after this review; its Settings dependency is
  // checked against the same window here. Only 48a124f5 reaches the line, by hand below.
  ['2641891c', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['b66691ba', 'src/components/panel/SettingsPanel.tsx', 'import { getCurrentWindow } from, AiProviderSwitch, CloudDashboard, SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['f98d68a4', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['c4ba9ac9', 'src/components/panel/SettingsPanel.tsx', 'import { useTranslation } from, KeybindRow, SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['3e186ce2', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['290818d5', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['66b800f5', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['c7c42306', 'src/components/panel/SettingsPanel.tsx', 'MyLens, SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['9b7e3368', 'src/components/panel/SettingsPanel.tsx', 'SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  ['5c4b7800', 'src/components/panel/SettingsPanel.tsx', 'CloudDashboard', ['object-label#settings.processing.ai.cpu.feature1']],
  ['06c52a49', 'src/components/panel/SettingsPanel.tsx', 'import {, AiProviderSwitch, CloudDashboard, SettingsPanel', ['object-label#settings.processing.ai.cpu.feature1']],
  // compact-sliders, registered after this review too. f2c3473b reaches the
  // value column, by hand below.
  ['f2c3473b', 'src/components/ui/Slider.tsx', 'Slider', ['compact-sliders#relative w-full h-5']],
  ['9ba20c02', 'src/components/ui/Slider.tsx', 'import React, SliderMarker, SliderProps, Slider', ['compact-sliders#relative w-full h-5', 'compact-sliders#w-14 text-right shrink-0']],
  ['9b822ef1', 'src/components/ui/Slider.tsx', 'FINE_ADJUSTMENT_MULTIPLIER, Slider', ['compact-sliders#relative w-full h-5', 'compact-sliders#w-14 text-right shrink-0']],
  ['b5802863', 'src/components/ui/Slider.tsx', 'SliderChangeEvent, Slider', ['compact-sliders#relative w-full h-5', 'compact-sliders#w-14 text-right shrink-0']],
];

const unreached165 = UNREACHED_165.flatMap(([commit, file, where, targets]) =>
  targets.map((target) => {
    const symbol = target.includes('#');
    const [entry, thing] = symbol ? target.split('#') : target.split(':');
    const overlap = symbol ? `${entry}:${file}#${thing}` : `${entry}:${file}:${thing}`;
    const place = where === null
      ? `It does not edit ${file}; it names ${thing} elsewhere.`
      : `In ${file} it changes ${where}; no changed line names ${thing}, and no hunk is labelled with it.`;
    return review165(commit, overlap, 'not-applicable', `${DID_165[commit]} ${place}`);
  }),
);

// Dependencies on a whole file: [verdict, what we rely on there, [commits]].
const FILE_165 = {
  'rapidraw-164-catchup|data/io.github.CyberTimon.RapidRAW.metainfo.xml': ['keep-ours', 'Their release notes are not ours; the file stays as our Flatpak recipe installs it, as decided at 1.6.4.', ['79c2a46b']],
  'mask-stage-size-guard|src/App.tsx': ['not-applicable', 'The guard lives in ImageCanvas.tsx; App.tsx is listed for the editor staying mounted while hidden, which this does not change.', ['d14887fd', 'b66691ba', 'c4ba9ac9']],
  'argentum-shell|src/App.tsx': ['not-applicable', 'The <Argentum /> mount and its import are untouched.', ['d14887fd', 'b66691ba', 'c4ba9ac9']],
  'rapidraw-164-catchup|src/store/useUIStore.ts': ['not-applicable', 'Quick Filter visibility, the 1.6.4 carry, is untouched and still initialised on workspace change.', ['d14887fd', '7ce629fc']],
  'rapidraw-164-catchup|src-tauri/src/app_settings.rs': ['combine', 'Their change and the fields we carry (editorNeutralGreyBg, last_import_settings) coexist; nothing of ours is renamed or retyped.', ['83fc86a8', '9ba20c02', '476389ad', 'c4ba9ac9', '3e186ce2', '290818d5', 'b4994ed4', 'b4a9d1a3', '7514c8f6', '2f907e36', 'd94777fe', 'f4a4c8d0', '45fa2ca1', '7ce629fc', '0f564b59', 'd151849b', 'ad25d2ba', '9b7e3368', '706026a6']],
  'rapidraw-164-catchup|src/components/ui/AppProperties.tsx': ['not-applicable', 'The neutral-canvas and Quick Filter fields we carry are untouched.', ['83fc86a8', 'e4d6fd16', '3ba2fcae', '9ba20c02', 'c4ba9ac9', '3e186ce2', '290818d5', '7514c8f6', '2f907e36', 'd94777fe', 'f4a4c8d0', '7ce629fc', '0f564b59', '9b7e3368', '706026a6', 'f62a8365']],
  'rapidraw-164-catchup|src/components/panel/BottomBar.tsx': ['not-applicable', 'Quick Filter visibility still comes from the UI store.', ['83fc86a8', '7e6a38c2', 'c4ba9ac9']],
  'my-gear|src/components/panel/SettingsPanel.tsx': ['not-applicable', 'Their lens UI, the block My Gear replaced, is not in this commit; the edit is elsewhere in the panel.', ['48a124f5', '2641891c', 'b66691ba', 'f98d68a4', 'c4ba9ac9', '3e186ce2', '290818d5', '66b800f5', 'c7c42306', '9b7e3368', '5c4b7800', '06c52a49']],
  'argentum-shell|src/components/panel/SettingsPanel.tsx': ['not-applicable', 'The data-argentum slot line is untouched; it now also renders for General, our change, not theirs.', ['48a124f5', '2641891c', 'b66691ba', 'f98d68a4', 'c4ba9ac9', '3e186ce2', '290818d5', '66b800f5', 'c7c42306', '9b7e3368', '5c4b7800', '06c52a49']],
  'model-manager|src/components/panel/SettingsPanel.tsx': ['combine', 'Their Generative AI card, which AI Models is placed under from the existing slot, is not moved by this commit.', ['48a124f5', '2641891c', 'b66691ba', 'f98d68a4', 'c4ba9ac9', '3e186ce2', '290818d5', '66b800f5', 'c7c42306', '9b7e3368', '5c4b7800', '06c52a49']],
  'no-cloud|src/components/panel/SettingsPanel.tsx': ['not-applicable', 'Their Cloud tile, hidden from our side by NoCloudTile.tsx, is not what this commit edits.', ['48a124f5', '2641891c', 'b66691ba', 'f98d68a4', 'c4ba9ac9', '3e186ce2', '290818d5', '66b800f5', 'c7c42306', '9b7e3368', '5c4b7800', '06c52a49']],
  'rapidraw-164-catchup|src-tauri/src/shaders/shader.wgsl': ['not-applicable', 'The 1.6.4 carries in the shader (Brightness, Vibrance, RGB curves, the 32-bit target) are intact after the merge; this commit edits other stages.', ['c91e0bf7', '1cc99d56', '66fa1600', '89020724', '28fa5120', '9ba20c02', 'e8834210']],
  'high-precision-export|src-tauri/src/shaders/shader.wgsl': ['not-applicable', 'Our bool HIGH_PRECISION_OUTPUT and its dither gate, and the rgba8unorm declaration the export rewrites, are intact; export_shader_source still finds it exactly once (tested).', ['c91e0bf7', '1cc99d56', '66fa1600', '89020724', '28fa5120', '9ba20c02', 'e8834210']],
  'rapidraw-164-catchup|src-tauri/src/gpu_processing.rs': ['combine', 'Our 32-bit export render path is kept; their half-float TIFF pipeline, re-offered in the merge hunks, is declined again as at 1.6.4.', ['c91e0bf7', '66fa1600', 'd6cda855']],
  'identity|src-tauri/tauri.conf.json': ['keep-ours', 'Our version, name and identifier stay; their version bump to 1.6.5 is not taken.', ['c91e0bf7', '2641891c']],
  'no-cloud|src-tauri/tauri.conf.json': ['keep-ours', 'Only the default capability is listed; their desktop-cloud capability is not.', ['c91e0bf7', '2641891c']],
  'highlight-recovery|src-tauri/src/app_state.rs': ['not-applicable', 'redecode::open_photo resets the caches it knows; this commit adds no pixel cache the open photo holds (the post-1.6.5 staged caches are noted for the next review).', ['66fa1600', '9ba20c02', '5c4b7800', 'f62a8365']],
  'no-cloud|src-tauri/src/lib.rs': ['not-applicable', 'The Clerk plugin registration we leave out is not what this commit edits.', ['2641891c', '667f2e4d', 'b66691ba', 'e4d6fd16', '3ba2fcae', '9ba20c02', 'c4ba9ac9', '078c90a8', 'b64adfc6', '9b7e3368', 'f62a8365']],
  'identity|src-tauri/Cargo.toml': ['not-applicable', 'Our package name and metadata are untouched.', ['2641891c', '1e6777ed', 'b66691ba', '1732175a', '9b7e3368']],
  'ai-gpu-runtime|src-tauri/Cargo.toml': ['not-applicable', 'ort stays at their =2.0.0-rc.10; our Windows table entry adding directml still matches it.', ['2641891c', '1e6777ed', 'b66691ba', '1732175a', '9b7e3368']],
  'no-cloud|src-tauri/Cargo.toml': ['not-applicable', 'Not the Clerk and store crates we leave out.', ['2641891c', '1e6777ed', 'b66691ba', '1732175a', '9b7e3368']],
  'rapidraw-164-catchup|.github/workflows/build.yml': ['not-applicable', 'Android CI only; Argentum builds no Android, and their net change to .github/ in this window is zero.', ['2641891c', 'd4429f1f', '63472b86', '1e6777ed']],
  'rapidraw-164-catchup|src/i18n/update_translations.py': ['adopt', 'Their translation extraction is taken as is.', ['2641891c', '7cc0b674', '9b7e3368']],
  'no-cloud|src-tauri/capabilities/desktop.json': ['keep-ours', 'The file is deleted here: it granted clerk:default and nothing else.', ['2641891c']],
  'no-cloud|src-tauri/capabilities/default.json': ['keep-ours', 'Their HTTP allow-list for clerk.getrapidraw.com and www.getrapidraw.com is emptied.', ['2641891c', 'b66691ba']],
  'auto-white-balance|src/components/adjustments/Color.tsx': ['combine', 'Their layout is taken; the color-tools marker sits in their white balance actions row beside K and the picker, placed at the merge.', ['5572f96a', 'f2c3473b', '28fa5120', 'bcc6e1e9', '9ba20c02', 'b4a9d1a3', 'fe5e2cc2', '7514c8f6', '2f907e36', '4227dcbd', 'd94777fe', 'f4a4c8d0', 'c7c42306', 'cbca858f']],
  'camera-profile|src/components/adjustments/Color.tsx': ['combine', 'Their layout is taken; the camera-profile marker is first in the panel, outside their tool sections, placed at the merge.', ['5572f96a', 'f2c3473b', '28fa5120', 'bcc6e1e9', '9ba20c02', 'b4a9d1a3', 'fe5e2cc2', '7514c8f6', '2f907e36', '4227dcbd', 'd94777fe', 'f4a4c8d0', 'c7c42306', 'cbca858f']],
  'raw-tone|src/components/adjustments/Color.tsx': ['combine', 'RAW tone rendering renders in the camera-profile marker, which the merge placed first in their new layout.', ['5572f96a', 'f2c3473b', '28fa5120', 'bcc6e1e9', '9ba20c02', 'b4a9d1a3', 'fe5e2cc2', '7514c8f6', '2f907e36', '4227dcbd', 'd94777fe', 'f4a4c8d0', 'c7c42306', 'cbca858f']],
  'compact-sliders|src/components/panel/SettingsPanel.tsx': ['not-applicable', 'Their Font row on General, which the Compact sliders switch is placed after, is not what this commit edits.', ['48a124f5', '2641891c', 'b66691ba', 'f98d68a4', 'c4ba9ac9', '3e186ce2', '290818d5', '66b800f5', 'c7c42306', '9b7e3368', '5c4b7800', '06c52a49']],
  'mask-stage-size-guard|src/components/panel/editor/ImageCanvas.tsx': ['not-applicable', 'The two positive-size checks on the mask Stage are intact after the merge.', ['aff9b3f2', 'e4d6fd16', '9fe2ee29', '8155513d', '3ba2fcae', '9ba20c02']],
  'rgb-readout|src/components/panel/editor/ImageCanvas.tsx': ['not-applicable', 'The overlay svg photoBox.ts finds is unchanged: still sized in px to the drawn image inside the pan/zoom transform.', ['aff9b3f2', 'e4d6fd16', '9fe2ee29', '8155513d', '3ba2fcae', '9ba20c02']],
  'identity|package.json': ['not-applicable', 'Our package name is untouched.', ['b66691ba', '1732175a']],
  'identity|package-lock.json': ['not-applicable', 'Their lockfile carries their package name; npm writes ours back from package.json.', ['b66691ba', '1732175a']],
  'no-cloud|src-tauri/src/inpainting.rs': ['not-applicable', 'Their cloud branch needs the Cloud provider and a Clerk token; neither exists here.', ['b66691ba', '5c4b7800', 'f62a8365']],
  'borrow-1633|src-tauri/src/raw_processing.rs': ['not-applicable', 'The borrowed 2.4 exponent between its markers is unchanged.', ['11e20e77', '9ba20c02', '4e45e620', 'e99082ad']],
  'rapidraw-164-catchup|src-tauri/src/raw_processing.rs': ['not-applicable', 'The 1.6.4 boundary (the scene-linear ceiling, our decode anchor before demosaic) is intact.', ['11e20e77', '9ba20c02', '4e45e620', 'e99082ad']],
  'adjustments-path-argument|src-tauri/src/image_loader.rs': ['not-applicable', 'The path is still threaded to the decode; the call carries it.', ['9ba20c02', '43248097', '9b7e3368']],
  'rapidraw-164-catchup|src-tauri/src/export_processing.rs': ['not-applicable', 'The headless depth setting and our TIFF encoder path, the 1.6.4 carries, are intact; the merge kept set_runtime_depth beside their cli_println!.', ['9ba20c02', 'cf6813f1', '078c90a8', 'e3022f65', 'b64adfc6', 'edb8c82c', '9a17644d', 'd151849b', 'ad25d2ba']],
  'export-precision-selector|src-tauri/src/export_processing.rs': ['not-applicable', 'The borrowed #1466 encoder arm between its markers is unchanged.', ['9ba20c02', 'cf6813f1', '078c90a8', 'e3022f65', 'b64adfc6', 'edb8c82c', '9a17644d', 'd151849b', 'ad25d2ba']],
  'sidecar-agdata|src-tauri/src/file_management.rs': ['not-applicable', 'The .agdata and .agexif lines are intact; new sidecar code in the commit goes through the same names.', ['9ba20c02', 'c4ba9ac9', '43248097', '4e45e620', '078c90a8']],
  'wb-legacy|src-tauri/src/preset_converter.rs': ['combine', 'This is why presets are converted once: the Lightroom import now writes 1.6.5 units with no whiteBalance key.', ['9ba20c02']],
  'identity|src/components/panel/MainLibrary.tsx': ['not-applicable', 'The update check and the removed donation link are untouched.', ['41bdbaad', 'c4ba9ac9']],
  'sidecar-agdata|src-tauri/src/tagging.rs': ['not-applicable', 'Tags still read and write .agdata.', ['367145a1']],
  'my-gear|src/components/panel/right/MetadataPanel.tsx': ['not-applicable', 'The camera-details marker is intact; the commit adds the flag display.', ['c4ba9ac9']],
  'sidecar-agdata|src-tauri/src/exif_processing.rs': ['not-applicable', 'The .agexif lines are intact; this edits lens reading.', ['c11c7a5c']],
  'rapidraw-164-catchup|src-tauri/src/launch_request.rs': ['combine', 'Their cli_println! and our validated --tiff-bit-depth sit side by side.', ['b64adfc6']],
  'export-precision-selector|src/hooks/useExportSettings.ts': ['keep-ours', 'Their border and pad state is taken; their TIFF depth state stays out (BorderBasis imported, TiffBitDepth not).', ['45fa2ca1', 'd151849b', 'ad25d2ba']],
  'export-precision-selector|src/components/panel/right/ExportPanel.tsx': ['combine', 'Their border and pad options are taken; their TIFF depth options stay out. The export-precision marker is intact.', ['ef6e64c4', '7cc0b674', 'e8d0d2c3', 'd151849b', 'ad25d2ba']],
  'tiff-export-metadata|src/components/panel/right/ExportPanel.tsx': ['not-applicable', 'The keep-metadata condition widened to TIFF is intact.', ['ef6e64c4', '7cc0b674', 'e8d0d2c3', 'd151849b', 'ad25d2ba']],
  'export-precision-selector|src/components/ui/ExportImportProperties.tsx': ['not-applicable', 'Border and pad fields are added to their export settings; no TIFF depth.', ['d151849b', 'ad25d2ba']],
  'export-precision-selector|src/hooks/useExternalEditSession.ts': ['not-applicable', 'External edit sessions get the border and pad fields, and still do not override the global TIFF depth.', ['d151849b', 'ad25d2ba']],
  'rapidraw-164-catchup|src/components/panel/Editor.tsx': ['not-applicable', 'The neutral-grey canvas is intact.', ['3c864270', '65472097']],
  'ai-super-resolution|src/components/panel/Editor.tsx': ['not-applicable', 'The minimum zoom bound for enlarged images is intact.', ['3c864270', '65472097']],
};

// One commit in a group that did more than the rest.
const FILE_OVERRIDE_165 = {
  'model-manager|src/components/panel/SettingsPanel.tsx|48a124f5': ['combine', 'This moves their AI card from Processing to General, so AI Models moves with it: out of the Processing tab and under their Generative AI card, placed from the existing slot by AiModelsPlacement.tsx.'],
  'no-cloud|src/components/panel/SettingsPanel.tsx|48a124f5': ['keep-ours', 'The Cloud dashboard moves to General with the AI card. It stays unreachable: the Cloud tile is hidden from our side.'],
  'no-cloud|src/components/panel/SettingsPanel.tsx|b66691ba': ['keep-ours', 'Adds the Clerk sign-in to the Cloud card. Unreachable here: the Cloud tile is hidden and the store is marked unsupported.'],
  'no-cloud|src/components/panel/SettingsPanel.tsx|06c52a49': ['keep-ours', 'Un-comments the Cloud tile. Their line stays as they wrote it; the tile is hidden from our side by NoCloudTile.tsx.'],
  'no-cloud|src/components/panel/SettingsPanel.tsx|2641891c': ['keep-ours', 'Scopes the Cloud card to desktop. Unreachable here on every platform.'],
  'no-cloud|src-tauri/src/lib.rs|2641891c': ['keep-ours', 'Puts the Clerk plugin behind cfg(desktop). Not registered here at all.'],
  'no-cloud|src-tauri/src/lib.rs|b66691ba': ['keep-ours', 'Registers the Clerk plugin with RapidRAW\'s production key and its session store. Not registered here.'],
  'no-cloud|src-tauri/Cargo.toml|2641891c': ['keep-ours', 'Moves tauri-plugin-clerk to the desktop table. Not built here.'],
  'no-cloud|src-tauri/Cargo.toml|b66691ba': ['keep-ours', 'Adds tauri-plugin-clerk, tauri-plugin-store and tauri-plugin-http. Clerk and store are not built here; http stays, unused by anything that can run.'],
  'no-cloud|src-tauri/src/inpainting.rs|b66691ba': ['keep-ours', 'Adds the cloud inpainting branch that posts to getrapidraw.com. Left in place and unreachable: it needs the Cloud provider and a Clerk token.'],
  'rapidraw-164-catchup|src-tauri/src/app_settings.rs|706026a6': ['combine', 'custom_aspect_ratios arrived beside our last_import_settings; the merge gave it the #[serde(default)] every other field has. Without it a settings.json written before this release fails to parse and the app starts on defaults.'],
};

const fileLevel165 = Object.entries(FILE_165).flatMap(([key, [verdict, clause, commits]]) => {
  const [entry, file] = key.split('|');
  return commits.map((commit) => {
    const [v, c] = FILE_OVERRIDE_165[`${key}|${commit}`] ?? [verdict, clause];
    return review165(commit, `${entry}:${file}`, v, `${DID_165[commit]} ${c}`);
  });
});

// Merge commits that change nothing of their own: [merge, what it merges, [targets]].
const MERGES_165 = [
  ['55be8227', 'Merge of #1832 (3f3e7df1, e4d6fd16, 9fe2ee29, 8155513d, 3ba2fcae)', [
    'display-transform:src-tauri/src/lib.rs#ag_display_matrix',
    'cache-keys:src-tauri/src/lib.rs#cache_version',
    'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH',
    'no-cloud:src-tauri/src/lib.rs',
  ]],
  ['f513a4ee', 'Merge of #1834 (476389ad)', [
    'import-dialogue-1714:src-tauri/src/app_settings.rs#last_import_settings',
    'rapidraw-164-catchup:src-tauri/src/app_settings.rs',
  ]],
  ['a96c0e7e', 'Merge of #1819 (b64adfc6)', [
    'display-transform:src-tauri/src/lib.rs#ag_display_matrix',
    'cache-keys:src-tauri/src/lib.rs#cache_version',
    'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH',
    'no-cloud:src-tauri/src/lib.rs',
    'adjustments-path-argument:src-tauri/src/export_processing.rs#get_all_adjustments_from_json',
    'rapidraw-164-catchup:src-tauri/src/export_processing.rs',
    'high-precision-export:src-tauri/src/export_processing.rs#process_image_for_export',
    'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image',
    'high-precision-export:src-tauri/src/export_processing.rs#apply_watermark',
    'high-precision-export:src-tauri/src/export_processing.rs#encode_image_to_bytes',
    'export-precision-selector:src-tauri/src/export_processing.rs',
    'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes',
    'tiff-export-metadata:src-tauri/src/export_processing.rs#save_image_with_metadata',
  ]],
  ['b191ae05', 'Merge of #1815 (43248097, 4e45e620)', [
    'raw-decode:src-tauri/src/raw_processing.rs#on_raw_decoded',
    'canon-old-wb:src-tauri/src/raw_processing.rs#on_raw_decoded',
    'canon-old-wb:src-tauri/src/raw_processing.rs#read_as_shot_white_balance',
    'highlight-recovery:src-tauri/src/raw_processing.rs#develop_internal',
    'borrow-1633:src-tauri/src/raw_processing.rs',
    'rapidraw-164-catchup:src-tauri/src/raw_processing.rs',
    'raw-tone:src-tauri/src/raw_processing.rs#develop_raw_image',
    'sidecar-agdata:src-tauri/src/file_management.rs',
    'import-dialogue-1714:src-tauri/src/file_management.rs#ImportSettings',
    'wb-legacy:src-tauri/src/file_management.rs#load_metadata',
  ]],
  ['c50001a8', 'Merge of #1820 (e3022f65)', [
    'adjustments-path-argument:src-tauri/src/export_processing.rs#get_all_adjustments_from_json',
    'rapidraw-164-catchup:src-tauri/src/export_processing.rs',
    'high-precision-export:src-tauri/src/export_processing.rs#process_image_for_export',
    'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image',
    'high-precision-export:src-tauri/src/export_processing.rs#apply_watermark',
    'high-precision-export:src-tauri/src/export_processing.rs#encode_image_to_bytes',
    'export-precision-selector:src-tauri/src/export_processing.rs',
    'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes',
    'tiff-export-metadata:src-tauri/src/export_processing.rs#save_image_with_metadata',
  ]],
  ['7bc85024', 'Merge of their main into a branch (3 commits)', [
    'my-gear:src/components/panel/SettingsPanel.tsx',
    'argentum-shell:src/components/panel/SettingsPanel.tsx',
    'model-manager:src/components/panel/SettingsPanel.tsx',
    'no-cloud:src/components/panel/SettingsPanel.tsx',
    'object-label:src/components/panel/SettingsPanel.tsx#settings.processing.ai.cpu.feature1',
    'compact-sliders:src/components/panel/SettingsPanel.tsx',
  ]],
  ['a1e2dda0', 'Merge of #1767 (367029d7, 7514c8f6, bbe7e986, de8d6ffe, 2f907e36, 4227dcbd, and 6 more)', [
    'import-dialogue-1714:src-tauri/src/app_settings.rs#last_import_settings',
    'rapidraw-164-catchup:src-tauri/src/app_settings.rs',
  ]],
  ['27554370', 'Merge of #1766 (ef6e64c4, e8d0d2c3, edb8c82c, 9a17644d, 0b85c91e, d151849b, and 2 more)', [
    'export-precision-selector:src/components/panel/right/ExportPanel.tsx',
    'tiff-export-metadata:src/components/panel/right/ExportPanel.tsx',
  ]],
];

const merges165 = MERGES_165.flatMap(([commit, what, targets]) =>
  targets.map((target) => review165(
    commit,
    target,
    'not-applicable',
    `${what}. It changes nothing of its own here; the work is reviewed in those commits.`,
  )),
);

// Where the diff reaches what we rely on, one at a time.
const reached165 = [
  review165('66fa1600', 'high-precision-export:src-tauri/src/gpu_processing.rs#process_and_get_dynamic_image_inner', 'combine', 'Their preview now builds guided-filter coefficients (build_guided_coeffs) before run(), which takes two more views. Our 32-bit export builds its own processor, so it does the same in export_precision::render_high_precision; their shadows, highlights, clarity and dehaze render in TIFF exports as on screen.'),
  review165('b66691ba', 'no-cloud:src/App.tsx#initAuth', 'keep-ours', 'This is where they start Clerk at every launch. Left in place: noCloud.ts marks the store unsupported first, and their initAuth returns at that state.'),
  review165('b66691ba', 'no-cloud:src/store/useCloudStore.ts#initAuth', 'keep-ours', 'Their guard returns on unsupported before Clerk is touched, which is what noCloud.ts relies on.'),
  review165('9fe2ee29', 'preview-encode:src-tauri/src/image_processing.rs#apply_cpu_default_raw_processing', 'not-applicable', 'The hunk only follows the function: srgb_channel_to_linear is added after it. apply_cpu_default_raw_processing itself is unchanged and still returns into mods::preview_encode.'),
  review165('3ba2fcae', 'import-dialogue-1714:src-tauri/src/image_processing.rs#calculate_auto_adjustments', 'not-applicable', 'The picker\'s sampling code is appended after calculate_auto_adjustments; the hunk is labelled with it but the function is unchanged, and the import auto-edit still calls it.'),
  review165('28fa5120', 'auto-white-balance:src-tauri/src/white_balance.rs#pick_white_balance', 'combine', 'Folds duplicated code into white_balance.rs, including pick_white_balance, which the wand now calls through auto_wb::removing_illuminant. Tested: what the wand writes neutralises what it detected, through their gains.'),
  review165('28fa5120', 'auto-white-balance:src-tauri/src/white_balance.rs#adaptation_log_gains', 'combine', 'The gains our tests apply to check the wand are the ones this commit consolidates.'),
  review165('28fa5120', 'camera-profile:src-tauri/src/image_processing.rs#GlobalAdjustments', 'combine', 'Their white balance fields in GlobalAdjustments are consolidated; our three profile rows are still after them and filled by rows_for.'),
  review165('28fa5120', 'adjustments-path-argument:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'combine', 'The signature keeps their as_shot argument third and our photo fifth; every call site carries both.'),
  review165('28fa5120', 'wb-legacy:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'combine', 'from_adjustments reads whiteBalance, temperature and tint here, which is what wb_legacy converts into. No hook of ours is in the function; conversion happens from our side.'),
  review165('28fa5120', 'raw-tone:src-tauri/src/image_processing.rs#GlobalAdjustments', 'not-applicable', 'The raw tone fields in GlobalAdjustments are untouched by the white balance consolidation.'),
  review165('28fa5120', 'camera-profile:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments', 'combine', 'Their white balance fields on the GPU side are consolidated; the profile rows still match the Rust struct.'),
  review165('28fa5120', 'raw-tone:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments', 'not-applicable', 'The raw tone fields on the GPU side are untouched.'),
  review165('11e20e77', 'canon-old-wb:src-tauri/src/raw_processing.rs#read_as_shot_white_balance', 'keep-ours', 'Interpolating A and D65 matrices refines their as-shot reader; it still decodes on its own and never reaches our anchor, so a 1D or 1Ds reads unity there. The fix needs a line of theirs and raw_processing.rs has no allowance left; recorded as a known issue: the Kelvin readout is off on those bodies, corrections still land neutral.'),
  review165('9ba20c02', 'auto-white-balance:src-tauri/src/white_balance.rs#pick_white_balance', 'adopt', 'Their white balance replaces ours: our engine (dt_white_balance) and picker are retired. Our auto mode stays and answers through pick_white_balance in their units.'),
  review165('9ba20c02', 'auto-white-balance:src-tauri/src/white_balance.rs#adaptation_log_gains', 'adopt', 'Their log-LMS gains are the white balance that renders now.'),
  review165('9ba20c02', 'wb-legacy:src-tauri/src/white_balance.rs#shifted', 'combine', 'The relative model (1.5 mired and 1.5 tint a step) is what old edits are converted into; tests check the converted gains against the retired shader\'s.'),
  review165('9ba20c02', 'auto-white-balance:src/utils/whiteBalance.ts#withRelativeWhiteBalance', 'combine', 'The wand writes its answer with these helpers, exactly as their picker does.'),
  review165('9ba20c02', 'auto-white-balance:src-tauri/src/app_state.rs#as_shot_white_balance', 'combine', 'The open photo\'s as-shot white balance, which the wand\'s answer is given on top of.'),
  review165('9ba20c02', 'camera-profile:src-tauri/src/image_processing.rs#GlobalAdjustments', 'combine', 'temperature and tint leave GlobalAdjustments for wb_log_gain_*; our profile rows stay. The scene-linear anchor now runs just before their apply_white_balance, so the profile still comes first.'),
  review165('9ba20c02', 'adjustments-path-argument:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'combine', 'Their as_shot argument arrives third; ours stays fifth. Resolved at the merge in every caller.'),
  review165('9ba20c02', 'wb-legacy:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'combine', 'This is where the new white balance is read; old edits are converted before it sees them, from our side.'),
  review165('9ba20c02', 'raw-tone:src-tauri/src/image_processing.rs#GlobalAdjustments', 'not-applicable', 'The raw tone fields are untouched; only the white balance fields change.'),
  review165('9ba20c02', 'camera-profile:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments', 'combine', 'The GPU struct gains wb_log_gain_* and their matrices; our profile rows still mirror the Rust struct (the shader compiles and the export tests render).'),
  review165('9ba20c02', 'raw-tone:src-tauri/src/shaders/shader.wgsl#GlobalAdjustments', 'not-applicable', 'The raw tone fields on the GPU side are untouched.'),
  review165('9ba20c02', 'wb-legacy:src/utils/adjustments.ts:whiteBalance', 'combine', 'Adds whiteBalance to INITIAL_ADJUSTMENTS, null in relative mode. Its absence is how an old edit is recognised.'),
  review165('9ba20c02', 'canon-old-wb:src-tauri/src/raw_processing.rs#read_as_shot_white_balance', 'keep-ours', 'Their as-shot reader is new here and never reaches our anchor, so on a 1D or 1Ds it reads unity. Corrections still land neutral (the picker and the wand work in ratios); the Kelvin readout is off. Known issue: the fix needs a line in raw_processing.rs, which has no allowance left.'),
  review165('9ba20c02', 'highlight-recovery:src-tauri/src/raw_processing.rs#develop_internal', 'not-applicable', 'The hunk only follows develop_internal: read_as_shot_white_balance is added after it. Nothing in develop_internal changes, so settle_blown\'s assumptions hold.'),
  review165('9ba20c02', 'highlight-recovery:src-tauri/src/image_loader.rs#load_image', 'not-applicable', 'load_image now also reads the as-shot white balance into LoadedImage. It adds no pixel cache, so redecode::open_photo resets everything it needs to.'),
  review165('9ba20c02', 'adjustments-path-argument:src-tauri/src/export_processing.rs#get_all_adjustments_from_json', 'combine', 'Export callers pass their as_shot third and our photo fifth.'),
  review165('9ba20c02', 'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image', 'not-applicable', 'Only the adjustments call gains the as-shot argument; the mask export path is otherwise unchanged.'),
  review165('9ba20c02', 'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes', 'not-applicable', 'Only the adjustments call changes; both estimate sites still go through render_for_estimate.'),
  review165('9ba20c02', 'adjustments-path-argument:src-tauri/src/lut_processing.rs#get_all_adjustments_from_json', 'combine', 'The LUT swatch call passes their as_shot and our photo.'),
  review165('43248097', 'raw-tone:src-tauri/src/image_loader.rs#embedded_preview_fallback', 'combine', 'Their fallback loses its path argument and finds the preview through extract_embedded_preview. Taken, kept crate-visible; raw_tone calls it without a path.'),
  review165('4e45e620', 'highlight-recovery:src-tauri/src/raw_processing.rs#develop_internal', 'not-applicable', 'develop_internal reads orientation through the new metadata_orientation helper; the decode, our anchor and the ceiling are unchanged.'),
  review165('4e45e620', 'raw-tone:src-tauri/src/raw_processing.rs#develop_raw_image', 'not-applicable', 'develop_raw_image\'s signature and output are unchanged; Auto-Matched still fits against it.'),
  review165('078c90a8', 'adjustments-path-argument:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'not-applicable', 'Mask sources are built from the exported image; the adjustments call is unchanged.'),
  review165('078c90a8', 'wb-legacy:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', 'not-applicable', 'The white balance read is unchanged by the mask source change.'),
  review165('078c90a8', 'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image', 'combine', 'Mask sources come from the exported image now; our 32-bit render path is unaffected and the masks it receives are better.'),
  review165('078c90a8', 'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes', 'not-applicable', 'Both estimate sites still go through render_for_estimate.'),
  review165('e3022f65', 'high-precision-export:src-tauri/src/export_processing.rs#encode_image_to_bytes', 'adopt', 'JPEG and PNG get an sRGB profile. TIFF goes through our encode_tiff and gets none, as before; their TIFF profile is a separate pull request (#1752) to port when it lands.'),
  review165('b64adfc6', 'ai-gpu-runtime:src-tauri/src/lib.rs#ORT_DYLIB_PATH', 'not-applicable', 'Only the log line changes, println! to cli_println!. ORT_DYLIB_PATH is still set after startup::init, so the DirectML pin still wins.'),
  review165('d6cda855', 'high-precision-export:src-tauri/src/gpu_processing.rs#to_rgba_f16', 'adopt', 'Their faster conversion is taken and kept crate-visible; the export\'s flare copy uses it unchanged.'),
  review165('7514c8f6', 'wb-legacy:src/utils/adjustments.ts:whiteBalance', 'not-applicable', 'Removes adjustment_visibility; whiteBalance stays in INITIAL_ADJUSTMENTS.'),
  review165('c7c42306', 'wb-legacy:src/utils/adjustments.ts:whiteBalance', 'not-applicable', 'Moves visibility into the Customise Panels submenu; whiteBalance stays in INITIAL_ADJUSTMENTS.'),
  review165('9a17644d', 'high-precision-export:src-tauri/src/export_processing.rs#process_image_for_export', 'combine', 'Shared geometry helpers replace inline resize code; they work on Rgba32F, so the 32-bit path keeps its precision.'),
  review165('9a17644d', 'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image', 'not-applicable', 'The mask export uses the same shared geometry; nothing in it narrows precision.'),
  review165('d151849b', 'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image', 'adopt', 'The border is drawn natively on Rgba32F, after our render; taken.'),
  review165('ad25d2ba', 'high-precision-export:src-tauri/src/export_processing.rs#export_masks_for_image', 'adopt', 'Padding is applied natively on Rgba32F; taken.'),
  review165('ad25d2ba', 'high-precision-export:src-tauri/src/export_processing.rs#apply_watermark', 'not-applicable', 'The watermark is placed against the padded frame; its precision handling is unchanged.'),
  review165('ad25d2ba', 'export-precision-selector:src-tauri/src/export_processing.rs#estimate_export_sizes', 'combine', 'The estimate includes border and padding; it still renders through render_for_estimate at the chosen depth.'),
  review165('9b7e3368', 'raw-decode:src-tauri/src/image_loader.rs#load_base_image_from_bytes', 'keep-ours', 'Apple RAW 9 is an opt-in macOS path (use_apple_raw9) that develops through Core Image and never reaches our decode anchor. Off by default; registered so the gap is visible.'),
  review165('9b7e3368', 'highlight-recovery:src-tauri/src/image_loader.rs#load_base_image_from_bytes', 'keep-ours', 'On the Apple RAW 9 path there is no highlight recovery or settle_blown. Off by default; rawler remains the decode that re-decodes for the switch.'),
  review165('48a124f5', 'object-label:src/components/panel/SettingsPanel.tsx#settings.processing.ai.cpu.feature1', 'not-applicable', 'Moves their AI card, this list with it, from Processing to General. The key is unchanged, so the card still lists Object, Sky, Foreground.'),
  review165('f2c3473b', 'compact-sliders:src/components/ui/Slider.tsx#w-14 text-right shrink-0', 'combine', 'Widens the value column from w-12 to w-14 and stops the value wrapping. compactSliders.css is written against this markup: it finds the column by w-14 and moves it to the third grid column.'),
];

// Subject hints: the word matched, and what is actually true.
const HINTS_165 = [
  ['48431439', ['mask-stage-size-guard'], 'not-applicable', 'AI inpainting skips mask refinement; the mask Stage is unaffected.'],
  ['48431439', ['object-label'], 'not-applicable', 'Inpainting skips the Subject mask\'s refinement; its label and key are untouched.'],
  ['37a2fcd7', ['rapidraw-164-catchup'], 'not-applicable', 'Merges the curve endpoint fix; curves are theirs and none of the 1.6.4 carries.'],
  ['1b17dd52', ['rapidraw-164-catchup'], 'not-applicable', 'Curve endpoint handling only.'],
  ['d4429f1f', ['ci-desktop-only'], 'not-applicable', 'Android CI, which we do not build.'],
  ['63472b86', ['ci-desktop-only'], 'not-applicable', 'Android CI, which we do not build.'],
  ['1e6777ed', ['ci-desktop-only'], 'not-applicable', 'Android CI, which we do not build.'],
  ['5572f96a', ['canon-old-wb', 'wb-legacy'], 'not-applicable', 'The Kelvin toggle is always shown; no change to how as-shot white balance is read or to what the numbers mean.'],
  ['aff9b3f2', ['preview-encode'], 'not-applicable', 'The culling preview uses the medium thumbnail; the encode is not involved.'],
  ['55be8227', ['auto-white-balance', 'canon-old-wb', 'rgb-readout'], 'combine', 'Merges their area picker, which replaces ours; the RGB readout and our auto mode are separate and stay.'],
  ['3f3e7df1', ['canon-old-wb'], 'not-applicable', 'Removes their white balance tests; ours stay.'],
  ['a6832200', ['rapidraw-164-catchup'], 'not-applicable', 'Curve fine adjust mode; curves are theirs.'],
  ['e4d6fd16', ['canon-old-wb'], 'not-applicable', 'The sampler returns Kelvin from their as-shot reader; the 1D/1Ds gap is recorded against 9ba20c02.'],
  ['9fe2ee29', ['auto-white-balance', 'raw-decode', 'canon-old-wb', 'display-transform', 'borrow-1633'], 'not-applicable', 'Refactors the picker\'s sRGB decode (2.4 exponent, as #1633) and outline. Not our decode, display transform or borrowed fix.'],
  ['8155513d', ['auto-white-balance', 'canon-old-wb'], 'not-applicable', 'Swatch label formatting only.'],
  ['3ba2fcae', ['canon-old-wb'], 'not-applicable', 'The picker samples the original image; as-shot reading is not touched.'],
  ['48aac8ab', ['raw-decode', 'identity', 'rapidraw-164-catchup'], 'not-applicable', 'A merge of their main into a branch; no change of its own.'],
  ['f2c3473b', ['canon-old-wb', 'wb-legacy'], 'not-applicable', 'Slider styling; the numbers mean the same.'],
  ['721939e9', ['canon-old-wb', 'wb-legacy'], 'combine', 'Merges the Kelvin white balance: reviewed through its commits (9ba20c02 and after). Old edits are converted into it; the 1D/1Ds gap is recorded.'],
  ['73bc73f4', ['canon-old-wb'], 'not-applicable', 'Clamps the LMS of an extreme warm and tint white balance; nothing about as-shot reading.'],
  ['28fa5120', ['canon-old-wb'], 'not-applicable', 'Deduplication; read_as_shot_white_balance is unchanged in behaviour.'],
  ['bcc6e1e9', ['canon-old-wb', 'my-gear', 'wb-legacy'], 'not-applicable', 'Remounts sliders on a mode switch; no change to as-shot reading, cameras or the numbers.'],
  ['5d39d8fc', ['argentum-shell'], 'combine', 'Czech: rebranded and given our strings here, like the other locales.'],
  ['924d255a', ['raw-decode', 'identity', 'rapidraw-164-catchup'], 'not-applicable', 'A merge of their main into a branch; no change of its own.'],
  ['125d77bf', ['argentum-shell'], 'not-applicable', 'Strips a BOM from nl.json; our strings were added after.'],
  ['13611ab8', ['argentum-shell'], 'combine', 'Czech review; rebranded and given our strings here.'],
  ['f98d68a4', ['display-transform'], 'not-applicable', 'A translation; "display" in the subject is about UI strings, not the screen transform.'],
  ['f513a4ee', ['export-precision-selector', 'tiff-export-metadata', 'wb-legacy'], 'keep-ours', 'Merges #1834, export preset fields: TIFF depth in presets stays declined (our depth is global), the timestamp option is taken. Reviewed through 476389ad.'],
  ['41bdbaad', ['clipping-view'], 'not-applicable', '"Clipping" here is the bottom bar clipping out of the window, not the highlight clipping view.'],
  ['61808be4', ['argentum-shell'], 'adopt', 'Registers Dutch; our strings are in nl.json.'],
  ['476389ad', ['tif-raw-sniffing', 'high-precision-export', 'export-precision-selector', 'tiff-export-metadata', 'wb-legacy'], 'keep-ours', 'Their ExportPreset gains tiff_bit_depth and preserve_timestamps. TIFF depth stays our global preference, as decided at 1.6.4, so the field is left unwired; preserve_timestamps is theirs and taken. Nothing about RAW sniffing, metadata or white balance.'],
  ['f0f1be48', ['raw-decode', 'rapidraw-164-catchup'], 'not-applicable', 'Library filtering and RAW+JPEG grouping; no decode.'],
  ['0ca6152a', ['high-precision-export'], 'adopt', 'Merges d6cda855, the faster f16 upload, reviewed there.'],
  ['875a8f4f', ['my-gear'], 'adopt', 'Merges c11c7a5c: Nikon lens from the MakerNote. Complements our Canon MakerNote reading, which runs first.'],
  ['b191ae05', ['preview-encode'], 'not-applicable', 'Merges the RAF embedded thumbnail fix; no encode.'],
  ['c50001a8', ['display-transform', 'borrow-1633'], 'adopt', 'Merges e3022f65, the sRGB ICC in exports; the screen transform and #1633 are untouched.'],
  ['cf6813f1', ['cache-keys', 'borrow-1307'], 'not-applicable', 'An export cache in export_processing.rs; cache_utils.rs and our #1307 block are untouched.'],
  ['a831f770', ['mask-stage-size-guard'], 'not-applicable', 'Merges 078c90a8, mask sources from the exported image; the mask Stage is untouched.'],
  ['597631c4', ['mask-stage-size-guard', 'display-transform'], 'not-applicable', 'Fullscreen Escape with a mask open; neither the Stage guard nor the screen transform.'],
  ['7c2346f8', ['highlight-recovery', 'rapidraw-164-catchup'], 'keep-ours', 'Merges e99082ad, an edit to their post-demosaic recovery, which we removed at 1.6.4; our pre-demosaic recovery and settle_blown stay the only recovery.'],
  ['c11c7a5c', ['canon-old-wb'], 'adopt', 'Nikon lens from the MakerNote; complements our Canon reading. Not white balance.'],
  ['43248097', ['preview-encode'], 'not-applicable', 'Thumbnail previews from the embedded JPEG; the CPU preview encode is not involved.'],
  ['4e45e620', ['preview-encode', 'tif-raw-sniffing', 'high-precision-export', 'export-precision-selector', 'tiff-export-metadata'], 'not-applicable', 'Thumbnails from the embedded preview for non-TIFF RAWs; a RAW .TIF still goes through our sniffing and the EXIF path. No export change.'],
  ['9671795e', ['mask-stage-size-guard'], 'not-applicable', 'Escape handling in the masks panel.'],
  ['078c90a8', ['mask-stage-size-guard'], 'not-applicable', 'Mask sources for export; not the Stage.'],
  ['e3022f65', ['camera-profile', 'display-transform', 'borrow-1633'], 'adopt', 'An sRGB profile in JPEG and PNG; exports are sRGB, so this labels them truthfully. Not the camera profile, the screen transform or #1633.'],
  ['0ef01514', ['argentum-shell'], 'not-applicable', 'Their strings; ours are in their own namespace and the rebrand lines are intact.'],
  ['cbc82ccf', ['argentum-shell'], 'not-applicable', 'Dutch strings; ours were added after.'],
  ['752b3163', ['argentum-shell'], 'not-applicable', 'Dutch strings; ours were added after.'],
  ['0552dad2', ['argentum-shell'], 'not-applicable', 'A Portuguese string.'],
  ['ee73e941', ['rapidraw-164-catchup'], 'not-applicable', 'Merges the curve overlays; curves are theirs.'],
  ['db893479', ['rapidraw-164-catchup'], 'not-applicable', 'Reverts curve axis strips.'],
  ['6d348a0d', ['import-dialogue-1714'], 'not-applicable', 'A Button import in the crop panel; "import" is the code word.'],
  ['454c33d6', ['import-dialogue-1714', 'export-precision-selector', 'wb-legacy'], 'not-applicable', 'Crop ratio presets; not import, export or white balance presets.'],
  ['143d7262', ['rapidraw-164-catchup'], 'not-applicable', 'Type fixes in their curve graph; curves are theirs, not a 1.6.4 carry.'],
  ['9b822ef1', ['rapidraw-164-catchup'], 'not-applicable', 'Shared channel and gradient helpers in their curve code; not a 1.6.4 carry.'],
  ['126c2400', ['rapidraw-164-catchup'], 'not-applicable', 'Curve axis strips (reverted in db893479).'],
  ['b5802863', ['rapidraw-164-catchup'], 'not-applicable', 'Coloured markers in their curve graph; not a 1.6.4 carry.'],
  ['c8a368e7', ['rapidraw-164-catchup'], 'not-applicable', 'Coloured overlays in their curve graph; not a 1.6.4 carry.'],
  ['45fa2ca1', ['tiff-export-metadata'], 'not-applicable', 'Default pad and border colour; no metadata.'],
  ['7cc0b674', ['argentum-shell'], 'not-applicable', 'A cancel button and its strings.'],
  ['0b85c91e', ['argentum-shell', 'tiff-export-metadata'], 'not-applicable', 'Border strings; no metadata.'],
  ['b88424ed', ['argentum-shell', 'tiff-export-metadata'], 'not-applicable', 'Pad strings; no metadata.'],
  ['1cbb313c', ['highlight-recovery'], 'not-applicable', '"Recovery" matched nothing of ours: this is library scroll offset.'],
];

const hints165 = HINTS_165.flatMap(([commit, entries, verdict, why]) =>
  entries.map((entry) => review165(commit, entry, verdict, why, 'feature')));

export const REVIEW_165_DECISIONS = [
  ...unreached165,
  ...fileLevel165,
  ...merges165,
  ...reached165,
  ...hints165,
];

// 818 at the merge, plus 14 for object-label and 21 for compact-sliders,
// registered after it (26.41.7).
if (REVIEW_165_DECISIONS.length !== 853) {
  throw new Error(`RapidRAW 1.6.5 review should account for 853 decisions, found ${REVIEW_165_DECISIONS.length}`);
}

const REVIEW_165_FEATURES = {
  verdict: 'overlap-found',
  why:
    'Read all 149 commits for features we already have. One duplicates ours: the white balance '
    + '(9ba20c02, 11e20e77, 28fa5120, 73bc73f4, 8aa523bc, the picker series 3ba2fcae..e4d6fd16). '
    + 'Theirs adopted, ours retired: their engine does what dt_white_balance did and more (Kelvin '
    + 'from the camera\'s as-shot data, white balance per mask, an area picker on the original), and '
    + 'ours turned out to decode the RAW twice and clip highlights above white whenever a slider '
    + 'was off zero. Our auto mode has no counterpart upstream and stays, answering through their '
    + 'pick_white_balance; old edits are converted (wb-legacy). Close to ours but not duplicates: '
    + 'their guided-filter shadows and highlights (1cc99d56, 66fa1600) are a slider on the GPU, our '
    + 'highlight recovery rebuilds clipped channels at decode; their recover_clipped_pixel edit '
    + '(e99082ad) is to the post-demosaic recovery we removed at 1.6.4; their TIFF depth in presets '
    + '(476389ad) re-offers what we declined at 1.6.4; Nikon MakerNote lenses (c11c7a5c) complement '
    + 'our Canon reading; embedded-preview thumbnails (4e45e620) leave our .TIF sniffing alone. '
    + 'Interface: their Kelvin mode, area picker, reorderable and collapsible sections, Tool Focus, '
    + 'pick/reject flags, export border and padding and AI-Free mode are taken as they are; our '
    + 'markers are re-seated in the new Color layout, AI Models follows their AI card to General, '
    + 'and Enlarge hides in AI-Free mode. RapidRAW Cloud (b66691ba, 2641891c, 06c52a49) is '
    + 'removed (no-cloud). Not covered by this window: the three commits after 1.6.5 (relight, '
    + 'fog, staged preview caches), which are for the next review.',
};

export const REVIEWS = [
  {
    through: 'ef25ba2af99b6b0da1c568f51ccbb9040162bfc6',
    date: '2026-09-13',
    decisions: [],
    note:
      'Seed entry. This is where the fork stood when overlap review was built, so '
      + 'there is no earlier entry to derive a range from and nothing is claimed '
      + 'about the ten commits merged before it. The first real review is the next '
      + 'one, and it starts here.',
  },
  {
    through: '5ad3ba0b000186c6c2ce4637530c6cdbe94c7cad',
    date: '2026-09-13',
    decisions: [
      { overlap: '8737fc4e:dep:display-transform:src-tauri/src/lib.rs#ag_display_matrix', verdict: 'not-applicable', why: 'Upstream changes compute_full_transformed_res and compute_patched_and_warped, not the display matrix rows or monitor refresh hook. Both Argentum display hooks survive unchanged.' },
      { overlap: '8737fc4e:dep:cache-keys:src-tauri/src/lib.rs#cache_version', verdict: 'not-applicable', why: 'This file overlap is mechanical: upstream changes spatial transform caching, while Argentum retains its own cache version stamp and key integration.' },
      { overlap: '8737fc4e:dep:cache-keys:src-tauri/src/cache_utils.rs', verdict: 'combine', why: 'Adopt calculate_patched_warped_hash, including lens blur inputs, and move orientationSteps from the geometry key to the thumbnail base key. Geometry is computed before orientation. Argentum content hashing for the lens blur depth map and AI patches remains in calculate_transform_hash.' },
      { overlap: '8737fc4e:dep:borrow-1307:src-tauri/src/cache_utils.rs', verdict: 'not-applicable', why: 'Upstream touches the surrounding cache module but does not replace the existing marked pull request 1307 correction, which remains intact.' },
      { overlap: '97cc7d5b:dep:display-transform:src-tauri/src/lib.rs#ag_display_matrix', verdict: 'not-applicable', why: 'This file overlap is mechanical: upstream adds crop transform caching, while Argentum retains its display matrix rows and monitor refresh hook.' },
      { overlap: '97cc7d5b:dep:cache-keys:src-tauri/src/lib.rs#cache_version', verdict: 'not-applicable', why: 'Upstream adds a patched/warped intermediate cache. It does not alter Argentum startup thumbnail invalidation or the pipeline stamp; RAW decoding and colour processing are unchanged in this batch.' },
      { overlap: '97cc7d5b:dep:adjustments-path-argument:src-tauri/src/image_loader.rs', verdict: 'not-applicable', why: 'Upstream clears patched_warped_cache when loading a photo. It does not change the adjustment-loading calls that carry Argentum photo path arguments.' },
      { overlap: '97cc7d5b:dep:cache-keys:src-tauri/src/cache_utils.rs', verdict: 'combine', why: 'Adopt clearing the new patched_warped_cache in clear_image_caches. Existing Argentum content hashing for lens blur depth maps and AI patches is unchanged.' },
      { overlap: '97cc7d5b:dep:borrow-1307:src-tauri/src/cache_utils.rs', verdict: 'not-applicable', why: 'Upstream touches the surrounding cache module but does not replace the existing marked pull request 1307 correction, which remains intact.' },
      { overlap: '97cc7d5b:feature:preview-encode', verdict: 'not-applicable', why: 'The crop pan and zoom commit changes transform caching and editor gestures, with no preview encode implementation or behavior duplicated.' },
    ],
    featureReview: {
      verdict: 'none',
      why: 'Read all three commits for processing and UI/UX overlap. Adopt upstream Ctrl/Meta crop pan, wheel crop zoom and double-click crop/rotation reset, including their interface. Canvas gestures are separate from Argentum Ctrl-drag clipping previews on sliders. Adopt the intermediate preview cache and follow-up spatial-order/blur-key fix; keep Argentum processing and borrowed fixes. No white balance, highlight recovery, display conversion or preview encoding implementation is replaced. The newer highlight commit 40cfa3df is outside this review. Interactive masking/picker checks remain part of candidate validation, not a claim made by this source review.',
    },
  },
  {
    through: '40cfa3df9c039f9f6adfd77a2c759b1ae788a3cd',
    date: '2026-09-19',
    decisions: [
      { overlap: '40cfa3df:dep:camera-profile:src-tauri/src/image_processing.rs#GlobalAdjustments', verdict: 'not-applicable', why: 'The upstream commit changes RAW highlight processing in this file, not Argentum camera-profile fields or their scene-linear anchor.' },
      { overlap: '40cfa3df:dep:clipping-view:src-tauri/src/image_processing.rs#show_clipping', verdict: 'not-applicable', why: 'The upstream commit does not change Argentum clipping-view state; the overlap is surrounding-file context only.' },
      { overlap: '40cfa3df:dep:preview-encode:src-tauri/src/image_processing.rs#apply_cpu_default_raw_processing', verdict: 'not-applicable', why: 'The upstream commit changes remove_raw_artifacts_and_enhance and detail enhancement, not Argentum preview encoding, which remains the output boundary.' },
      { overlap: '40cfa3df:dep:adjustments-path-argument:src-tauri/src/image_processing.rs#get_all_adjustments_from_json', verdict: 'not-applicable', why: 'The upstream commit does not change adjustment parsing or the photo-path argument required by Argentum profile correction.' },
      { overlap: '40cfa3df:dep:raw-decode:src-tauri/src/raw_processing.rs#on_raw_decoded', verdict: 'not-applicable', why: 'The upstream post-demosaic recovery is intentionally excluded; Argentum keeps its single pre-demosaic decode anchor unchanged.' },
      { overlap: '40cfa3df:dep:canon-old-wb:src-tauri/src/raw_processing.rs#on_raw_decoded', verdict: 'not-applicable', why: 'The upstream post-demosaic recovery is intentionally excluded; Argentum keeps the Canon old-WB ordering unchanged.' },
      { overlap: '40cfa3df:dep:borrow-1633:src-tauri/src/raw_processing.rs', verdict: 'not-applicable', why: 'The upstream hunk is separate from Argentum\'s retained sRGB exponent correction; the marked borrowed fix remains unchanged.' },
      { overlap: '40cfa3df:dep:import-dialogue-1714:src-tauri/src/image_processing.rs#calculate_auto_adjustments', verdict: 'not-applicable', why: 'The highlight-recovery commit changes scene-linear clamping and RAW recovery, not calculate_auto_adjustments. The import feature continues to call Argentum\'s existing auto-edit and lens-resolution path.' },
      { overlap: '40cfa3df:feature:highlight-recovery', verdict: 'combine', why: 'Adopt the upstream unbounded scene-linear handling in image_processing.rs, keep Argentum\'s pre-demosaic recovery as the sole RAW recovery, and exclude RapidRAW\'s overlapping post-demosaic recovery and rawler lockfile revision.' },
    ],
    featureReview: {
      verdict: 'overlap-found',
      why: 'The commit contains two separable concerns. Its image_processing upper-clamp removal is compatible with Argentum\'s scene-linear pipeline and is adopted. Its raw_processing RGB recovery runs after demosaicing and would stack on Argentum\'s measured pre-demosaic recovery, so it is explicitly excluded. The rawler lockfile revision is also excluded pending an independent decoder review. Numerical tests cover headroom preservation, lower-floor behavior and nonzero above-white detail changes.',
    },
  },
  {
    through: '71a07921',
    date: '2026-09-22',
    decisions: REVIEW_164_DECISIONS,
    featureReview: {
      verdict: 'overlap-found',
      why: 'Read the upstream range through v1.6.4, including the final Brightness sequence, separate RAW decoder and clipping-boundary changes, full TIFF PR chain, final Android workflow, and UI/shader commits. Adopt the neutral-grey canvas, persistent Quick Filter, Vibrance and RGB curves; retain Argentum highlight recovery and 32-bit-float TIFF output. Keep TIFF depth global, add validated headless 8/16-bit selection, and preserve Argentum identity. Automated validation is recorded separately from the still-unavailable exact R6 III CR3/DPP visual A/B.',
    },
  },
  {
    through: '79c2a46b448b9a0bee0b30f8c345daad003ad694',
    date: '2026-10-07',
    decisions: REVIEW_165_DECISIONS,
    featureReview: REVIEW_165_FEATURES,
  },
];

/** The commit every upstream change up to which has been reviewed. */
export const reviewedThrough = () => REVIEWS[REVIEWS.length - 1].through;

/** The entry being added, and the range it must account for. */
export const newestRange = () => ({
  from: REVIEWS.length > 1 ? REVIEWS[REVIEWS.length - 2].through : null,
  to: reviewedThrough(),
  entry: REVIEWS[REVIEWS.length - 1],
});
