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

export const REVIEW_164_DECISIONS = [
  ...review164ShaderDecisions,
  ...review164OtherDecisions,
  ...review164FeatureDecisions,
  ...review164CatchupDecisions,
  ...review164PresetDecisions,
  ...review164CompositionDecisions,
];

if (REVIEW_164_DECISIONS.length !== 219) {
  throw new Error(`RapidRAW 1.6.4 review should account for 219 overlaps, found ${REVIEW_164_DECISIONS.length}`);
}

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
];

/** The commit every upstream change up to which has been reviewed. */
export const reviewedThrough = () => REVIEWS[REVIEWS.length - 1].through;

/** The entry being added, and the range it must account for. */
export const newestRange = () => ({
  from: REVIEWS.length > 1 ? REVIEWS[REVIEWS.length - 2].through : null,
  to: reviewedThrough(),
  entry: REVIEWS[REVIEWS.length - 1],
});
