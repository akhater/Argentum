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
    id: 'mask-stage-size-guard',
    kind: 'behaviour-change',
    what: 'Do not mount the mask canvas at zero size when the editor is hidden.',
    ours: [],
    dependsOn: [
      { file: 'src/components/panel/editor/ImageCanvas.tsx', how: 'extends',
        note: 'Two positive-dimension checks on the mask Stage render condition. Retire when upstream prevents zero-sized Stage drawing.' },
      { file: 'src/App.tsx', how: 'extends',
        note: 'Lifecycle dependency: the editor remains mounted but is hidden on return to the library; no extra App change for this guard.' },
    ],
    tests: ['Manual: open photo 6577, show a mask, back to library, reopen and repeat from Crop; user confirmed stable on 2026-09-13.'],
    keywords: /mask|canvas|stage|back.?arrow|editor|navigation/i,
  },
  {
    id: 'auto-white-balance',
    kind: 'feature',
    what:
      'Auto white balance: darktable\'s illuminant detection, answered in RapidRAW 1.6.5\'s '
      + 'white balance units. Until 1.6.5 the white balance engine and the picker were ours too; '
      + 'theirs replaced both (review 79c2a46b). Auto was a wand button until 26.41.8 and is now an '
      + 'entry in the white balance menu (white-balance-presets).',
    ours: [
      'src/argentum/WhiteBalanceMenu.tsx',
      'src-tauri/src/mods/auto_wb.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/white_balance.rs', symbol: 'pick_white_balance', how: 'calls',
        note: 'removing_illuminant hands the detected illuminant to it, the door their picker uses, '
          + 'so Auto and the picker answer in the same units. If it changes what "current" means, '
          + 'or stops returning an absolute white balance, Auto is wrong by exactly that.' },
      { file: 'src-tauri/src/white_balance.rs', symbol: 'adaptation_log_gains', how: 'calls',
        note: 'Only in tests: the answer is checked by applying it the way their shader does.' },
      { file: 'src/utils/whiteBalance.ts', symbol: 'withRelativeWhiteBalance', how: 'calls',
        note: 'Auto writes its answer with their helpers (withKelvinWhiteBalance, '
          + 'toRelativeWhiteBalance, getWhiteBalanceMode), exactly as their picker in ImageCanvas does.' },
      { file: 'src-tauri/src/app_state.rs', symbol: 'as_shot_white_balance', how: 'calls',
        note: 'The as-shot white balance of the open photo, which the answer is given on top of.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'data-argentum="color-tools" mount point, in their white balance actions row beside '
          + 'the K and picker buttons. If the white balance tool is hidden, Auto goes with it.' },
    ],
    tests: ['src-tauri/src/mods/auto_wb.rs #[cfg(test)]'],
    keywords: /white.?balance|temperature|tint|auto.?wb|grey.?world|gray.?world|eyedropper|wb.?picker/i,
  },
  {
    id: 'white-balance-presets',
    kind: 'feature',
    what:
      'Lightroom\'s white balance menu: As Shot, Auto, Daylight, Cloudy, Shade, Tungsten, Fluorescent, '
      + 'Flash, and Custom once a slider has moved. It took the auto white balance wand\'s place.',
    ours: ['src/argentum/WhiteBalanceMenu.tsx'],
    dependsOn: [
      { file: 'src-tauri/src/white_balance.rs', symbol: 'TINT_SCALE', how: 'calls',
        note: 'The presets are Adobe Camera Raw\'s numbers, and mean what Lightroom means only while '
          + 'their kelvin and tint are the DNG SDK\'s: Robertson isotherms, a tint scale of -3000. '
          + 'Change either and every preset lands somewhere else.' },
      { file: 'src/utils/whiteBalance.ts', symbol: 'withKelvinWhiteBalance', how: 'calls',
        note: 'Presets are written with it, as an absolute kelvin in either slider mode; As Shot with '
          + 'withRelativeWhiteBalance at zero. The menu\'s label comes from resolveWhiteBalance, so if '
          + 'what an edit resolves to changes, the menu names the wrong entry.' },
      { file: 'src/utils/adjustments.ts', symbol: 'normalizeLoadedAdjustments', how: 'calls',
        note: 'Auto\'s answer is kept in the edit as whiteBalanceAuto, a key their type does not declare. '
          + 'It survives a reload only because their loader spreads the saved edit over the defaults. '
          + 'If the loader starts picking keys, the menu says Custom where it said Auto; nothing renders differently.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'The data-argentum="color-tools" mount point in their white balance actions row, beside '
          + 'the K and picker buttons. Shared with auto-white-balance. The menu is placed from it, '
          + 'not in it.' },
      { file: 'src/components/adjustments/AdjustmentSubSection.tsx', how: 'calls',
        note: 'The menu is a row of ours put first in the section\'s folding body, found from the '
          + 'marker by their markup: up to the header row (.cursor-pointer), across to the element after '
          + 'it, into its first child, which must hold a range input. Reshape that and the menu falls '
          + 'back into the header slot, where it crowds the title on a narrow panel.' },
    ],
    tests: [
      'Manual: on a RAW, each preset shows its kelvin on the Temperature slider in K mode and its name '
        + 'on the button; dragging a slider turns the name to Custom, undo brings it back; Auto says Auto '
        + 'and still does after reopening the photo; As Shot returns both sliders to the camera\'s.',
    ],
    keywords: /white.?balance|wb.?preset|daylight|cloudy|tungsten|fluorescent|as.?shot/i,
  },
  {
    id: 'camera-profile',
    kind: 'feature',
    what: 'DCP camera profiles, and the matrix correction derived from them.',
    ours: [
      'src/argentum/CameraProfile.tsx',
      'src/argentum/RawSection.tsx',
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
        note: 'data-argentum="camera-profile" mount point, first in their Color panel and outside '
          + 'their tool sections. RawSection.tsx renders the RAW card there, holding the profile, Raw '
          + 'Tone Rendering and Highlight Recovery: it cannot be hidden or reordered on its own, and '
          + 'hiding the whole Color section hides it.' },
      { file: 'src/components/adjustments/AdjustmentSubSection.tsx', how: 'calls',
        note: 'The RAW card is their section component, so it looks and folds like White Balance. '
          + 'Its fold state is kept in their adjustmentLayout.collapsedTools under argentumRaw, an id '
          + 'none of their tools has. Change its props, or start pruning unknown ids, and the card '
          + 'stops building or forgets being folded. Their focus mode folds siblings from '
          + 'ADJUSTMENT_SECTION_TOOLS, which ours is not in, so RawSection.tsx mirrors it by watching '
          + 'collapsedTools with enableToolFocusMode and getAdjustmentSectionToolIds for color. If '
          + 'focus mode changes what it folds, the RAW card no longer matches.' },
      { file: 'src/utils/adjustments.ts', symbol: 'getAdjustmentSectionToolIds', how: 'calls',
        note: 'The Color tools the RAW card folds in focus mode, and folds itself for. A tool added '
          + 'to their color list is picked up; one moved out of it is not.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_scene_linear', how: 'calls',
        note: 'The scene-linear anchor, placed immediately before their apply_white_balance so the '
          + 'profile decides what the colours are before their white balance decides the light. '
          + 'Their picker and the as-shot white balance are measured without the profile, so with a '
          + 'profile chosen a picked white can land slightly off neutral.' },
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
      { file: 'src-tauri/src/image_loader.rs', symbol: 'load_base_image_from_bytes', how: 'calls',
        note: 'Reaches the anchor through develop_raw_image, except on their Apple RAW 9 path '
          + '(1.6.5, macOS, off unless use_apple_raw9 is set), which develops through Core Image and '
          + 'never decodes with rawler: no sRAW levels, highlight recovery or camera profile there.' },
    ],
    tests: [
      'src-tauri/src/mods/decode.rs #[cfg(test)]',
      'src-tauri/src/mods/sraw_levels.rs #[cfg(test)]',
    ],
    keywords: /raw|decode|demosaic|rawler|black.?level|white.?level|sraw/i,
  },
  {
    id: 'canon-old-wb',
    kind: 'feature',
    what: 'As-shot white balance for Canon bodies older than ColorData (1D, 1Ds).',
    ours: [
      'src-tauri/src/mods/canon_makernote.rs',
      'src-tauri/src/mods/canon_old_wb.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'on_raw_decoded', how: 'calls',
        note: 'Runs behind the decode anchor; adds no line of theirs of its own.' },
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'read_as_shot_white_balance', how: 'calls',
        note: 'Their as-shot reader decodes on its own and never reaches the anchor, so it calls '
          + 'canon_old_wb::fixed itself, one line. Without it a 1D or 1Ds reads as unity there, '
          + 'and their Kelvin mode and per-mask white balance start from the wrong place.' },
    ],
    tests: [
      'src-tauri/src/mods/canon_makernote.rs #[cfg(test)]',
      'src-tauri/src/mods/canon_old_wb.rs #[cfg(test)]',
    ],
    // Not expressible as a dependency, because it is not their code: rawler's
    // Cr2Decoder::get_wb looks for MakerNote 0x00a4 in the root IFD chain and
    // so never finds it. If that is ever fixed upstream this becomes a no-op
    // rather than a conflict — it only runs when wb_coeffs came back NaN — but
    // it should then be retired rather than left lying around.
    keywords: /white.?balance|wb|canon|1ds|1d|makernote|colordata|rawler|greenish/i,
  },
  {
    id: 'tif-raw-sniffing',
    kind: 'behaviour-change',
    what: 'A .TIF that a raw decoder can open is treated as a raw, not as a picture.',
    ours: ['src-tauri/src/mods/tif_raw.rs'],
    dependsOn: [
      { file: 'src-tauri/src/formats.rs', symbol: 'is_raw_file', how: 'extends',
        note: 'One disjunct on their extension test, on an anchor taken 2026-09-15 from a budget of zero. They keep the fast path and decide everything else; ours only ever says yes to a .tif rawler will open. Retire if upstream adds content sniffing of its own.' },
      { file: 'src-tauri/src/formats.rs', symbol: 'NON_RAW_EXTENSIONS', how: 'extends',
        note: 'tif and tiff stay on their list and we do not move them - an ordinary TIFF must keep loading as a picture. We only override the answer per file. If upstream ever moves tif to RAW_EXTENSIONS, every ordinary TIFF goes to the raw decoder and this entry is the place that says so.' },
    ],
    tests: ['src-tauri/src/mods/tif_raw.rs #[cfg(test)]'],
    keywords: /\.tif|tiff|is_raw_file|RAW_EXTENSIONS|raw.?detect|sniff|1ds/i,
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
      'src-tauri/src/mods/lens_name.rs',
      'src-tauri/src/mods/makernote_lens.rs',
    ],
    dependsOn: [
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'replaces',
        note: '207 lines of their lens UI removed rather than hidden. This is the uncomfortable one: upstream edits this file often and any change inside the block we deleted is a conflict resolved by hand.' },
      { file: 'src-tauri/src/lens_correction.rs', symbol: 'match_for_camera', how: 'calls' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'read_exif_data', how: 'calls',
        note: 'One call, with_lens around their read_exif_data_from_bytes: the point every format '
          + 'passes on a fresh read, before the result is cached. It used to sit inside '
          + 'extract_metadata, which only RAWs kamadak opens ever reached, so CR3 went unread. If '
          + 'upstream starts reading LensModel from CR3, RAF, ORF or RW2 itself, ours goes quiet: '
          + 'it only fills a blank one. EXIF cached before the fix is read by useAutoDetectOnLoad.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'update_exif_fields', how: 'calls',
        note: 'recover_lens_name writes a recovered lens into EXIF cached before the fix, through '
          + 'the command their metadata edits use. Needed because their resolve_lens_params_in_adjustments '
          + 're-detects the lens from cached EXIF on every save and deletes it when none is there. If '
          + 'update_exif_fields stops writing the sidecar, a CR3 opened before the fix loses its lens '
          + 'again on its first save.' },
      { file: 'src/components/panel/right/MetadataPanel.tsx', how: 'calls',
        note: 'data-argentum="camera-details" mount point.' },
    ],
    tests: [
      'src-tauri/src/mods/lens_name.rs #[cfg(test)] - and every_file_in_a_folder (ignored) runs real files through to the lensfun match',
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
      'src/argentum/photoBox.ts',
      'src-tauri/src/mods/colour_compare.rs',
    ],
    dependsOn: [
      { file: 'src/components/panel/editor/EditorToolbar.tsx', how: 'calls',
        note: 'The readout button sits in the toolbar row, found from their data-bench-id="undo" '
          + 'button, which they tag for their own benchmarks. It used to share the color-tools '
          + 'mount in Color.tsx; this note said so long after it moved.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', how: 'calls',
        note:
          'photoBox.ts finds the photo on screen as the overlay svg their canvas sizes in px to the '
          + 'drawn image, inside the pan/zoom transform. If that svg goes, or is sized in percent, the '
          + 'readout goes silent with no error. Registered under object-brush until it was retired.' },
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
      'src-tauri/src/mods/redecode.rs',
      'src-tauri/src/mods/sigmoid.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/image_loader.rs', symbol: 'load_image', how: 'shadows',
        note: 'redecode::open_photo does for the open photo what load_image does for a new one, in the other order: decode to one side, swap, then drop the pixel caches. If upstream adds a pixel-derived cache to the list load_image resets, add it to open_photo too, or the switch renders from stale pixels.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'load_base_image_from_bytes', how: 'calls',
        note: 'The same decode load_image runs, so a re-decoded photo is identical to a freshly opened one.' },
      { file: 'src-tauri/src/app_state.rs', how: 'calls',
        note: 'open_photo reads original_image and load_image_generation and resets cached_preview, gpu_image_cache and the warped/transformed caches. A renamed or new field here has to be reflected there.' },
      { file: 'src/hooks/useImageProcessing.ts', how: 'calls',
        note: 'HighlightRecovery.tsx re-renders by setting an equal adjustments object; the render effect fires on identity. If that effect stops depending on adjustments identity, the switch decodes but does not repaint.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_scene_linear', how: 'calls',
        note: 'The scene-linear anchor, now holding only the camera profile and placed before their '
          + 'white balance. Recovery itself runs at decode; nothing of it is in this stage.' },
      { file: 'src-tauri/src/multi_exposure.rs', symbol: 'neutralize_wb_if_multiexposure', how: 'calls',
        note: 'settle_blown makes blown blocks neutral under the white balance rawler will apply, and this is where develop_internal swaps it for unity on multi-exposure CR2s. If upstream changes that rule, blown highlights in those files take a cast.' },
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'develop_internal', how: 'calls',
        note: 'settle_blown assumes rawler only clips negatives and that nothing after decode desaturates above white. Upstream 85bf424a removed the compression pass and 40cfa3df replaced it post-demosaic; Argentum declined that replacement, and blown highlights went magenta until settle_blown took over. Any upstream change to highlight handling here has to be read against it.' },
    ],
    tests: [
      'src-tauri/src/mods/highlights.rs #[cfg(test)]',
      'src-tauri/src/mods/redecode.rs #[cfg(test)]',
      'src-tauri/src/mods/sigmoid.rs #[cfg(test)]',
    ],
    keywords: /highlight|recover|clip.*reconstruct|blown|rolloff|roll.?off|magenta|pink/i,
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
    id: 'lut-library',
    kind: 'feature',
    what: 'Persistent logical LUT libraries with collapsible groups and safe reassignment.',
    ours: ['src-tauri/src/lut_processing.rs'],
    dependsOn: [
      { file: 'src/components/ui/LUTControl.tsx', how: 'extends',
        note: 'Adds the library selector, collapsible library groups, and move/rename/delete controls around their LUT surface. The LUT files stay in app storage; this UI only manages the logical manifest.' },
    ],
    tests: ['Manual: import LUTs, create/rename/delete a library, move a LUT, restart, and verify the grouping and existing .agdata application survive.'],
    keywords: /lut|lookup.?table|colour.?grade|color.?grade/i,
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
    id: 'import-dialogue-1714',
    kind: 'feature',
    what:
      'Import remembers its options, can apply automatic edits or a preset, '
      + 'captures embedded XMP metadata, and removes associated sidecars cleanly.',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/app_settings.rs', symbol: 'last_import_settings', how: 'extends',
        note: 'Stores the import dialog choices between sessions.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'ImportSettings', how: 'extends',
        note: 'Carries the auto-edit/preset request, embedded XMP capture, import-sidecar copy, and associated-file deletion through the Argentum .agdata/.agexif naming.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'calculate_auto_adjustments', how: 'extends',
        note: 'The import auto-edit uses the same automatic analysis and lens-correction result as the editor action.' },
      { file: 'src/components/modals/AppModals.tsx', symbol: 'ImportSettingsModal', how: 'extends',
        note: 'Passes the persisted choices into the existing import dialog.' },
      { file: 'src/components/modals/ImportSettingsModal.tsx', how: 'extends',
        note: 'Adds the Edits on Import controls and preset selection.' },
      { file: 'src/components/ui/AppProperties.tsx', symbol: 'ImportSettings', how: 'extends',
        note: 'Keeps the frontend command and persisted-settings shapes aligned with Rust.' },
      { file: 'src/hooks/useFileOperations.ts', symbol: 'handleStartImport', how: 'extends',
        note: 'Persists the import choices and refreshes folder counts after deletion.' },
      { pattern: /^src\/i18n\/locales\/(de|en)\.json$/, how: 'extends',
        note: 'Adds translations for the import-edit controls.' },
    ],
    tests: [
      'src-tauri/src/file_management.rs #[cfg(test)] for embedded XMP packet parsing',
      'Manual: reopen import dialog and verify options persist; import with auto edits and a preset; verify embedded rating/label/tags and .xmp/.agexif cleanup.',
    ],
    keywords: /import|embedded.?xmp|xmp|preset|auto.?adjust|rating|color.?label|sidecar|delete/i,
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
        note: 'data-argentum slot for the about, gear and general categories. General holds the AI '
          + 'Models card, placed from our side under their Generative AI card.' },
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
      { file: 'package-lock.json', how: 'rebrands',
        note: 'Its package name follows package.json, which npm rewrites on every install.' },
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
    what: 'No Android build, and the full matrix runs on release rather than on every push or pull request.',
    ours: ['.github/workflows/upstream.yml'],
    dependsOn: [
      { file: '.github/workflows/ci.yml', how: 'replaces' },
      { file: '.github/workflows/pr-ci.yml', how: 'replaces',
        note: 'The Android matrix entry, and the pull_request trigger. Twelve desktop '
          + 'targets compiled for a docs-only change is a queue, not a check: the one '
          + 'that decides whether a change is safe is upstream.yml, and lint.yml is '
          + 'what catches the cheap mistakes. Run by hand when the build itself changes.' },
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
  {
    id: 'rapidraw-164-catchup',
    kind: 'behaviour-change',
    what: 'Carry the reviewed RapidRAW 1.6.4 updates while preserving Argentum-specific rendering, identity, and export behaviour.',
    ours: [],
    dependsOn: [
      { file: '.github/workflows/build.yml', how: 'extends',
        note: 'The final Android build setup requests platform-tools; keep the v1.6.4 workflow fix.' },
      { file: 'data/io.github.CyberTimon.RapidRAW.metainfo.xml', how: 'shadows',
        note: 'Retained because Argentum’s existing Flatpak manifest still installs this path. Upstream deletes it; removing it would break packaging. The pre-existing RapidRAW app label is left for a separate full packaging-identity migration.' },
      { file: 'src-tauri/src/app_settings.rs', how: 'extends',
        note: 'Carries the editorNeutralGreyBg setting, with a migration test ensuring legacy highlight-compression values still round-trip.' },
      { file: 'src-tauri/src/launch_request.rs', how: 'extends',
        note: 'Adds validated headless TIFF depth (8/16, default 16) as a plain byte value; it must not import Argentum modules.' },
      { file: 'src-tauri/src/export_processing.rs', how: 'extends',
        note: 'Retains Argentum’s TIFF pipeline and sets the process-local depth for headless export through the existing single mods import block.' },
      { file: 'src-tauri/src/raw_processing.rs', how: 'extends',
        note: 'Keeps Argentum’s pre-demosaic recovery hook and applies the reviewed full-quality headroom policy without stacking RapidRAW recovery.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', how: 'extends',
        note: 'Adopts the final post-tone-map Brightness implementation, Vibrance and RGB curves while retaining Argentum’s 32-bit TIFF target and dither control.' },
      { file: 'src-tauri/src/gpu_processing.rs', how: 'extends',
        note: 'Keeps Argentum’s high-precision TIFF rendering path and removes the redundant readback boolean.' },
      { file: 'src/components/adjustments/Basic.tsx', how: 'extends',
        note: 'Adopts the final Brightness label and control placement from the reviewed v1.6.4 UI.' },
      { file: 'src/components/panel/BottomBar.tsx', how: 'extends',
        note: 'Persists Quick Filter visibility through Argentum’s UI store rather than a component-local flag.' },
      { file: 'src/components/panel/Editor.tsx', how: 'extends',
        note: 'Adds the optional neutral-grey editor canvas without changing the app theme.' },
      { file: 'src/components/panel/right/FolderTree.tsx', how: 'extends',
        note: 'Applies the reviewed fixed search-field height.' },
      { file: 'src/components/ui/AppProperties.tsx', how: 'extends',
        note: 'Adds the neutral-canvas setting and persistent Quick Filter visibility to the UI types.' },
      { file: 'src/store/useUIStore.ts', how: 'extends',
        note: 'Stores Quick Filter visibility with UI state and initializes it safely on workspace changes.' },
      { file: 'src/i18n/update_translations.py', how: 'extends',
        note: 'Carries the upstream translation extraction update.' },
      { pattern: /^src\/i18n\/locales\/[\w-]+\.json$/, how: 'extends',
        note: 'The 13 shipped locales receive the Brightness, TIFF-depth and neutral-grey-canvas labels.' },
    ],
    tests: [
      'src-tauri/src/launch_request.rs #[cfg(test)]',
      'src-tauri/src/app_settings.rs #[cfg(test)]',
      'scripts/test-upstream-checks.mjs and scripts/test-upstream-e2e.mjs',
    ],
    keywords: /brightness|tone.?map|vibrance|curve|quick.?filter|neutral.?grey|neutral.?gray|platform.?tools|tiff.?bit.?depth|highlight|raw/i,
  },
  {
    id: 'high-precision-export',
    kind: 'feature',
    what:
      'A TIFF export renders into a 32-bit float target instead of an 8-bit one, '
      + 'so the 16-bit file it has always claimed to write now contains 16-bit data.',
    ours: ['src-tauri/src/mods/export_precision.rs'],
    dependsOn: [
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'GpuProcessor::new', how: 'extends',
        note:
          'Their constructor body became new_with_precision(..., Precision) and new() is a '
          + 'wrapper passing Preview, so their own call site is untouched. The render target '
          + 'format, the shader text, the dither pipeline constant and the bytes per pixel of '
          + 'the readback all come from that one value. If upstream changes the signature the '
          + 'wrapper conflicts, which is the loud failure we want.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'read_texture_data_roi', how: 'extends',
        note: 'Gained a bytes_per_pixel parameter; its single call site passes self.precision.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'to_rgba_f16', how: 'extends',
        note: 'Made pub(crate) so the export path uploads its input exactly as previews do.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'GpuProcessor::run', how: 'extends',
        note:
          'The readback strides multiply by bytes-per-pixel rather than 4. Upstream rewriting '
          + 'that copy loop is the change that would silently tear an export.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', pr: '1466', how: 'borrows',
        note:
          'override HIGH_PRECISION_OUTPUT and the gate around the dither, between '
          + '// upstream #1466 and // end upstream #1466. dimafa, commit 0e8cd15977001cee9f86d5efb6adccc105db4cb1.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'output_texture', how: 'retypes',
        note:
          'export_shader_source() rewrites the rgba8unorm storage declaration to rgba32float by '
          + 'text. It errors rather than no-ops when the declaration is not found exactly once, so '
          + 'a rename upstream fails the build instead of shipping 8-bit data in a 16-bit file.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'process_image_for_export', how: 'extends',
        note: 'Gained a Precision parameter, chosen from the output extension by Precision::for_path.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'export_masks_for_image', how: 'extends',
        note:
          'Routed through render_for_export as well. Missed on the first pass and found by '
          + 'review: the per-mask files take the same extension as the main export, so a TIFF '
          + 'batch with masks on wrote one 16-bit file and N 8-bit companions.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'apply_watermark', how: 'replaces',
        note:
          'The image::imageops::overlay call is replaced by overlay_preserving_precision. Theirs '
          + 'blends through Rgba<u8>, so it quantised every pixel in the stamp bounding box - '
          + 'transparent ones included. Ours delegates straight back to theirs for any image that '
          + 'is not Rgba32F, and a test asserts the 8-bit result is byte for byte identical.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'process_and_get_dynamic_image_inner', how: 'shadows',
        note:
          'A TIFF export goes round it, so it does not reuse the cached processor or the cached '
          + 'input texture. One deliberate behaviour change follows: an image past '
          + 'max_texture_dimension_2d makes a TIFF export fail, where theirs logs a warning and '
          + 'returns the image unedited - so every other format still silently exports an '
          + 'unprocessed file and TIFF says so. Chosen, not overlooked.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'encode_image_to_bytes', how: 'shadows',
        note:
          'Deliberately NOT changed. DynamicImage::to_rgb16 already quantises f32 correctly; it '
          + 'was being handed 8-bit data, which was the whole bug. A test asserts the image crate '
          + 'still agrees with sample_to_u16, NaN included.' },
    ],
    tests: ['src-tauri/src/mods/export_precision.rs #[cfg(test)]'],
    keywords: /tiff|16.?bit|32.?bit|float|precision|bit.?depth|dither|export.?format|rgba32|half/i,
  },
  {
    id: 'export-precision-selector',
    kind: 'feature',
    what:
      'A TIFF can be exported at 8 or 16 bits, chosen in the export panel and '
      + 'remembered between runs.',
    ours: [
      'src/argentum/ExportPrecision.tsx',
      'src/argentum/Argentum.tsx',
      'src/argentum/locales/en.json',
      'src-tauri/src/mods/ag_settings.rs',
      'src-tauri/src/mods/export_precision.rs',
    ],
    dependsOn: [
      { file: 'src/components/panel/right/ExportPanel.tsx', how: 'calls',
        note:
          'One data-argentum="export-precision" marker, rendered only while TIFF is the '
          + 'chosen format, so the control appears and disappears with no state of theirs '
          + 'read from our side. Upstream #1466 does the same job by putting tiffBitDepth '
          + 'on ExportSettings and threading it through six of their files.' },
      { file: 'src/components/ui/ExportImportProperties.tsx', how: 'shadows',
        note: 'Do not add TIFF depth to ExportSettings or ExportPreset; Argentum keeps the output-depth preference global rather than per preset.' },
      { file: 'src/hooks/useExportSettings.ts', how: 'shadows',
        note: 'Do not copy TIFF depth into export-panel or preset state; the independent Argentum depth control owns this preference.' },
      { file: 'src/hooks/useExternalEditSession.ts', how: 'shadows',
        note: 'External edit sessions keep the existing export-settings shape; they do not override the user’s global TIFF-depth preference.' },
      { file: 'src-tauri/src/export_processing.rs', pr: '1466', how: 'borrows',
        note:
          'Their encoder arm, between // upstream #1466 and // end upstream #1466: 8-bit '
          + 'images write Rgb8, everything else Rgb16, and the match covers "tif" as well '
          + 'as "tiff" - a path that always failed here with "Unsupported file format". '
          + 'The depth arrives as the image type rather than as their fourth parameter on '
          + 'encode_image_to_bytes, so their file keeps one call.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'estimate_export_sizes', how: 'extends',
        note:
          'Both estimate sites go through render_for_estimate, which renders on the '
          + 'preview path and widens to 16-bit when that is the depth the export will '
          + 'write. Without it the size shown for a 16-bit TIFF would be the 8-bit one, '
          + 'because the estimate renders its own preview. Rendering them at High instead '
          + 'would be correct and costs a throwaway GpuProcessor per estimate - measured '
          + 'at ~57ms on an Arc 140T - which is unnecessary because this TIFF encoder '
          + 'writes uncompressed, so size is dimensions times depth and does not depend '
          + 'on the pixels. A test asserts that and fails the day it starts compressing.' },
    ],
    tests: [
      'src-tauri/src/mods/export_precision.rs #[cfg(test)] - depth policy, the encoder, and that a TIFF size does not depend on its content',
      'src-tauri/src/mods/ag_settings.rs #[cfg(test)] - one preference does not erase another, including from two threads at once',
    ],
    notes: [
      'Headless export accepts --tiff-bit-depth 8 or 16, defaults to 16, and rejects invalid '
      + 'values before startup. It overrides only the process-local depth in the headless '
      + 'process; the saved global UI preference is not changed.',
      'The one case where the image type does not carry the user intent: an image past '
      + 'max_texture_dimension_2d returns unedited from gpu_processing as Rgba32F, so an '
      + '8-bit TIFF export of it writes a 16-bit container. The export was already wrong '
      + 'there - it is unedited - and this only puts the wrong depth on top.',
    ],
    keywords: /tiff|bit.?depth|8.?bit|export.?panel|export.?settings|preset|selector|dropdown/i,
  },
  {
    id: 'tiff-export-metadata',
    kind: 'feature',
    what:
      'An exported TIFF carries its camera, lens, exposure, date and GPS. The '
      + 'Keep metadata switch, which was shown only for JPEG and silently did '
      + 'nothing for TIFF, now appears for TIFF and means it.',
    ours: ['src-tauri/src/mods/export_metadata.rs'],
    dependsOn: [
      { file: 'src-tauri/src/export_processing.rs', symbol: 'save_image_with_metadata', how: 'extends',
        note:
          'One call changed: write_export_metadata instead of '
          + 'exif_processing::write_image_with_metadata. Ours delegates every format but '
          + 'TIFF straight back to theirs, so a JPEG export is byte-for-byte what it was '
          + '- asserted by a test. The call names no module of ours and so is not a hook, '
          + 'which is why the anchor carries a `requires` for it: an upstream merge that '
          + 'resolved this line back to theirs would pass every other gate.' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'write_image_with_metadata', how: 'calls',
        note:
          'Called twice, and their file is not edited. Once as the passthrough for every '
          + 'format they already handle. Once against a one-pixel JPEG carrier, purely to '
          + 'reuse the ~250 lines of tag gathering they own - full EXIF from a non-RAW '
          + 'source, the .agexif sidecar, rawler for a RAW, GPS - which the function '
          + 'writes but never returns. We read the tags back out of the carrier. '
          + 'Depends on the signature and on JPEG staying a format they write; both break '
          + 'at compile time or in tests, not silently. Their TIFF guard and their '
          + 'Metadata::new() are never on our path, so the FIXME can stay where it is.' },
      { file: 'src/components/panel/right/ExportPanel.tsx', how: 'extends',
        note:
          'One condition widened, from fileFormat == FileFormats.Jpeg to a list holding '
          + 'Jpeg and Tiff. Not a portal: keepMetadata and stripGps are their useState in '
          + 'useExportSettings.ts and are already sent to Rust for every format, so a '
          + 'portal could not reach them and would need a second Argentum-only preference '
          + '- two switches for one idea. Upstream #1322 asks for this same line to cover '
          + 'JXL and WebP, so they will probably edit it; that is a one-line conflict '
          + 'resolved by taking theirs and adding Tiff back.' },
    ],
    tests: [
      'src-tauri/src/mods/export_metadata.rs #[cfg(test)] - pixels, dimensions and bit '
      + 'depth survive at 8 and 16 bits; a source that carries its own ImageWidth cannot '
      + 'stamp it on ours; both spellings of the extension; the switch off rewrites '
      + 'nothing; a JPEG is unchanged from upstream',
    ],
    notes: [
      'Software is written as "Argentum" for a TIFF. Every other format still gets '
      + '"RapidRAW" from their line in exif_processing.rs, which is a rebrand question '
      + 'rather than part of this feature.',
      'The TIFF path parses and re-serialises the whole file, because little_exif owns '
      + 'the strip bytes while the metadata is being edited. Peak memory is roughly 3-4x '
      + 'the encoded size for the duration of the write - about 2.5GB on a 60MP 16-bit '
      + 'export against 1.7GB without. The alternative is writing the Exif IFD ourselves '
      + 'through the tiff crate (TiffEncoder::extra_directory), which is a day of work '
      + 'and belongs with the memory row on the roadmap, not here.',
      'A TIFF *source* is still skipped by their gathering for every output format '
      + '(exif_processing.rs, "Skip TIFF sources to avoid potential tag corruption '
      + 'issues"). Unrelated to this and untouched.',
      'MakerNote is copied with its internal offsets still relative to the source file, '
      + 'so a reader that walks into it can read the wrong thing - a garbage lens name in '
      + 'Lightroom is the shape this takes. Exactly what their JPEG export already does, '
      + 'so it is inherited rather than introduced, and fixing it means rewriting '
      + 'per-manufacturer offset tables. Written down so the next person does not think '
      + 'the TIFF path caused it.',
      'XResolution, YResolution and ResolutionUnit are carried from the source as a set. '
      + 'Our encoder writes 1/1, 1/1, none, which opens as "unspecified" where the same '
      + 'shot exported as JPEG says 300 dpi. Nothing about decoding consults them, so '
      + 'overwriting is safe; all three or none, because two thirds of a resolution is '
      + 'worse than none.',
    ],
    keywords: /tiff|exif|metadata|little_exif|keep.?metadata|strip.?gps|gps|software.?tag|export/i,
  },
  {
    id: 'ai-super-resolution',
    kind: 'feature',
    what: 'Local AI image super-resolution with selectable 2x and 4x scaling.',
    ours: [
      'src-tauri/src/mods/super_resolution.rs',
      'src/argentum/SuperResolutionButton.tsx',
      'src/argentum/SuperResolutionModal.tsx',
      'src/argentum/superResolution.ts',
    ],
    dependsOn: [
      { file: 'src/components/panel/Editor.tsx', how: 'extends',
        note:
          'Caps minimum zoom at fit-to-window so a full-resolution upscaled image can '
          + 'zoom back out to the whole image instead of stopping at a cropped view. '
          + 'Keep this bound if upstream changes the editor zoom calculation.' },
      { file: 'src-tauri/src/adjustment_utils.rs', symbol: 'apply_all_transformations', how: 'calls',
        note:
          'Frames the photo before enlarging it, exactly as the export does: geometry and lens '
          + 'correction, lens blur, then rotation, flips and crop. strip_non_transferable_adjustments '
          + 'removes every key that call reads from the enlarged photo’s sidecar, so nothing is applied '
          + 'twice. If upstream adds a geometric step here, or a new key it reads, add the key there too.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'composite_patches_on_image', how: 'calls',
        note:
          'Inpainting is composited onto the decoded photo before framing, as the editor does in '
          + 'compute_patched_and_warped, because its patches sit in source coordinates the enlargement '
          + 'drops. Without it an enlarged photo silently lost every inpainted area.' },
      { file: 'src-tauri/src/white_balance.rs', symbol: 'pick_white_balance', how: 'calls',
        note:
          'A raw’s white balance is restated for its enlargement with as_shot_white_balance, '
          + 'from_adjustments, adaptation_log_gains, rgb_to_lms, pick_white_balance and '
          + 'WhiteBalance::reference. It rests on as_shot_white_balance answering D65 for anything that '
          + 'is not a raw, and on the gains being lms(as shot) / lms(chosen). If upstream starts reading '
          + 'an as-shot white from a TIFF, or changes how the gains are formed, carry_white_balance must follow.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_curve', how: 'calls',
        note:
          'Mirrored, not called. encode_for_reopening is the inverse of the non-raw srgb_to_linear decode; '
          + 'raw_basic_curve copies the basic tone mapper’s raw branch (BRIGHTNESS_GAMMA 1.1, CONTRAST_MIX '
          + '0.75); apply_curve is ported step for step to fold that branch into the luma curve, which '
          + 'runs after it. A change to any of the three makes an enlarged raw open looking different '
          + 'from the raw.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'resolve_tonemapper_override', how: 'calls',
        note:
          'The Settings tone-mapper override is what a raw actually renders with, so it decides which '
          + 'view transform the enlargement has to reproduce.' },
      { file: 'src-tauri/src/mask_generation.rs', symbol: 'generate_ai_bitmap_from_base64', how: 'calls',
        note:
          'carry_masks moves every mask into the enlargement. AI masks are re-rendered over the enlarged '
          + 'frame with generate_ai_bitmap_from_base64 and TransformParams, and stored as the enlargement’s '
          + 'own bitmap. Shapes rely on the generators measuring centre, ends, strokes and radii in the '
          + 'straightened, uncropped source, with crop_offset subtracted. move_sample_point runs '
          + 'generate_color_bitmap’s placement backwards. A change to any of those conventions moves masks '
          + 'off what they covered.' },
    ],
    tests: [
      'Manual: open an upscaled image, zoom in and back out with the mouse wheel, and verify the full image fits in the editor.',
      'Manual: crop and straighten a photo, enlarge it; the result is the framed area only, and opens with no crop or rotation applied.',
      'Manual: give a raw a custom white balance, an inpainted area, the basic tone mapper and a radial, a linear, a brush and an AI mask, enlarge it; the enlargement opens with the same colour, tone, inpainting and masks as the raw.',
      'super_resolution tests: framing already applied is not carried over, the look is',
      'super_resolution tests: an enlarged raw decodes to its light, is balanced by the raw’s gains, and carries the raw tone curve (basic, base curve, own luma curve, AgX, unedited); shapes, AI masks and colour sample points move into the enlargement',
    ],
    keywords: /upscale|super.?resolution|zoom|image.?size/i,
  },
  {
    id: 'object-brush',
    kind: 'feature',
    what: 'Paint roughly over something and the mask snaps to it (Lightroom\'s Select Object).',
    ours: [
      'src-tauri/src/mods/object_brush.rs',
      'src-tauri/src/mods/matting.rs',
      'src/argentum/ObjectBrush.tsx',
      'src/argentum/objectMask.ts',
      'src/argentum/photoBox.ts',
    ],
    // None of their files is edited. Everything below is something of theirs
    // the feature reads, calls or adds to at runtime, so an upstream change to
    // any of it gets reviewed against this entry rather than discovered.
    dependsOn: [
      { file: 'src/components/panel/right/Masks.tsx', symbol: 'MASK_AI_TYPES', how: 'extends',
        note:
          'The Object tile is spliced into their array at startup, after Subject, and pushed onto '
          + 'ALL_MASK_TYPES and MASK_ICON_MAP. Their grid reads the array on every render. If it '
          + 'becomes a copy, or the grid stops reading it, the tile silently disappears.' },
      { file: 'src/components/panel/right/Masks.tsx', symbol: 'formatMaskTypeName', how: 'calls',
        note:
          'The tile\'s label, and the new component\'s name, is their fallback capitalising the '
          + 'type string "object". A change to the fallback changes the label.' },
      { file: 'src/utils/maskUtils.ts', symbol: 'createSubMask', how: 'calls',
        note:
          'An "object" component is created by their default branch: empty parameters and the '
          + 'fallback name. adoptObjectMasks then makes it ai-subject with their Subject defaults.' },
      { file: 'src/components/panel/right/MasksPanel.tsx', how: 'calls',
        note:
          'handleGridClick, the add-component menu and drag-and-drop all pass the type string to '
          + 'handleAddSubMask / handleAddMaskContainer, which is what carries "object" through.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', how: 'calls',
        note:
          'Four things: presses on the mask stage (.konvajs-content) are swallowed in the capture '
          + 'phase so their box never starts; the overlay svg sized in px to the drawn photo gives '
          + 'the geometry (photoBox.ts, shared with the RGB readout); strokes are drawn beside that '
          + 'svg above their mask preview at zIndex 3; and their ai-subject outline draws the '
          + 'painted extent from startX..endY.' },
      { file: 'src/components/panel/Editor.tsx', how: 'calls',
        note: 'isPanningDisabled covers ai-subject. Without it a brush stroke would also pan the photo.' },
      { file: 'src/hooks/useAiMasking.ts', how: 'calls',
        note:
          'objectMask.ts sends their getTransformAdjustments subset verbatim, and their precompute '
          + 'effect warms the embedding cache when an ai-subject component is selected. If the '
          + 'subset changes on their side, ours must follow or every stroke re-encodes the photo.' },
      { file: 'src/hooks/useKeyboardShortcuts.ts', how: 'calls',
        note: 'brush_size_up / brush_size_down change brushSettings.size in the masks panel, which is the brush size.' },
      { file: 'src/store/useEditorStore.ts', how: 'calls',
        note: 'activeMaskId, brushSettings, isGeneratingAiMask and patchesSentToBackend.' },
      { file: 'src-tauri/src/ai_commands.rs', symbol: 'generate_ai_subject_mask', how: 'calls',
        note:
          'Two things copied from it and kept identical: the embedding cache key (blake3 of the '
          + 'path plus a hash of GEOMETRY_KEYS), and the screen-to-source mapping (fine rotation, '
          + 'flips, quarter turns). A test pins the mapping against their box corners.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'run_sam_decoder', how: 'calls',
        note:
          'The decoder call is ours (more points, a padding point, low_res_masks fed back), but '
          + 'the models, generate_image_embeddings, fast_guided_filter, AiSubjectMaskParameters '
          + 'and AiState.embeddings are theirs, and follow_edges repeats their post-processing so '
          + 'a painted mask looks like a boxed one. Their SAM3 branch replaces all of this; when it '
          + 'lands, this module is rewritten against it, not patched.' },
      { file: 'src-tauri/src/mask_generation.rs', how: 'calls',
        note: 'Renders the result as any ai-subject mask, from maskDataBase64 and the transform fields.' },
      { file: 'src/hooks/useTauriListeners.ts', how: 'calls',
        note:
          'matting.rs emits their ai-model-download-start / -finish events, so the first-use '
          + 'download of the edge model shows in their notice. Renamed events mean a silent '
          + '100 MB download.' },
      { file: 'src-tauri/src/cache_utils.rs', symbol: 'GEOMETRY_KEYS', how: 'calls' },
      { file: 'src-tauri/src/lib.rs', symbol: 'get_cached_full_warped_image', how: 'calls' },
    ],
    tests: [
      'src-tauri/src/mods/object_brush.rs #[cfg(test)]',
      'src-tauri/src/mods/object_brush.rs on_a_real_photo (ignored; needs the SAM models and ORT_DYLIB_PATH; AG_MATTE_MODEL adds the edge model)',
      'src-tauri/src/mods/matting.rs #[cfg(test)]',
      'Manual: Masks > Object, paint over a thing, release; paint again to add, Alt-paint to take away; Start over clears.',
    ],
    keywords: /segment|\bsam\d?\b|select.?object|object.?select|subject.?mask|ai.?mask|point.?prompt|scribble|mask.?type/i,
    retired: {
      recordedIn: '71a07921',
      why:
        'Removed in 26.41.5 by our choice, not because upstream moved. On AK\'s first real '
        + 'test a stroke along one eye took its lids and brow, where their Subject box round the '
        + 'other took that eye alone: a box tells SAM the size of the thing, paint only roughly where it is, and five '
        + 'guessed prompts plus thresholds tuned on a dozen cases did not close that gap. Its '
        + 'code (object_brush.rs, matting.rs, ObjectBrush.tsx, objectMask.ts) is in git at '
        + 'c15e53f0; photoBox.ts and its ImageCanvas dependency moved to rgb-readout.',
    },
  },
  {
    id: 'ai-gpu-runtime',
    kind: 'feature',
    what:
      'AI enlargement on the GPU on Windows: Microsoft’s DirectML build of ONNX Runtime 1.22, '
      + 'downloaded on first use and made the process’s runtime at startup. CPU fallback everywhere.',
    ours: ['src-tauri/src/mods/gpu_runtime.rs'],
    dependsOn: [
      { file: 'src-tauri/src/lib.rs', symbol: 'ORT_DYLIB_PATH', how: 'shadows',
        note:
          'Their setup points ORT_DYLIB_PATH at the bundled CPU runtime. startup::init runs first '
          + 'and pins the DirectML build with ort::init_from, which ort prefers over the variable, so '
          + 'their line stays and is not used once the GPU runtime is installed. If upstream starts '
          + 'loading ONNX Runtime before setup, or calls init_from itself, the pin stops working and '
          + 'enlargement quietly goes back to the CPU.' },
      { file: 'src-tauri/Cargo.toml', how: 'extends',
        note:
          'A second ort entry in our Windows table adds the directml feature to their '
          + 'ort =2.0.0-rc.10, plus flate2. When upstream moves ort (their #1742 goes to rc.13 with '
          + 'its own DirectML), the pinned 1.22 package has to move with it: gpu_runtime will not pin '
          + 'a runtime whose API version differs, and the_pinned_runtime_matches_the_ort_crate fails '
          + 'until the package is updated.' },
      { file: 'src-tauri/src/ai_processing.rs', how: 'retypes',
        note:
          'Once the GPU runtime is installed, their masking, denoise, tagging and inpainting sessions '
          + 'load from the DirectML build instead of the CPU build in CyberTimon/RapidRAW-Models. They '
          + 'ask for no execution provider, so they run on its CPU path, the same 1.22 release.' },
    ],
    tests: [
      'gpu_runtime tests: zip extraction, and the pinned runtime matching the ort crate',
      'Manual (Windows): enlarge a photo; the progress line says "on the GPU". On a fresh profile the GPU runtime downloads first.',
    ],
    keywords: /onnx|\bort\b|directml|execution.?provider|ORT_DYLIB|gpu.*(ai|model|onnx)/i,
  },
  {
    id: 'model-manager',
    kind: 'feature',
    what: 'Settings > General > AI Models: every model the app downloads, what is on disk, and deleting it.',
    ours: [
      'src-tauri/src/mods/model_catalog.rs',
      'src-tauri/src/mods/model_manager.rs',
      'src/argentum/AiModels.tsx',
    ],
    // What is relied on is what their code writes to disk, and that it fetches
    // a missing model again. The card itself is a line in their settings panel.
    dependsOn: [
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'calls',
        note: 'A data-argentum="ai-models" marker straight after their Generative AI card on the '
          + 'General page, which 1.6.5 moved there from Processing. One line. If they move that card '
          + 'again, the marker has to follow it.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_models_dir', how: 'calls',
        note:
          'Their model file names and addresses are private constants here, copied into '
          + 'model_catalog.rs rather than made pub. Its tests read this file and fail when a copy '
          + 'drifts or a new *_FILENAME appears (their SAM3 models are the next one). Deleting relies '
          + 'on download_and_verify_model fetching a missing file again on next use, and on '
          + 'get_models_dir staying app_data_dir()/models. If upstream starts remembering downloads '
          + 'anywhere else, a deleted model would stop coming back and this must follow.' },
    ],
    tests: [
      'src-tauri/src/mods/model_catalog.rs #[cfg(test)]: the copies match ai_processing.rs, super_resolution.rs and gpu_runtime.rs, and every model their code saves is listed',
      'src-tauri/src/mods/model_manager.rs #[cfg(test)]: sizes, deleting one entry and nothing else, path checks, removal at the next start',
      'Manual: Settings > General > AI Models, under Generative AI, lists the models with sizes; delete one, use its feature, and it downloads again.',
      'Manual (Windows): enlarge a photo, then delete Graphics card support; it says it goes at the next start, and is gone after a restart.',
    ],
    keywords: /models?.?(manag|download|delet|remov|folder|dir\b|size)|(delete|remove|manage|clear).{0,12}models?\b|disk.?(space|usage)/i,
  },
  {
    id: 'wb-legacy',
    kind: 'behaviour-change',
    what:
      'White balance saved before RapidRAW 1.6.5 is converted to its units, so an old edit keeps '
      + 'its colour: in renders, when the editor loads it, and in saved presets once.',
    ours: ['src-tauri/src/mods/wb_legacy.rs'],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'calls',
        note: 'First line of their function reads an old edit converted: thumbnails, exports and '
          + 'previews of photos nobody has opened since the update.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'load_metadata', how: 'calls',
        note: 'Hands the editor the converted edit, so its first save stores the new units. If the '
          + 'editor ever loads a sidecar another way, that way needs the same line.' },
      { file: 'src/utils/adjustments.ts', key: 'whiteBalance', how: 'calls',
        note: 'The test for an old edit is a missing whiteBalance key: their INITIAL_ADJUSTMENTS '
          + 'carries it, null in relative mode, so every edit written since 1.6.5 has it. If they '
          + 'drop it from the defaults, new edits would be converted a second time.' },
      { file: 'src-tauri/src/white_balance.rs', symbol: 'shifted', how: 'calls',
        note: 'The conversion targets their model: shifted and from_adjustments read it back, '
          + 'MIRED_PER_RELATIVE_UNIT and TINT_PER_RELATIVE_UNIT give the step. A change to either '
          + 'scale changes what every converted edit means.' },
      { file: 'src-tauri/src/preset_converter.rs', how: 'calls',
        note: 'Why presets are converted once and not on every load: their Lightroom preset import '
          + 'writes 1.6.5 units with no whiteBalance key, and would otherwise be converted twice.' },
    ],
    tests: [
      'src-tauri/src/mods/wb_legacy.rs #[cfg(test)]: conversion against the retired shader\'s gains at three as-shot white balances, masks, kelvin overflow, idempotence, presets in folders, the presets file and its backup',
      'Manual: a photo whose white balance was set in 26.41.5 or earlier opens in the same colour, with its highlights back if any were clipped.',
    ],
    keywords: /white.?balance|temperature|tint|kelvin|preset|sidecar|migrat/i,
  },
  {
    id: 'no-cloud',
    kind: 'behaviour-change',
    what:
      'No RapidRAW Cloud: no Clerk sign-in at launch, no Cloud option, no account plugin in the '
      + 'build, and no address the HTTP permission allows.',
    ours: ['src/argentum/noCloud.ts', 'src/argentum/NoCloudTile.tsx'],
    dependsOn: [
      { file: 'src/store/useCloudStore.ts', symbol: 'initAuth', how: 'shadows',
        note: 'Their AppWrapper still calls it at launch. noCloud.ts sets authStatus to '
          + '\'unsupported\' at import, and their own guard returns before Clerk is touched, '
          + 'as it does on Android and iOS. If the guard goes, Clerk starts again on every launch.' },
      { file: 'src/App.tsx', symbol: 'initAuth', how: 'shadows',
        note: 'Where they start it. noCloud.ts is imported through Argentum.tsx, which App.tsx '
          + 'imports first, so it runs before any effect of theirs.' },
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'shadows',
        note: 'Their Cloud tile is left in their provider list and hidden from our side by '
          + 'NoCloudTile.tsx, found by its translated label. Commenting it out would be a line '
          + 'over their settings file\x27s budget.' },
      { file: 'src-tauri/src/lib.rs', how: 'replaces',
        note: 'Their Clerk plugin (with RapidRAW\'s production key) and the store it keeps its '
          + 'session in are not registered. Deleted lines of theirs: leaving them meant shipping '
          + 'their key and an account plugin in Argentum.' },
      { file: 'src-tauri/Cargo.toml', how: 'replaces',
        note: 'tauri-plugin-clerk and tauri-plugin-store are not built.' },
      { file: 'src-tauri/tauri.conf.json', how: 'replaces',
        note: 'Only the default capability is listed; theirs added desktop-cloud.' },
      { file: 'src-tauri/capabilities/desktop.json', how: 'replaces',
        note: 'Deleted. It granted clerk:default and nothing else, and names a plugin that is no '
          + 'longer built.' },
      { file: 'src-tauri/capabilities/default.json', how: 'replaces',
        note: 'The HTTP permission allows no address. Its only addresses were clerk.getrapidraw.com '
          + 'and www.getrapidraw.com.' },
      { file: 'src-tauri/src/inpainting.rs', how: 'shadows',
        note: 'Their cloud inpainting branch posts to getrapidraw.com. Left in place: it needs the '
          + 'Cloud provider and a Clerk token, and neither can happen here.' },
    ],
    tests: [
      'Manual: Settings > General > Generative AI offers Built-in, AI Connector and AI-Free, and no Cloud.',
      'Manual: with a network monitor, launching Argentum makes no request to clerk.getrapidraw.com, getrapidraw.com or clerk.accounts.dev.',
    ],
    keywords: /cloud|clerk|subscription|sign.?in|account|getrapidraw/i,
  },
  {
    id: 'raw-tone',
    kind: 'feature',
    what:
      'RAW tone rendering: Default, a camera-style Base Curve, or a curve matched to the JPEG the '
      + 'camera embedded in the file, applied on the GPU in place of their tone mapper.',
    ours: ['src-tauri/src/mods/raw_tone.rs', 'src/argentum/RawToneRendering.tsx'],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'raw_tone_mode, the curve and its count, filled from rawToneRendering/rawToneCurve.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'The same fields on the GPU side, and two blocks in main() that replace their tone '
          + 'mapper for a RAW when a curve is set. Their struct and ours must stay in step.' },
      { file: 'src/utils/adjustments.ts', key: 'rawToneRendering', how: 'extends',
        note: 'rawToneRendering and rawToneCurve in their adjustments type, defaults and loader.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'embedded_preview_fallback', how: 'calls',
        note: 'Auto-Matched compares the RAW with this. Kept crate-visible; 1.6.5 dropped its path '
          + 'argument and now finds the preview through raw_processing::extract_embedded_preview.' },
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'develop_raw_image', how: 'calls',
        note: 'The scene-linear RAW the curve is fitted against.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'calls',
        note: 'Rendered in the camera-profile slot.' },
    ],
    tests: ['src-tauri/src/mods/raw_tone.rs #[cfg(test)]'],
    keywords: /tone.?(curve|map)|base.?curve|embedded.?(jpe?g|preview)|agx|filmic/i,
  },
  {
    id: 'object-label',
    kind: 'behaviour-change',
    what:
      'Their Subject mask is labelled Object, Lightroom\'s name for a tool you box something with. '
      + 'Only the label: the type is still ai-subject.',
    ours: ['src/argentum/locales/renames.ts'],
    // None of their files is edited: the word is replaced in i18next at startup.
    dependsOn: [
      { file: 'src/components/panel/right/Masks.tsx', symbol: 'masks.types.subject', how: 'shadows',
        note: 'formatMaskTypeName reads this key for the ai-subject label in the masks and AI '
          + 'panels; renames.ts replaces its value in every locale. A renamed key brings Subject back.' },
      { file: 'src/components/panel/SettingsPanel.tsx', symbol: 'settings.processing.ai.cpu.feature1', how: 'shadows',
        note: 'Their Built-in AI card lists the masks by name. Their word for Subject is swapped '
          + 'for ours inside their sentence, so a rewritten sentence just keeps its own wording.' },
    ],
    tests: [
      'Manual: Masks and AI panels show an Object tile where Subject was; it still boxes a thing. '
        + 'Settings > General > Built-in AI lists Object, Sky, Foreground. French says Objet.',
    ],
    keywords: /select.?object|select.?subject|subject.?mask|object.?mask/i,
  },
  {
    id: 'compact-sliders',
    kind: 'feature',
    what:
      'Compact panels: sliders on one line with a short upright marker on a thin bar, and a smaller tone '
      + 'curve, as Lightroom draws them, chosen in Settings > General. Their slider and curve are '
      + 'restyled from a stylesheet of ours; no line of either changes.',
    ours: [
      'src/argentum/compactSliders.css',
      'src/argentum/compactSliders.ts',
      'src/argentum/CompactSlidersSetting.tsx',
    ],
    // None of their files is edited. The stylesheet finds their slider by its
    // shape, so these are what a change of theirs could quietly undo.
    dependsOn: [
      { file: 'src/components/ui/Slider.tsx', symbol: 'relative w-full h-5', how: 'shadows',
        note: 'The bar\'s wrapper, which identifies a slider: its root is the .group holding it, '
          + 'beside the header row (.flex: the name in .grid, the value in .w-14). Their two-line '
          + 'layout stays and is laid out on one line while compact is on. A reshaped slider stops '
          + 'matching and is drawn their way again.' },
      { file: 'src/components/ui/Slider.tsx', symbol: 'w-14 text-right shrink-0', how: 'shadows',
        note: 'The value column, moved to the third grid column. If it is renamed the value falls '
          + 'into the grid wherever it lands, so check the layout rather than just the selector. '
          + 'Their markers are placed with calc(8px + (100% - 16px) * f), a 16px thumb; our '
          + 'marker is drawn in a 16px box so they stay on the value they mark.' },
      { file: 'src/components/adjustments/Curves.tsx', symbol: 'viewBox="0 0 255 255"', how: 'shadows',
        note: 'Their curve is found by its 255x255 graph: the graph (.relative > .aspect-square) and '
          + 'the button row above it (.flex, first child) are drawn at 62% of the panel, centred. '
          + 'Their drags are measured against the graph\'s on-screen size, which is what makes a '
          + 'smaller graph safe; if that changes, points stop following the pointer.' },
      { file: 'src/components/ui/ColorWheel.tsx', symbol: 'cg-lum-gradient', how: 'shadows',
        note: 'The colour grading sliders are left on two lines, recognised by the cg-hue, cg-sat '
          + 'and cg-lum track classes ColorWheel passes in. They are half a panel wide or less, and '
          + 'one line leaves the bar no room. A renamed class squeezes them.' },
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'calls',
        note: 'The switch is placed after their Font row on General, found by its translated label, '
          + 'from the data-argentum slot argentum-shell registers. If the row is not found it goes '
          + 'in the slot at the end of the page, in a card of its own.' },
    ],
    tests: [
      'Manual: Settings > General > Compact panels on: every adjustment slider is one line with a '
        + 'bar thumb, drag, Shift fine-adjust, click to type, double-click and click-the-name '
        + 'reset all work; a long name ends in ... and shows in full on hover; colour grading wheels '
        + 'keep two lines with the bar thumb; the as-shot marker on Temperature sits under the thumb at as-shot; Row spacing moves the rows live and survives a restart. The '
        + 'tone curve is smaller and its points still follow the pointer, in point and parametric '
        + 'mode. Off restores their layout. The choice survives a restart.',
    ],
    // Not /slider/ alone: half of upstream's commits mention one.
    keywords: /compact|density|slider.?(layout|height|row|size)|one.?line/i,
  },
  {
    id: 'mask-falloff',
    kind: 'behaviour-change',
    what:
      'Linear and radial masks fade with smoothstep instead of a straight ramp, so neither shows a '
      + 'line where the fade starts or stops. Linear is exactly 100% and 0% on its two outer lines; '
      + 'radial uses the curve their own brush feather uses.',
    ours: ['src-tauri/src/mods/mask_falloff.rs'],
    // One import and two lines in their file, each handing their ramp to ours.
    dependsOn: [
      { file: 'src-tauri/src/mask_generation.rs', symbol: 'generate_linear_bitmap', how: 'calls',
        note:
          'Their `0.5 - t * 0.5` is replaced by mask_falloff::linear(t). The curve assumes their '
          + 'meaning of t: distance from the centre line over `range`, the distance to each outer '
          + 'line, positive towards the side that fades out. If they redefine range as the full '
          + 'width, or flip the sign, the fade is the wrong width or the wrong way round, and the '
          + 'two lines mask-guides draws no longer mark 100% and 0%.' },
      { file: 'src-tauri/src/mask_generation.rs', symbol: 'generate_radial_bitmap', how: 'calls',
        note:
          'Their clamp of the feather ramp is replaced by mask_falloff::radial, which clamps and '
          + 'then smoothsteps. It needs the ramp to be 1 at the inner edge of the feather and 0 at '
          + 'the outer; a change to how they compute it changes what ours shapes.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', symbol: 'handleLinearRangeDragMove', how: 'calls',
        note:
          'Where range comes from in their canvas: dragging the outer lines sets it, so their '
          + 'dashed lines sit exactly where the curve reaches 100% and 0%.' },
    ],
    tests: [
      'src-tauri/src/mods/mask_falloff.rs #[cfg(test)]',
      'Manual: a linear mask with Exposure -2 over a sky fades with no line at either outer line '
        + 'and stops at them; a radial mask with feather fades with no ring at the inner edge. '
        + 'Feather 0 is still a hard edge.',
    ],
    keywords: /linear.?(mask|gradient)|radial.?(mask|gradient)|graduated|fall.?off|mask.?feather/i,
  },
  {
    id: 'mask-guides',
    kind: 'feature',
    what:
      'The linear mask on the canvas as two lines, full effect and none, with handles marked 100% '
      + 'and 0%; drawn as a graduated filter, from where the effect is full to where it has gone. The radial '
      + 'mask gets a solid inner ellipse where its full effect ends, following the Feather slider '
      + 'and the outer ellipse as it is dragged. In the masks panel and the AI panel.',
    ours: [
      'src/argentum/LinearMask.tsx',
      'src/argentum/RadialFeather.tsx',
      'src/argentum/linearEdges.ts',
      'src/argentum/photoLayer.ts',
    ],
    // None of their files is edited. Presses are taken before their stage sees
    // them, as the object brush did, and the mask is stored in their format.
    dependsOn: [
      { file: 'src/components/panel/editor/ImageCanvas.tsx', symbol: 'isInitialDraw', how: 'calls',
        note:
          'A new linear component waits for a drag while parameters.isInitialDraw is set. Ours '
          + 'takes that press with a capturing window listener on .konvajs-content, writes the '
          + 'geometry and deletes isInitialDraw on the first move. Their handleUp returns early '
          + 'because isDrawing never went true; if it stops checking that, it would write their '
          + 'empty localInitialDrawParams over ours on release.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', symbol: 'MaskOverlay', how: 'shadows',
        note:
          'Their linear overlay (centre line, two dashed lines, two handles) is drawn on the Konva '
          + 'stage canvas, which is hidden by CSS while a linear component is selected, so the other '
          + 'components\' outlines hide with it. Their hit shapes stay live underneath: the dashed '
          + 'lines coincide with ours and the centre line lies in our band, so ours take those '
          + 'presses first. A press anywhere else goes through to them.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', symbol: 'isSliderDragging', how: 'calls',
        note:
          'Why the radial inner ellipse exists: their red mask preview goes to opacity 0 while any '
          + 'slider is dragged, Feather included. If they keep the preview up for mask sliders, the '
          + 'ellipse is still right but no longer the only way to see the feather.' },
      { file: 'src/components/panel/right/MasksPanel.tsx', symbol: 'createMaskLogic', how: 'calls',
        note:
          'New linear and radial components start with isInitialDraw and their geometry at -10000, '
          + 'which is what tells ours a mask is waiting to be drawn.' },
      { file: 'src/store/useEditorStore.ts', symbol: 'activeMaskId', how: 'calls',
        note: 'The selected component, read with adjustments, showOriginal and selectedImage.' },
      { file: 'src/store/useUIStore.ts', symbol: 'activePanel', how: 'calls',
        note: 'The masks panel and the AI panel, the two that put their mask stage up.' },
      { file: 'src/components/panel/right/AIPanel.tsx', symbol: 'createMaskLogic', how: 'calls',
        note:
          'The AI panel\'s copy of it: new linear and radial components start with isInitialDraw '
          + 'and their geometry at -10000, as in the masks panel.' },
      { file: 'src/store/useEditorStore.ts', symbol: 'activeAiSubMaskId', how: 'calls',
        note: 'The AI panel\'s selection, which it keeps apart from activeMaskId.' },
      { file: 'src/components/panel/Editor.tsx', symbol: 'updateSubMaskLocal', how: 'calls',
        note:
          'withParameters updates a component in masks and in aiPatches, as this does. If they '
          + 'move components somewhere else, ours stops finding them and draws nothing.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', symbol: 'Transformer', how: 'calls',
        note:
          'RadialFeather reads the outer ellipse live, while it is dragged or resized, from the '
          + 'Konva shape their Transformer is attached to. Attach it to something else, or drop it, '
          + 'and the inner ellipse falls back to the stored mask and catches up on release.' },
    ],
    tests: [
      'Manual: Masks > Linear, drag from a point to another: the effect is full where the drag '
        + 'began, 0% where it ended, smooth between. Press Delete straight after: the mask goes. Drag a handle, a line and the band. Undo is '
        + 'one step per drag. Radial: move Feather and the inner ellipse follows; Feather 0 puts it '
        + 'on the outer one. Drag and resize the outer ellipse: the inner one moves with it, not '
        + 'after. Both again in the AI panel.',
    ],
    keywords: /linear.?(mask|gradient)|radial.?(mask|gradient)|graduated|mask.?(handle|overlay|canvas)|konva/i,
  },
  {
    id: 'memory-release',
    kind: 'behaviour-change',
    what:
      'AI models nothing is using are unloaded (the eraser, denoise and tagging after a minute, the '
      + 'mask models after five), freed memory is handed back to the system every 20 seconds, and the '
      + 'log says where the memory is whenever the total moves by 256 MB. RapidRAW keeps every model '
      + 'it has loaded until the app quits: one AI erase on a 32 MP raw left 6 GB behind that '
      + 'survived a change of photo.',
    ours: ['src-tauri/src/mods/memory.rs', 'src-tauri/src/mods/startup.rs'],
    // No line of theirs: the thread starts from the startup anchor and reads
    // their state through its public fields.
    dependsOn: [
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'AiState', how: 'calls',
        note:
          'Ours takes lama_model, denoise_model, clip_models and models out of it when only AiState '
          + 'holds them. That is safe because every one of their callers goes through get_or_init_*, '
          + 'which loads the model again when its slot is empty, and clones the Arc for the length of '
          + 'the job, which is what in-use means here. A model they add as a new field is never '
          + 'unloaded until it is listed in mods/memory.rs; a caller that keeps a model without '
          + 'holding its Arc would have it dropped under it.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_or_init_ai_models', how: 'calls',
        note:
          'Reloading after an unload verifies every model file\'s SHA-256 again and builds the '
          + 'sessions: a few seconds for the five mask models, which is why they wait longest.' },
      { file: 'src-tauri/src/app_state.rs', symbol: 'AppState', how: 'calls',
        note:
          'The report reads original_image, cached_preview, the warped, patched and transformed '
          + 'caches, the geometry, thumbnail and mask caches and the four results, with try_lock. '
          + 'decoded_image_cache keeps its items private, so its other photos are part of "the rest".' },
    ],
    tests: [
      'src-tauri/src/mods/memory.rs #[cfg(test)]',
      'Manual: open a raw, AI-erase something, then leave it: the log says "unloaded the idle AI '
        + 'eraser" a minute later and Task Manager drops with it; erase again and it works.',
    ],
    keywords: /memory|\bram\b|leak|unload|ai.?model|onnx|session|mimalloc|allocator|cache.?size/i,
  },
  {
    id: 'ai-sessions-without-arena',
    kind: 'behaviour-change',
    what:
      'Every AI model session is built with ONNX Runtime\'s CPU memory arena off, so a run\'s working '
      + 'memory is returned when it ends instead of being kept for the life of the model. The mask '
      + 'models held 6.1 GB after one selection on a 32 MP raw, 1.4 GB of it their weights.',
    ours: ['src-tauri/src/mods/ai_session.rs'],
    dependsOn: [
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_or_init_ai_models', how: 'calls',
        note:
          'Its five Session::builder() calls are session_builder(), one line for one, on the anchor '
          + 'taken 2026-10-09. A model they add here with Session::builder() keeps its arena until '
          + 'it is switched too; the anchor\'s requires only guard the SAM encoder and depth lines.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_or_init_denoise_model', how: 'calls',
        note: 'The denoise session, built the same way.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_or_init_clip_models', how: 'calls',
        note: 'The tagging session, built the same way.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_or_init_lama_model', how: 'calls',
        note: 'The eraser session, built the same way.' },
    ],
    tests: [
      'Manual: open a raw, make an AI Subject mask: the [memory] line after it says the masks are '
        + 'loaded at around 1.5 GB more than before, not 5 or 6; the mask is the same as before; '
        + 'a second mask takes no longer than the first did.',
    ],
    keywords: /onnx|\bort\b|arena|ai.?model|inference/i,
  },
  {
    id: 'shared-unchanged-copies',
    kind: 'behaviour-change',
    what:
      'A step that changes nothing shares the photo it was given instead of copying it. Each '
      + 'full-size copy of a 32 MP raw is 500 MB, and with no crop, no lens correction and no '
      + 'patches the open photo was held three times over.',
    ours: [],
    dependsOn: [
      { file: 'src-tauri/src/lib.rs', symbol: 'compute_patched_and_warped', how: 'extends',
        note:
          'Their `Arc::new(blurred.into_owned())`, one line for one: a Cow::Borrowed result is the '
          + 'original itself, so it is the original\'s Arc. Correct only while every step it passes '
          + 'through (composite_patches_on_image, apply_geometry_warp, apply_lens_blur) returns '
          + 'Borrowed when, and only when, it changed nothing. Worth sending upstream.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'compute_full_transformed_res', how: 'extends',
        note:
          'Same, one line for one, for apply_spatial_transformations over the warped image: no '
          + 'crop, rotation or flip shares the warped Arc.' },
    ],
    tests: [
      'Manual: open a raw with no edits; the [memory] line in the log counts fewer copies of the '
        + 'open photo than before, and the picture is unchanged.',
    ],
    keywords: /into_owned|patched_warped|transformed_cache|warped_cache|copy|memory/i,
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
