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
// upstream's it depends on, and what proves it still works. Any file of theirs
// we change must be claimed by an entry here, and once per RapidRAW release
// `npm run review:upstream` shows which of their changes land on these files.
//
// WHAT THIS CANNOT DO
//
// It cannot tell you that upstream has independently built the same feature in
// files we have never touched. `keywords` is a hint over commit subjects, not
// detection, and a subject that says "improve colour handling" will match
// nothing. That is why every release review carries a `sameAsOurs` block
// stating, in a person's words, that the release was read for duplicates.
// Nothing verifies that sentence. It is there so the claim is made explicitly
// and by someone, rather than assumed by a regular expression.
//
// RETIRING AN ENTRY
//
// Entries are never deleted. Set `retired: { recordedIn, why }` where recordedIn
// names the review that decided it: the `release` of a release review, which
// lists the id in its `retires`, or, for the per-commit reviews before
// 2026-10-18, the `through` sha with a `retire:<id>` decision.
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

/** Paths that are ours. Anything under these is unlimited. */
export const OURS = [
  'src-tauri/src/mods/',
  'src-tauri/src/shaders/modules.wgsl',
  'src-tauri/src/shaders/ag_display.wgsl',
  'src-tauri/src/shaders/sharpen.wgsl',
  // The engine library: a crate of our own beside the app, not part of it.
  'src-tauri/engine/',
  // RAWmakase's selection code (MIT), a crate beside the app like the engine.
  'src-tauri/rm_selection/',
  'src/argentum/',
  // The new editor interface, and the contract types generated for it.
  'src/shell/',
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
  // Ours by creation: upstream has no such file (keeps tauri dev off the
  // engine crate's own build output).
  'src-tauri/.taurignore',
  // Ours by creation: upstream has no such workflow. Their three are still
  // theirs, and what we removed from each is claimed in the registry.
  '.github/workflows/upstream.yml',
  // Ours by creation: upstream has no engine crate, and so no workflow for it.
  '.github/workflows/engine.yml',
  // Ours by creation: the app's unit tests, and its shaders on DirectX 12.
  '.github/workflows/app.yml',
  '.github/workflows/dx12.yml',
  // Ours by creation too: upstream has no cargo config. It holds the dev
  // linker (registry entry fast-dev-builds).
  '.cargo/config.toml',
  // Ours by creation: upstream ships no presets. The presets that come with
  // the app, each source in its own folder with its licence (mods/looks.rs).
  'src-tauri/resources/presets/',
];

/**
 * Files that carry the fork's identity rather than its features.
 *
 * The name, the bundle id, the crate. These say "this is Argentum, not
 * RapidRAW" — they were written once, they will fight an upstream merge once,
 * and no feature will ever add to them. Counting them would make the numbers
 * meaningless.
 */
export const IDENTITY = [
  'package.json',
  // Generated from package.json, as Cargo.lock is from Cargo.toml: it moves
  // when a dependency does, and that is not a feature either.
  'package-lock.json',
  'src-tauri/Cargo.toml',
  'src-tauri/Cargo.lock',
  'src-tauri/tauri.conf.json',
  'src-tauri/.identity',
  'src-tauri/src/main.rs',
];

export const isOurs = (file) => OURS.some((prefix) => file.startsWith(prefix));

/**
 * The render path: files that came from RapidRAW and are now Argentum's own.
 *
 * Since 2026-10-18 the code that renders the preview and the export is ours to
 * change, a piece at a time, on the way into the engine. These files carry no
 * line budget, no ratchet and no anchor count. Upstream's changes to them are
 * listed at each release review for reading and copied in by hand when wanted;
 * they are never merged. Entries below may still name them, as a record of what
 * each feature touches.
 */
export const RENDER_PATH = [
  'src-tauri/src/gpu_processing.rs',
  'src-tauri/src/lib.rs',
  'src-tauri/src/image_processing.rs',
  // Their main compute shader, since 2026-10-20: the slow Vulkan and DirectX 12
  // renders were caused inside it (arrays passed by value, copied per pixel),
  // and the fix could only go there.
  'src-tauri/src/shaders/shader.wgsl',
];

export const isRenderPath = (file) => RENDER_PATH.includes(file);

/**
 * Today's interface: the app shell, the editor and its panels, also Argentum's
 * own to change in place since 2026-10-18 - for a layout switch built from
 * today's panels, and for swapping panels one at a time for ones built from the
 * engine's tool registry. Treated like the render path: no ratchet, no anchor
 * count, no shadow check, and upstream's changes read and copied at each release
 * review, never merged blind. A path ending in / is a folder.
 */
export const INTERFACE = [
  'src/App.tsx',
  // Editor.tsx and every panel: right, library, editor.
  'src/components/panel/',
  // The adjustment sections, the panels the registry-driven ones replace.
  'src/components/adjustments/',
  // The editor and library layouts.
  'src/components/views/',
  // The interface's own text: labels change with the panels they belong to.
  'src/i18n/locales/',
  // The interface's dev server.
  'vite.config.mjs',
];

export const isInterface = (file) => INTERFACE.some((p) => (p.endsWith('/') ? file.startsWith(p) : file === p));

/** A file that came from RapidRAW and is ours to change: the render path or today's interface. */
export const isOursToChange = (file) => isRenderPath(file) || isInterface(file);

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
      + 'entry in the white balance menu and a button beside it (white-balance-presets).',
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
      { file: 'src/components/adjustments/Basic.tsx', how: 'calls',
        note: 'Basic starts with the white balance row (WhiteBalance.tsx, moved out of their Color '
          + 'section), whose data-argentum="white-balance" slot holds the menu and the Auto button.' },
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
      { file: 'src/components/adjustments/Basic.tsx', how: 'calls',
        note: 'The data-argentum="white-balance" slot in the white balance row at the top of Basic '
          + '(WhiteBalance.tsx), beside their K and picker buttons. Shared with auto-white-balance.' },
    ],
    tests: [
      'Manual: on a RAW, each preset shows its kelvin on the Temperature slider in K mode and its name '
        + 'on the button; dragging a slider turns the name to Custom, undo brings it back; Auto says Auto '
        + 'and still does after reopening the photo; As Shot returns both sliders to the camera\'s.',
    ],
    keywords: /white.?balance|wb.?preset|daylight|cloudy|tungsten|fluorescent|as.?shot/i,
  },
  {
    id: 'edit-panel-order',
    kind: 'feature',
    what:
      'The Edit panel in the order photos are corrected: Profile (camera profile, RAW rendering, '
      + 'tone mapper, highlight recovery), then Basic (the white balance row with its menu, Auto and '
      + 'picker, Temperature and Tint, then Tone, then Presence: Clarity, Dehaze, Structure, Centre, '
      + 'Vibrance, Saturation), Tone curve, Color, Details, Effects. White balance, Vibrance and '
      + 'Saturation came out of Color, Clarity to Centre out of Details, the tone mapper out of Basic, '
      + 'and the RAW card out of the top of Color. Each section\'s eye, reset, copy and paste follow '
      + 'what it shows, masks included.',
    ours: [
      'src/argentum/editSections.ts',
      'src/argentum/ProfileSection.tsx',
      'src/argentum/DetailTools.tsx',
    ],
    dependsOn: [
      { file: 'src/utils/adjustments.ts', symbol: 'ADJUSTMENT_SECTIONS', how: 'calls',
        note: 'The panel\'s section keys are derived from theirs (editSections.ts EDIT_SECTION_KEYS), '
          + 'with the moved keys taken out of color and details and put in basic, the manual fringe keys '
          + 'out of details (Crop > Lens now), and agSharpen added to details. A key they add to a '
          + 'section arrives in it; a key they move between sections is not followed.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'is_section_visible', how: 'extends',
        note: 'White balance, saturation, vibrance, clarity, dehaze, structure and centre, global '
          + 'and in masks, are turned off by the basic eye, not by color\'s or details\'. An edit saved '
          + 'with Color or Details turned off renders those with them on. The manual fringe sliders are '
          + 'read with no section, like the lens corrections beside them in Crop.' },
      { file: 'src/components/adjustments/Basic.tsx', how: 'extends',
        note: 'Basic renders the white balance row and sliders (WhiteBalance.tsx, their Color code '
          + 'moved unchanged), Tone and Presence. ToneMapperSwitch is exported for Profile, without '
          + 'the Exposure slider it held.' },
      { file: 'src/components/adjustments/WhiteBalance.tsx', how: 'extends',
        note: 'New beside their sections: their white balance row and Temperature and Tint sliders, '
          + 'moved unchanged out of Color.tsx, with the label and the white-balance slot added. Without '
          + 'the K button: Kelvin is a switch in Settings.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'extends',
        note: 'Without its White Balance and Presence tools and the camera-profile slot.' },
      { file: 'src/components/adjustments/Details.tsx', how: 'extends',
        note: 'Without its Presence and Chromatic Aberration tools; Denoise… under Noise reduction and '
          + 'an Enhance tool with Restore & Enlarge… (argentum/DetailTools.tsx).' },
      { file: 'src/store/useUIStore.ts', symbol: 'denoiseModalState', how: 'calls',
        note: 'Denoise… opens their dialog for the open photo as the right-click menu does.' },
      { file: 'src/components/panel/right/CropPanel.tsx', how: 'extends',
        note: 'A Manual fringe tile after the profile tiles in Lens Correction, never greyed; the lens '
          + 'reset also puts the fringe sliders back.' },
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'extends',
        note: 'White balance in Kelvin, a switch after Tool Focus Mode setting their whiteBalanceMode.' },
      { file: 'src/components/panel/right/ControlsPanel.tsx', how: 'extends',
        note: 'Profile first; Reset all, a section\'s reset, copy and paste by EDIT_SECTION_KEYS. '
          + 'Profile folds with their sections under "profile" in their open sections. The header\'s ⋯: '
          + 'handleCopyAdjustments, handlePasteAdjustments, their Copy & Paste Settings dialog '
          + '(isCopyPasteSettingsModalOpen), copyPasteSettings.autoSync and Customize Panels.' },
      { file: 'src/components/panel/right/MasksPanel.tsx', how: 'extends',
        note: 'A mask\'s section copy, paste and reset by EDIT_SECTION_KEYS.' },
      { file: 'src/components/panel/right/AdjustmentSectionsSubMenu.tsx', how: 'extends',
        note: 'The Customise menu leaves out the tools that moved into Basic (MOVED_TOOLS).' },
    ],
    tests: [
      'src-tauri/src/image_processing.rs #[cfg(test)] section_eyes_follow_the_edit_panel.',
      'Manual: open a RAW. Edit reads Profile, Basic, Curves, Color, Details, Effects; Basic starts '
        + 'with White Balance (As Shot menu, Auto, picker). Turn Basic\'s eye off: white balance '
        + 'and Presence go too; Color\'s eye leaves them. Reset Basic: Temperature and Clarity go back. '
        + 'Reset all: the profile goes back to Built-in.',
      'Manual: Details shows Denoise… (opens the Denoise dialog) and Enhance > Restore & Enlarge…; '
        + 'Crop > Lens Correction shows Manual fringe, and Details\' eye leaves it on. Settings > '
        + 'General > White balance in Kelvin shows Temperature in K. ⋯ in the Edit header copies, '
        + 'pastes, opens Copy & Paste Settings and turns Auto-sync on with a tick.',
    ],
    keywords: /section|basic|presence|white.?balance|profile|tone.?mapper|edit.?panel/i,
  },
  {
    id: 'selection-masks',
    kind: 'feature',
    what:
      'Subject, Sky, Background and Object masks: RAWmakase\'s selection (DETR panoptic finds people, '
      + 'animals and sky, SAM 2.1 outlines them and answers clicks), its crate taken whole as src-tauri/rm_selection '
      + '(crates/rawmakase-inference @ 7c3c81a, MIT, ported to ort rc.10). A selection is stored '
      + 'as their Foreground (Subject, Background) or Sky mask stores its own, with agSelection '
      + 'naming which, so it renders through their code unchanged. The models (271 MB) are fetched '
      + 'on a yes, pinned and checksummed, into models/rawmakase-selection.',
    ours: [
      'src-tauri/rm_selection/',
      'src-tauri/src/mods/selection.rs',
      'src-tauri/src/mods/selection_models.rs',
      'src/argentum/selection/selection.tsx',
    ],
    dependsOn: [
      { file: 'src/components/panel/right/Masks.tsx', how: 'extends',
        note: 'Three menu-only types (ag-subject, ag-sky, ag-background) with names and icons; '
          + 'MASK_AI_TYPES lists Subject, Object, Sky, Background, Foreground, Depth, their Sky '
          + 'replaced by ours.' },
      { file: 'src/components/panel/right/MasksPanel.tsx', how: 'extends',
        note: 'createMaskLogic makes a selection sub-mask as their Foreground or Sky with our '
          + 'starting parameters; the two add paths call useSelectionMasks; icons come from '
          + 'menuTypeOf.' },
      { file: 'src/utils/maskUtils.ts', symbol: 'createSubMask', how: 'calls',
        note: 'Makes the stored sub-mask; its name is then set to ours.' },
      { file: 'src/components/panel/editor/ImageCanvas.tsx', how: 'extends',
        note: 'An Object selection takes their Object box gesture: a click (under 5 px of drag) adds a '
          + 'point, with Alt it leaves one out, a drag replaces the box (refineObject). Its clicks are '
          + 'drawn as green and red dots, its box as theirs is.' },
      { file: 'src/components/panel/Editor.tsx', how: 'extends',
        note: 'Panning is off while an Object selection is active, as for their Object.' },
      { file: 'src-tauri/src/ai_commands.rs', symbol: 'generate_ai_subject_mask', how: 'calls',
        note: 'Its mapping from the canvas to the warped picture (fine rotation, flips, quarter turns) '
          + 'is repeated in selection.rs to_picture for each click, with tests.' },
      { file: 'src-tauri/src/mask_generation.rs', symbol: 'generate_ai_foreground_bitmap', how: 'calls',
        note: 'Draws a selection: the ai-foreground and ai-sky arms read maskDataBase64, rotation, '
          + 'flips and orientation steps and ignore agSelection. Change those parameters and '
          + 'selections stop drawing.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'get_cached_full_warped_image', how: 'calls',
        note: 'The picture the models see, shared with their mask models\' cache.' },
      { file: 'src/store/useEditorStore.ts', symbol: 'patchesSentToBackend', how: 'calls',
        note: 'Cleared for a sub-mask when its mask changes, as their AI masks do, so it is sent again.' },
    ],
    tests: [
      'src-tauri/rm_selection: cargo test (27 model-free tests of RAWmakase\'s); tests/real_model.rs '
        + 'with --ignored and the models.',
      'src-tauri/src/mods/selection_models.rs #[cfg(test)]: an empty folder needs every file; the '
        + 'right size with the wrong bytes fails the check.',
      'src-tauri/src/mods/selection.rs #[cfg(test)]: the analysis is reused only for the same photo '
        + 'and geometry; a_person_is_found_and_background_is_the_rest with --ignored on a real RAW.',
      'Manual: Masks > Subject on a photo of a person; Background; Sky on a landscape; with the '
        + 'models folder empty, the size is asked before anything downloads.',
      'src-tauri/src/mods/selection.rs #[cfg(test)]: clicks go back through flips, turns and tilt; clicks '
        + 'and a box become fractions; a_click_on_the_person_selects_the_person with --ignored.',
      'Manual: Masks > Object, click a person, Alt-click their bag, drag a box round a car; Start over.',
    ],
    keywords: /subject|sky|background|selection|sam ?2|detr|panoptic/i,
  },
  {
    id: 'copy-our-keys',
    kind: 'feature',
    what:
      'Copy settings, Paste and presets carry cameraProfile, rawToneRendering, agSharpen, '
      + 'whiteBalanceAuto and agBlackWhite. They are in their copy groups (a Camera Profile, RAW Rendering & B&W group; '
      + 'agSharpen with Sharpness; whiteBalanceAuto with White Balance) '
      + 'and written into their paste list (mods/copy_keys.rs holds the test). A profile made for another camera is '
      + 'left out of a render (profile_correction fits_this_photo); a curve fitted for another '
      + 'rendering is not used (raw_tone points_of, by rawToneCurveMode). Never rawToneCurve: it '
      + 'is fitted to one photo. whiteBalanceAuto names the photo it was measured on, and each photo '
      + 'it reaches is measured again (argentum/autoWhiteBalance.ts, mods/auto_wb_photo.rs).',
    ours: [
      'src-tauri/src/mods/copy_keys.rs',
      'src-tauri/src/mods/auto_wb_photo.rs',
      'src/argentum/autoWhiteBalance.ts',
    ],
    dependsOn: [
      { file: 'src/utils/adjustments.ts', symbol: 'ADJUSTMENT_GROUPS', how: 'extends',
        note: 'One group line of ours at the top of basic, agSharpen in their Sharpness group\'s line '
          + 'and whiteBalanceAuto in their White Balance group\'s; rawToneCurve has no default of ours '
          + 'any more, to make room. COPYABLE_ADJUSTMENT_KEYS, presets and the Copy & Paste Settings dialog follow from '
          + 'it. If they rebuild the groups, the keys must go back in.' },
      { file: 'src/utils/adjustments.ts', symbol: 'normalizeLoadedAdjustments', how: 'calls',
        note: 'Our keys come back from a saved edit only because their loader spreads it over the '
          + 'defaults; our own checks of them there were removed to stay under the line cap. If the '
          + 'loader starts picking keys, the camera profile and RAW rendering are lost on reload.' },
      { file: 'src-tauri/src/app_settings.rs', symbol: 'all_available_adjustments', how: 'extends',
        note: 'Our five keys written into their list, four lines (agBlackWhite on the whiteBalanceAuto line); '
          + 'a redundant serde default on last_import_settings (an Option) went to make room. Their load switches a key new to '
          + 'the list on in includedAdjustments, which is what makes Paste carry ours for existing users.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'handlePasteAdjustments', how: 'calls',
        note: 'Paste applies the keys in copyPasteSettings.includedAdjustments and nothing else. '
          + 'It goes to the selection, else the open photo, and sets isPasted after it: the Auto '
          + 'watcher reads the same targets. A paste elsewhere is caught when the photo opens.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'apply_adjustments_to_paths', how: 'calls',
        note: 'Writes every sidecar before drawing any thumbnail. auto_wb_photo waits for the pasted '
          + 'Auto to reach a sidecar before measuring, then writes the photo\'s own through it.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'load_base_image_from_bytes', how: 'calls',
        note: 'auto_wb_photo decodes a photo that is not open as Restore & Enlarge does, then '
          + 'composite_patches_on_image and mask_generation build_full_warped_image, the picture the '
          + 'open photo\'s Auto reads.' },
    ],
    tests: [
      'src-tauri/src/mods/copy_keys.rs #[cfg(test)] our_keys_are_pasted_by_default.',
      'src-tauri/src/mods/profile_correction.rs #[cfg(test)] a_profile_for_another_camera_is_left_out.',
      'src-tauri/src/mods/raw_tone.rs #[cfg(test)] a_curve_fitted_for_another_rendering_is_not_used.',
      'src-tauri/src/mods/auto_wb_photo.rs #[cfg(test)] a_pasted_photo_is_measured_for_its_own_light, '
        + 'a_paste_that_does_not_arrive_is_given_up_on.',
      'Manual: press Auto on a RAW, Copy settings, select photos shot under other light, Paste: a '
        + 'note counts them, and each thumbnail changes to its own white balance; opening one shows '
        + 'Auto in the white balance menu.',
      'Manual: on a RAW choose a camera profile, Base Curve and a Capture sharpening change; Copy settings; '
        + 'paste onto a RAW from the same camera: all three arrive. Onto a RAW from another camera: '
        + 'the rendering and sharpening arrive, the profile shows Built-in.',
    ],
    keywords: /copy.?settings|paste|preset|camera.?profile|raw.?rendering|agSharpen|auto.?white|whiteBalanceAuto/i,
  },
  {
    id: 'snapshots',
    kind: 'feature',
    what:
      'Snapshots, in the Modern left column above History: named states of a photo\'s edit. + or '
      + 'Ctrl+N keeps the photo as it is now and starts naming it; a click puts one back as a History '
      + 'step; double-click renames; right-click renames, updates or deletes. Kept in the sidecar as '
      + 'agSnapshots beside the edit, so undo never removes one. The line above the list drags its '
      + 'height (snapshotsHeightPx in the layout). RAWmakase\'s behaviour '
      + '(src/app/snapshots.rs @ 7c3c81a), kept in the sidecar because there is no catalog.',
    ours: [
      'src/argentum/snapshots/SnapshotsSection.tsx',
      'src/argentum/snapshots/snapshotsStore.ts',
      'src-tauri/src/mods/snapshots.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'ImageMetadata', how: 'extends',
        note: 'agSnapshots, an optional field beside adjustments. Their save of the edit '
          + '(save_metadata_and_update_thumbnail) reads the sidecar and replaces only adjustments, '
          + 'which is what keeps the snapshots; if it starts writing a fresh record, they are lost.' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'load_sidecar', how: 'calls',
        note: 'Snapshots are read and written through it, the rest of the sidecar kept as read.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'setAdjustments', how: 'calls',
        note: 'A snapshot is put back through it, so it is a History step and Undo takes it off.' },
      { file: 'src/utils/adjustments.ts', symbol: 'normalizeLoadedAdjustments', how: 'calls',
        note: 'A snapshot\'s edit is read the way a saved edit is, so one from an older version still applies.' },
    ],
    tests: [
      'src-tauri/src/mods/snapshots.rs #[cfg(test)]: kept beside the edit, the rest of the sidecar '
        + 'untouched; deleting the last takes the field out.',
      'Manual: open a photo, Ctrl+N, type a name, Enter. Change Exposure, click the snapshot: '
        + 'Exposure is back; Ctrl+Z takes it off and the snapshot stays. Restart: still there. In '
        + 'the grid the photo\'s snapshots show dimmed.',
    ],
    keywords: /snapshot/i,
  },
  {
    id: 'looks-tab',
    kind: 'feature',
    what:
      'The Looks tab: presets, LUTs and film looks in one list, with search, chips (All, Favourites, '
      + 'Presets, LUTs; the film looks are the first folder of LUTs), hover preview on the photo, click to put on and take off, stars, one '
      + 'Import, and On this photo with a LUT\'s Strength and a preset\'s Amount. The presets that '
      + 'come with the app are RAWmakase\'s (assets/presets @ 7c3c81a, BandW @ a8c5d6d7; MIT). Amount '
      + 'follows RAWmakase\'s rule on RapidRAW\'s mixing: from the photo before the preset, only '
      + 'the preset\'s settings. The LUT block left Effects. Suggested for this photo: the LUTs '
      + 'picked for the photos most like the open one, by light and colours (look_picks.rs).',
    ours: [
      'src/argentum/looks/LooksPanel.tsx',
      'src/argentum/looks/looksStore.ts',
      'src/argentum/looks/presetAmount.ts',
      'src/argentum/looks/suggestions.ts',
      'src-tauri/src/mods/looks.rs',
      'src-tauri/src/mods/look_picks.rs',
    ],
    dependsOn: [
      { file: 'src/hooks/useEditorActions.ts', symbol: 'setAdjustments', how: 'calls',
        note: 'A LUT or film look goes on as their handleLutSelect puts one on, and previews as '
          + 'setLutPreviewOverride does, with a mark of ours (entry film-look-input); a preset '
          + 'previews through the editor store\'s previewOverride, as the LUT preview does.' },
      { file: 'src/hooks/usePresets.ts', symbol: 'usePresets', how: 'calls',
        note: 'The user\'s presets and folders, their import and every change to them: addPreset, '
          + 'configurePreset, overwritePreset, duplicatePreset, movePreset, addFolder, renameItem, '
          + 'exportPresetsToFile, deleteItem. A folder\'s children are plain presets.' },
      { file: 'src/components/modals/ConfigurePresetModal.tsx', how: 'calls',
        note: 'Save > As preset and Configure, as today\'s presets panel opens it; CreateFolderModal '
          + 'and RenameFolderModal the same way.' },
      { file: 'src/components/panel/right/ExportPanel.tsx', symbol: 'wantLutExport', how: 'extends',
        note: 'Save > As LUT file opens the Export tab with the format set to CUBE, read once from '
          + 'looksStore.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'apply_adjustments_to_paths', how: 'calls',
        note: 'A look clicked in the grid goes onto the selected photos as their paste does; the '
          + 'Undo puts back each photo\'s own values for the keys the look changed (load_metadata).' },
      { file: 'src-tauri/src/lut_processing.rs', symbol: 'list_luts', how: 'calls',
        note: 'Film looks are the built-in LUTs; with list_lut_libraries, import_luts, remove_lut, '
          + 'set_lut_library (Move to), create_lut_library, rename_lut_library, delete_lut_library, '
          + 'and generate_lut_previews for the swatches. Their uncategorized library cannot be '
          + 'renamed, so renaming Imported creates a library and moves its LUTs there.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'save_presets', how: 'calls',
        note: 'Copy to My presets writes the user\'s list with the copied preset or group added, '
          + 'then reloads it through usePresets.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'generate_preset_preview', how: 'calls',
        note: 'A preset\'s swatch, rendered on the app\'s defaults as today\'s presets panel does.' },
      { file: 'src-tauri/src/preset_converter.rs', symbol: 'convert_xmp_to_preset', how: 'calls',
        note: 'Reads the shipped RAWmakase XMPs, as an imported XMP is read. What it does not map, '
          + 'Convert to Grayscale and the gray mixer, looks.rs adds from black_white::from_xmp.' },
      { file: 'src/components/adjustments/Effects.tsx', how: 'extends',
        note: 'Without its LUT block; editSections.ts takes the LUT keys out of Effects and the tool '
          + 'out of the Customise menu.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_global_adjustments_from_json', how: 'extends',
        note: 'The LUT is applied whatever Effects\' eye says.' },
      { file: 'src/components/panel/right/PresetsPanel.tsx', symbol: 'mixAdjustments', how: 'shadows',
        note: 'presetAmount.ts is a copy with RAWmakase\'s changes; theirs still serves the presets '
          + 'panel where it shows (a narrow portrait window).' },
    ],
    tests: [
      'src-tauri/src/mods/looks.rs #[cfg(test)]: every shipped preset reads into its group; stars read back.',
      'Manual: open a photo, press P. Hover K-Portra 400: the photo previews it; away: back. Click it: '
        + 'On this photo with Strength. Open Presets > Color, click Warm: Amount; drag to 0: the '
        + 'photo is as before the preset, other edits kept. Star a look: Favourites lists it after a '
        + 'restart. Import a .cube and an .xmp at once: both appear. Effects has no LUT block.',
    ],
    keywords: /preset|lut|look|film|cube|favourite|amount/i,
  },
  {
    id: 'black-white',
    kind: 'feature',
    what:
      'B&W: a Color | B&W switch at the top of Basic (V too) and a B&W Mix in the Color Mixer\'s '
      + 'place. The photo turns grey in ag_stage_color (modules.wgsl), after their Saturation and '
      + 'before their Color Grading, so grading tints the grey, with RAWmakase\'s measured mix '
      + '(engine rm_black_white.rs @ a8c5d6d7; its chart generated by scripts/rawmakase-gray-chart.mjs). '
      + 'Their Color Mixer, Hue, Saturation and Vibrance are left out of the render while it is on. '
      + 'Looks ships RAWmakase\'s six B&W presets; Copy settings carries agBlackWhite.',
    ours: [
      'src-tauri/engine/src/rm_black_white.rs',
      'src-tauri/engine/src/rm_black_white_chart.tsv',
      'src-tauri/src/mods/black_white.rs',
      'src/argentum/BlackWhite.tsx',
      'src/argentum/blackWhite.css',
      'scripts/rawmakase-gray-chart.mjs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_color', how: 'calls',
        note: 'One call after their apply_creative_color and before apply_color_grading. Render path.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'ag_black_white, its last field (AgBlackWhite in modules.wgsl); shader_check holds the '
          + 'sizes on both sides. Render path.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_global_adjustments_from_json', how: 'extends',
        note: 'Fills ag_black_white, and leaves their colour controls out while it is on '
          + '(black_white::leave_out_colour_controls). Render path.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'ag_gray_grid_buffer', how: 'extends',
        note: 'Binding 14, the mix\'s grid, written in run() when a render asks for B&W. Render path.' },
      { file: 'src/components/adjustments/Basic.tsx', how: 'extends',
        note: 'The switch above white balance; Vibrance and Saturation greyed in B&W. Today\'s interface.' },
      { file: 'src/components/adjustments/Color.tsx', how: 'extends',
        note: 'The B&W Mix in the colorMixer subsection\'s place, same id and order; Hue greyed in B&W. '
          + 'Today\'s interface.' },
    ],
    tests: [
      'src-tauri/engine/src/rm_black_white.rs #[cfg(test)]: the chart reads whole; RAWmakase\'s own test '
        + 'of it (neutrals kept, a band at -100 near black, +100 brighter, no band moves a grey).',
      'src-tauri/src/mods/black_white.rs #[cfg(test)]: the edit, what B&W leaves out, the XMP keys; '
        + 'gpu (ignored, needs a GPU): through the shader each colour is within 0.3/255 of the chart\'s '
        + 'grey, Color Grading tints it, the colour controls do nothing in B&W; sheet (ignored): real RAWs.',
      'src-tauri/src/mods/looks.rs #[cfg(test)]: every B&W preset, and no other, turns the photo grey.',
      'node scripts/test-rawmakase-gray-chart.mjs: the chart is what the script makes from the release it names.',
      'Manual: open a photo, click B&W at the top of Basic: grey; Color > B&W Mix, drag Reds: red things '
        + 'lighten. Press V: Color again, its Color Mixer as it was. Looks > B&W > Sepia: a warm grey.',
    ],
    keywords: /black.?(and|&).?white|b&w|monochrome|gr[ae]yscale|gr[ae]y.?mix|treatment|toning/i,
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
      { file: 'src/components/panel/right/ControlsPanel.tsx', how: 'calls',
        note: 'Renders the Profile section (ProfileSection.tsx, edit-panel-order) first, holding the '
          + 'profile, the RAW rendering, the tone mapper and highlight recovery.' },
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
    id: 'first-thumbnail',
    kind: 'feature',
    what: 'While browsing, a RAW\'s thumbnail is its camera\'s JPEG at once; the made thumbnail replaces it.',
    ours: ['src-tauri/src/mods/first_thumbnail.rs', 'src-tauri/src/mods/sb_embedded.rs'],
    dependsOn: [
      { file: 'src-tauri/src/file_management.rs', symbol: 'generate_single_thumbnail_and_cache', how: 'calls',
        note: 'One line, after the cache check, for the browsing queue only (force_regenerate false). The '
          + 'grid takes the picture as the data of their thumbnail-generated event, which useTauriListeners.ts already reads.' },
    ],
    tests: ['src-tauri/src/mods/first_thumbnail.rs #[cfg(test)]'],
    keywords: /thumbnail|embedded.?(jpeg|preview)/i,
    retired: {
      recordedIn: '79c2a46b448b9a0bee0b30f8c345daad003ad694',
      why:
        'Reverted on 2026-10-18 by our choice, not because upstream moved: the camera JPEG first '
        + 'showed edited photos in their unedited look, and thumbnail changes were not wanted. '
        + 'Their thumbnail path is unchanged again. The code is in git history (#83 and #78).',
    },
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
    what: 'Highlight recovery, and the sigmoid that lands it. What counts as clipped follows the '
      + 'white level rawler reads, lowered to the white the file proves (a hard-clip spike in the '
      + 'histogram, highlights::proven_white) where the stated one sits above the point the sensor '
      + 'stopped, as Canon\'s SpecularWhiteLevel does on some 5D Mark II frames. The white that '
      + 'scales the picture is rawler\'s, untouched.',
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
    what: 'The fork is Argentum: name, bundle id, no donation link.',
    ours: ['src-tauri/.identity', 'scripts/check-identity.mjs'],
    dependsOn: [
      { file: 'src/components/panel/MainLibrary.tsx', how: 'replaces',
        note: 'Their start screen is gone (editor-layout-modern), and with it the update check that '
          + 'pointed at our releases and the place their Ko-fi link was: Argentum must not raise money in '
          + 'another author’s name. The credit is in Special Thanks and CREDITS.md.' },
      { file: 'package.json', how: 'rebrands' },
      { file: 'package-lock.json', how: 'rebrands',
        note: 'Its package name follows package.json, which npm rewrites on every install.' },
      { file: 'src-tauri/Cargo.toml', how: 'rebrands' },
      { file: 'src-tauri/tauri.conf.json', how: 'rebrands' },
      { file: 'src-tauri/src/main.rs', how: 'rebrands' },
      { file: 'index.html', how: 'rebrands' },
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
        note: 'Carries the editorNeutralGreyBg setting. The test that legacy highlight-compression values still round-trip is in mods/raw_tone.rs.' },
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
      'src-tauri/src/mods/raw_tone.rs #[cfg(test)] legacy_raw_highlight_compression_setting_survives_load_and_save',
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
    id: 'restore',
    kind: 'feature',
    what: 'Restore: OmniLens undoes lens softness at the photo’s own size, through a detail mask, '
      + 'before Capture sharpening. In the Super Resolution window, alone or before an enlargement; one file. '
      + 'Before any enlargement of a RAW, its capture sharpening is applied at the camera’s size (order '
      + 'Restore, Capture, Enlarge - DEC-25).',
    ours: [
      'src-tauri/src/mods/restore.rs',
      'src-tauri/src/mods/capture_bake.rs',
      'src-tauri/src/mods/super_resolution.rs',
      'src-tauri/src/mods/sharpen.rs',
      'src/argentum/SuperResolutionModal.tsx',
      'src/argentum/Sharpening.tsx',
    ],
    dependsOn: [
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'get_or_init_gpu_context', how: 'calls',
        note:
          'Capture before an enlargement runs on the editor’s own graphics context, so the sharpening '
          + 'engine and its pipelines are shared with the preview rather than built twice.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'to_rgba_f16', how: 'calls',
        note:
          'Uploads the photo to be capture-sharpened as their preview uploads its input, Rgba16Float. '
          + 'If they change the input format, capture_bake’s read-back (8 bytes a texel) must follow.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'load_base_image_from_bytes', how: 'calls',
        note:
          'Restore decodes the RAW through it, as Super Resolution does, and that decode is what records '
          + 'the RAW’s capture-sharpening radius (mods::decode::on_raw_decoded). The restored file carries '
          + 'that radius in agRestore, because a TIFF has no mosaic to measure it from.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'srgb_to_linear', how: 'calls',
        note:
          'Capture sharpening on a restored file relies on their pipeline handing a TIFF to the GPU '
          + 'sRGB-encoded, as encode_for_reopening wrote it; the sharpening shader linearises a non-RAW '
          + 'before deconvolving. If upstream starts linearising TIFFs on load, a restored file would be '
          + 'linearised twice and Capture would deconvolve the wrong light.' },
    ],
    tests: [
      'restore tests: the default is the tuned threshold; smooth areas keep the original; edges take the restored photo; 0 is the whole photo; a tighter mask never reaches further; the window keeps one photo (decoded once, a small mask view) and one model output',
      'super_resolution tests: the file is named for what was done; a recipe does something and only known scales; only a restored RAW carries its capture radius; Restore runs at its one tile',
      'sharpen tests: a restored RAW keeps capture sharpening at its RAW’s radius',
      'capture_bake tests (GPU): capture steepens an edge and off means untouched; only capture is applied, the manual Sharpen stays for the editor',
      'Manual: Restore a wide-open telephoto RAW at the default mask; eyes, whiskers and fur edges are crisper, a soft background is unchanged, and the _Restored file opens with Capture sharpening on at the RAW’s radius.',
    ],
    keywords: /restor|deblur|aberration|lens.?soft|omnilens|swinir/i,
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
        'Removed in 26.41.5 by our choice, not because upstream moved. On the first real '
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
    what: 'Settings > AI and models: every model the app downloads, what is on disk, downloading, deleting it, and clearing what no model uses.',
    ours: [
      'src-tauri/src/mods/model_catalog.rs',
      'src-tauri/src/mods/model_download.rs',
      'src-tauri/src/mods/model_manager.rs',
      'src/argentum/AiModels.tsx',
    ],
    // What is relied on is what their code writes to disk, and that it fetches
    // a missing model again. The card itself is a line in their settings panel.
    dependsOn: [
      { file: 'src/components/panel/SettingsPanel.tsx', how: 'calls',
        note: 'The data-argentum slot their page ends with, on the Modern window\x27s AI and models '
          + 'section, which shows their Generative AI card (moved to General by 1.6.5) and then this. Under '
          + 'Classic it was placed under that card on General by AiModelsPlacement.tsx, deleted with Classic.' },
      { file: 'src-tauri/src/ai_processing.rs', symbol: 'get_models_dir', how: 'calls',
        note:
          'Their model file names, addresses and SHA-256s are private constants here, copied into '
          + 'model_catalog.rs rather than made pub. Its tests read this file and fail when a copy '
          + 'drifts or a new *_FILENAME appears (their SAM3 models are the next one). The Download '
          + 'button relies on download_and_verify_model accepting a file with that name and checksum '
          + 'without fetching it again, and Clear on SKYSEG_LEGACY_FILENAME being the only old name '
          + 'promote_legacy_model_filename renames (RENAMED in model_catalog.rs). Deleting relies '
          + 'on download_and_verify_model fetching a missing file again on next use, and on '
          + 'get_models_dir staying app_data_dir()/models. If upstream starts remembering downloads '
          + 'anywhere else, a deleted model would stop coming back and this must follow.' },
    ],
    tests: [
      'src-tauri/src/mods/model_catalog.rs #[cfg(test)]: the copies (names, addresses, checksums) match ai_processing.rs, super_resolution.rs and gpu_runtime.rs, every model their code saves is listed, the selection entry is the rm_selection manifest, and every file that writes to the models folder is known',
      'src-tauri/src/mods/model_download.rs #[cfg(test)]: against a local server, a matching file lands under its name, a wrong one is not kept, only missing or wrong files are fetched, the fallback is tried, Stop leaves nothing, nothing is written through a junction',
      'src-tauri/src/mods/model_manager.rs #[cfg(test)]: sizes, deleting one entry and nothing else, path checks, removal at the next start, Clear deletes leftovers and never a catalogue file, hard-linked and junctioned copies kept elsewhere survive',
      'Manual: Settings > AI and models, Download on a model that is not downloaded shows progress, then Downloaded; use its feature and nothing downloads.',
      'Manual: Settings > AI and models, under Generative AI, lists the models with sizes; delete one, use its feature, and it downloads again.',
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
        note: 'The same fields on the GPU side, read by ag_stage_tone (modules.wgsl), which replaces '
          + 'their tone mapper’s result for a RAW when a curve is set. Until 2026-10-17 that was two '
          + 'blocks inside their main(). Their struct and ours must stay in step.' },
      { file: 'src/utils/adjustments.ts', key: 'rawToneRendering', how: 'extends',
        note: 'rawToneRendering and rawToneCurve in their adjustments type and defaults; their loader '
          + 'keeps them by spreading the saved edit.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'embedded_preview_fallback', how: 'calls',
        note: 'Auto-Matched compares the RAW with this. Kept crate-visible; 1.6.5 dropped its path '
          + 'argument and now finds the preview through raw_processing::extract_embedded_preview.' },
      { file: 'src-tauri/src/raw_processing.rs', symbol: 'develop_raw_image', how: 'calls',
        note: 'The scene-linear RAW the curve is fitted against.' },
      { file: 'src/components/panel/right/ControlsPanel.tsx', how: 'calls',
        note: 'Rendered in the Profile section, which ControlsPanel shows first.' },
    ],
    tests: ['src-tauri/src/mods/raw_tone.rs #[cfg(test)]'],
    keywords: /tone.?(curve|map)|base.?curve|embedded.?(jpe?g|preview)|agx|filmic/i,
  },
  {
    id: 'dng-standard',
    kind: 'feature',
    what:
      'DNG Standard, a fourth RAW tone rendering, opt-in: the camera’s exposure offset '
      + '(BaselineExposure, from a DNG or a per-body table taken from RAWmakase) and Adobe’s default '
      + 'tone curve, applied in linear ProPhoto to each pixel’s largest and smallest channel as '
      + 'Adobe’s RGBTone does, so hues hold. Linear, a fifth, is the same without the tone curve: '
      + 'the offset alone, the base a film-simulation LUT is made for.',
    ours: [
      'src-tauri/src/mods/dng_standard.rs', 'src-tauri/src/mods/dng_baseline.tsv',
      'src-tauri/src/shaders/modules.wgsl', 'src/argentum/RawToneRendering.tsx',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'default_tonemapped', how: 'extends',
        note: 'One call, ag_stage_tone, right after their tone-mapper chain, which fills default_tonemapped. It is handed their result '
          + 'and the scene-linear colour, and returns theirs unless a RAW tone rendering is set. If '
          + 'upstream moves their tone mapping, the call has to move with it, or ours runs on the '
          + 'wrong colour.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_curve', how: 'calls',
        note: 'The 16 points are read with their monotone spline. mods/dng_standard.rs tests the '
          + 'points against a copy of it, so a change to their spline must be copied there too.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'linear_to_srgb', how: 'calls',
        note: 'The final encode, which is also the per-channel clip of colours beyond sRGB.' },
      { file: 'src/utils/adjustments.ts', key: 'rawToneRendering', how: 'extends',
        note: 'dngStandard in their type and in the loader’s list of values it keeps.' },
    ],
    tests: [
      'src-tauri/src/mods/dng_standard.rs #[cfg(test)]',
      'src-tauri/src/mods/shader_check.rs #[cfg(test)]',
    ],
    keywords: /baseline.?exposure|dng.?(render|sdk|standard)|acr3|camera.?raw|prophoto|rgb.?tone/i,
  },
  {
    id: 'raw-starting-look',
    kind: 'behaviour-change',
    what:
      'A RAW whose edit names no rendering - one nobody has edited, one reset, settings pasted '
      + 'onto an unedited photo - starts in DNG Standard, which brings Sharpen 25 at radius 1 until '
      + 'someone sets their own. Edits that name RapidRAW’s rendering keep it; edits saved '
      + 'before 2026-09-22 name none and now open in DNG Standard. Read the same everywhere: '
      + 'raw_tone::mode_of and sharpen::from_json for the renderers, INITIAL_ADJUSTMENTS and their '
      + 'loader for the editor.',
    ours: [
      'src-tauri/src/mods/raw_tone.rs', 'src-tauri/src/mods/sharpen.rs',
      'src-tauri/src/mods/dng_standard.rs', 'src-tauri/src/mods/super_resolution.rs',
      'src-tauri/src/mods/cache_version.rs', 'src/argentum/sharpenSettings.ts',
      'src/argentum/Sharpening.tsx', 'src/argentum/RawToneRendering.tsx',
    ],
    dependsOn: [
      { file: 'src/utils/adjustments.ts', key: 'rawToneRendering', how: 'extends',
        note: 'INITIAL_ADJUSTMENTS starts it at dngStandard, and their loader fills a saved edit '
          + 'without it from INITIAL_ADJUSTMENTS, as mode_of does.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'raw_tone_mode', how: 'calls',
        note: 'Their get_global_adjustments_from_json asks raw_tone::mode_of for the mode and '
          + 'raw_tone::fill_missing for the photo’s points.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'is_image_edited', how: 'calls',
        note: 'Compares an edit against json!({}), which now reads as DNG Standard: a photo nobody '
          + 'edited still counts as unedited; an edit naming RapidRAW’s rendering now counts as edited.' },
      { file: 'src-tauri/src/file_management.rs', symbol: 'reset_adjustments_for_paths', how: 'calls',
        note: 'Their reset writes an empty edit, which now renders as the new start, as the '
          + 'editor’s reset to INITIAL_ADJUSTMENTS does.' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'load_sidecar', how: 'calls',
        note: 'A photo without a sidecar has no adjustments, which every renderer reads as the new '
          + 'start. If upstream ever fills in a rendering here, unedited RAWs lose it.' },
    ],
    tests: [
      'src-tauri/src/mods/raw_tone.rs #[cfg(test)]',
      'src-tauri/src/mods/sharpen.rs #[cfg(test)]',
      'src-tauri/src/mods/super_resolution.rs #[cfg(test)]',
    ],
    keywords: /default.?(look|render|sharpen|preset)|initial.?adjust|unedited|reset/i,
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
      + 'curve, as Lightroom draws them, always, with the row spacing and the text size chosen in '
      + 'Settings > General > Panels. Their slider and curve are restyled from a stylesheet of ours; no '
      + 'line of either changes.',
    ours: [
      'src/argentum/compactSliders.css',
      'src/argentum/compactSliders.ts',
      'src/argentum/PanelsSetting.tsx',
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
  {
    id: 'sharpening',
    kind: 'feature',
    what:
      'Sharpening: RawTherapee capture sharpening (Richardson-Lucy deconvolution, auto radius read '
      + 'from the RAW), darktable\'s sharpen (unsharp mask on L), and RawTherapee\'s contrast mask '
      + 'deciding where either lands, with a view of that mask. Whole-image GPU passes before their '
      + 'shader, through the input-stage anchor. Replaces their Sharpness slider in Details, whose '
      + 'radius grew with the sensor size, and their local Sharpness in a mask\'s Details with a '
      + 'Sharpen of its own on the same engine - its own radius, threshold and contrast mask, which '
      + 'nothing in the main card reaches.',
    ours: [
      'src-tauri/src/mods/input_stage.rs',
      'src-tauri/src/mods/sharpen.rs',
      'src-tauri/src/mods/sharpen_gpu.rs',
      'src-tauri/src/shaders/sharpen.wgsl',
      'src-tauri/src/shaders/modules.wgsl',
      'src-tauri/src/mods/clipping.rs',
      'src-tauri/src/mods/decode.rs',
      'src-tauri/src/mods/startup.rs',
      'src-tauri/src/mods/export_precision.rs',
      'src-tauri/src/mods/dispatch.rs',
      'src/argentum/Sharpening.tsx',
      'src/argentum/LocalSharpening.tsx',
      'src/argentum/sharpenSettings.ts',
      'src/argentum/sharpening.css',
      'src/argentum/Argentum.tsx',
    ],
    dependsOn: [
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'process_and_get_dynamic_image_inner', how: 'calls',
        note:
          'The input-stage anchor: one call after their input texture and guided-filter cache exist, '
          + 'and their processor.run handed staged.as_ref().unwrap_or(&cache.texture_view). Everything '
          + 'of theirs downstream - main pass, sharpness/clarity blurs, flare - reads the sharpened '
          + 'texture; the guided-filter coefficients stay built from the unsharpened one. Called under '
          + 'their processor lock, which is what makes reusing one output texture between frames safe.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'TILE_OVERLAP', how: 'calls',
        note:
          'With a region of interest only that region is sharpened, widened by 160 px because their '
          + 'tiles read 128 px past it. Raise their overlap past 160 and the edge of a zoomed drag '
          + 'reads unsharpened texels.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'ag_sharpen (sharpen::Params, 560 bytes) at the end of their struct, filled by '
          + 'sharpen::from_json. The settings reach the stage the way every other adjustment reaches a render.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'calls',
        note:
          'Each mask\'s Sharpen is placed at the index their shader gives the mask: the masks that are '
          + 'visible and have a sub-mask, in order, up to MAX_MASKS. sharpen::locals repeats that '
          + 'rule on the JSON. Change their filter and the settings land on the wrong masks.' },
      { file: 'src-tauri/src/mask_generation.rs', symbol: 'generate_mask_bitmap', how: 'calls',
        note:
          'The render carries the mask bitmaps their shader reads, built by the same rule (None for a '
          + 'hidden or empty mask), so bitmap i is mask i. The stage uploads the ones with a Sharpen of '
          + 'their own and reads them as their get_mask_influence does: the red channel, opacity and '
          + 'invert already in it.' },
      { file: 'src/store/useEditorStore.ts', symbol: 'activeMaskContainerId', how: 'calls',
        note:
          'LocalSharpening.tsx edits the mask their Masks panel is editing, found the way their panel '
          + 'finds it: adjustments.masks by activeMaskContainerId.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'AgSharpen mirrored at the end of their WGSL struct, never read there. '
          + 'shader_check::the_adjustments_have_the_same_layout_on_both_sides fails if the sides drift.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'absolute_coord', how: 'calls',
        note: 'Passed to ag_stage_display, which in modes 7 to 9 reads input_texture at that pixel: the mask '
          + 'the stage wrote there, unchanged by every adjustment in between.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_sharpen', how: 'shadows',
        note:
          'Their sharpening stays in their shader and still runs for an old edit with a sharpness '
          + 'value, global or in a mask. Both its sliders are hidden; each card offers to remove an '
          + 'old value. Fixes to it upstream do nothing for a new edit.' },
      { file: 'src/components/adjustments/Details.tsx', how: 'calls',
        note:
          'data-argentum="sharpening" as the first child of their Sharpening section, data-mask in a '
          + 'mask\'s Details, where LocalSharpening.tsx mounts. sharpening.css hides its siblings - '
          + 'their sliders - only once our card has mounted into it. Move their sliders out of that '
          + 'section and they reappear.' },
      { file: 'src/utils/adjustments.ts', symbol: 'normalizeLoadedAdjustments', how: 'calls',
        note:
          'agSharpen is a key their type does not declare, written only once a setting is changed so '
          + 'the thumbnail cache hash of an untouched photo does not move. It survives a reload because '
          + 'their loader spreads the saved edit over the defaults. If the loader starts picking keys, '
          + 'every photo falls back to the defaults.' },
      { file: 'src-tauri/src/app_state.rs', symbol: 'full_transformed_cache', how: 'calls',
        note:
          'Radii are in full-resolution pixels. A render whose transform hash matches the cached '
          + 'full-resolution image is the editor preview at width / full width; anything else is 1:1. '
          + 'The full image is also where auto contrast is measured, as RawTherapee measures on the RAW.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'generate_transformed_preview', how: 'calls',
        note: 'Assumed: the preview is that cached full-resolution image downscaled, under the same transform hash.' },
      { file: 'src-tauri/src/export_processing.rs', symbol: 'export_lut', how: 'calls',
        note:
          'Caller names the stage skips: their LUT export renders an identity cube as an image, and '
          + 'sharpening it would bend the exported LUT. Also generate_thumbnail_data and the other '
          + 'small previews in input_stage::NOT_SHARPENED. A renamed caller gets sharpened at 1:1.' },
      { file: 'src-tauri/src/app_settings.rs', symbol: 'raw_preprocessing_sharpening', how: 'calls',
        note: 'Set to 0 once (2026-10-10): their load-time box-blur pre-sharpening would stack '
          + 'on capture sharpening. A user who turns it back up keeps it.' },
      { file: 'src-tauri/src/image_loader.rs', symbol: 'remove_raw_artifacts_and_enhance', how: 'calls',
        note: 'Where that pre-sharpening runs. If upstream makes it unconditional, the stacking is back.' },
    ],
    tests: [
      'src-tauri/src/mods/sharpen.rs #[cfg(test)]',
      'src-tauri/src/mods/sharpen_gpu.rs #[cfg(test)], and with --ignored on a GPU: deconvolution, '
        + 'darktable\'s formula, the mask, a mask\'s own Sharpen, regions, the whole render, and the '
        + 'software adapter (WARP), and on OpenGL - including another thread using the GPU while '
        + 'it runs, which crashed the app on GL before submissions were made small',
      'src-tauri/src/mods/shader_check.rs the_adjustments_have_the_same_layout_on_both_sides',
    ],
    keywords: /sharpen|sharpness|deconvol|unsharp|capture.?sharp|richardson|lucy|detail|halo/i,
  },
  {
    id: 'engine-contract',
    kind: 'feature',
    what:
      'The engine library and the contract the new interface talks to it through. The TypeScript '
      + 'types and fixtures in src/shell/ are generated from src-tauri/engine/src/contract.rs, so '
      + 'Prettier is told to leave them alone: a formatted copy would be stale at once.',
    ours: [
      'src-tauri/engine/',
      'src/shell/contract.generated.ts',
      'src/shell/contract.fixtures.json',
    ],
    dependsOn: [
      { file: '.prettierignore', how: 'extends',
        note: 'Two lines naming the generated files. If upstream moves or replaces the ignore file, '
          + 'npm run format:check starts flagging them again.' },
    ],
    tests: ['src-tauri/engine/tests/contract.rs the_generated_files_are_current'],
    keywords: /prettier|format|lint/i,
  },
  {
    id: 'new-shell-switch',
    kind: 'feature',
    what:
      'The new editor interface (DEC-29), opt-in: `?shell=new` opens src/shell/ instead of their App. '
      + 'Panels are built from the engine\'s tool descriptors; it reaches the engine only through '
      + 'src/shell/engine/client.ts: the real engine inside the app (engine-in-app), the contract fake '
      + 'in a plain browser. Anything else opens their App as before.',
    ours: ['src/shell/'],
    dependsOn: [
      { file: 'src/main.tsx', how: 'calls',
        note:
          'Their `import App from \'./App\'` now imports from ./shell/switch, one line for one. The '
          + 'switch loads their App with a dynamic import unless the shell is asked for, because their '
          + 'App reaches for Tauri while its modules load. If they rename or move App.tsx, or render '
          + 'something other than <App /> from main.tsx, the switch is what breaks.' },
    ],
    tests: [
      'npm run test:shell (src/shell/tests)',
      'Manual: the app opens as before without ?shell=new; with it, the shell opens, on the contract '
        + 'fake in a plain browser.',
    ],
    keywords: /main\.tsx|createRoot/i,
  },
  {
    id: 'dev-watch-ignores',
    kind: 'behaviour-change',
    what:
      'A running dev app no longer restarts or reloads because of agents\x27 work folders (under '
      + '.claude/) or the engine crate\x27s own build output (src-tauri/engine/target).',
    dependsOn: [
      { file: 'src-tauri/.taurignore', how: 'adds',
        note: 'A new file in their src-tauri/: tauri dev reads it to leave engine/target/ out of its watch.' },
      { file: 'vite.config.mjs', how: 'extends',
        note:
          'Adds **/.claude/** to their server.watch.ignored, next to their **/src-tauri/**. '
          + 'Without it every new worktree reloaded the page of a running dev app.' },
    ],
    tests: ['By hand: create a worktree under .claude/worktrees while tauri dev runs; the page does not reload.'],
  },
  {
    id: 'dev-warmup',
    kind: 'behaviour-change',
    what:
      'The dev window opens on a ready page: Vite transforms the app while cargo builds, instead of '
      + 'on the first request, so the frontend reports ready before the 4-second failsafe.',
    dependsOn: [
      { file: 'vite.config.mjs', how: 'extends',
        note:
          'Adds server.warmup.clientFiles. Measured on a freshly started server: frontend_ready at '
          + '2.6-10.2 s without it, 0.6 s with it.' },
    ],
    tests: ['By hand: npm start; the log has no "Frontend failed to report ready within timeout".'],
  },
  {
    id: 'fast-dev-builds',
    kind: 'behaviour-change',
    what:
      'Dev and test builds do less: the app library is built as an rlib only, Windows x64 dev and '
      + 'test builds link with rust-lld, dependencies carry no debug info in dev builds, and '
      + 'build.rs neither hashes again nor downloads an ONNX Runtime library already on this disk. '
      + 'Shipped builds are unchanged: release profile, MSVC linker. npm start leaves the '
      + 'mergeability check to CI and npm run check:merge.',
    ours: ['.cargo/config.toml', 'src-tauri/src/mods/build_ort.rs'],
    dependsOn: [
      { file: 'src-tauri/Cargo.toml', how: 'replaces',
        note:
          'crate-type is ["rlib"]; theirs is ["staticlib", "cdylib", "rlib"]. The binary links the '
          + 'rlib, and the other two were written by every build for mobile targets Argentum does not '
          + 'build (ci-desktop-only): a 640 MB staticlib and a cdylib with its own link. A mobile '
          + 'build needs theirs back - cdylib for Android, staticlib for iOS. Also debug = false in '
          + 'their [profile.dev.package."*"]: dependencies lose file and line in a backtrace, keep '
          + 'their function names, and a full rebuild of them took 368 s instead of 528 s.' },
      { file: 'src-tauri/build.rs', how: 'calls',
        note:
          'Includes mods/build_ort.rs with #[path] and asks build_ort::present before their own '
          + 'check, which then runs only when the answer is no. Relies on their dest_path and '
          + 'expected_hash: if they move the library or the hash, present looks at the wrong file '
          + 'and their download runs as before.' },
      { file: '.github/workflows/build.yml', how: 'extends',
        note:
          'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=link.exe on their tauri-action step, so a '
          + 'shipped Windows x64 build links with MSVC as before; the variable wins over '
          + '.cargo/config.toml.' },
      { file: 'package.json', how: 'replaces',
        note:
          'Their start is `tauri dev`; ours runs check-identity first. It ran check-mergeability '
          + 'too, about 45 s before every dev window; that check is the required one in '
          + 'upstream.yml and runs by hand with npm run check:merge.' },
    ],
    tests: [
      'Measured on Windows x64 before and after (pull request "Faster dev and test builds"): a '
        + 'one-line change in mods/sharpen.rs, then cargo check, cargo test --lib sharpen::, '
        + 'cargo build; and a cold build at a new target directory.',
      'Manual: delete src-tauri/resources/onnxruntime.dll in a worktree and build - the log says '
        + 'it was copied from the main checkout, and its SHA-256 is the pinned one.',
    ],
    keywords: /crate.?type|staticlib|cdylib|linker|lld|profile\.dev|debuginfo|build\.rs|onnxruntime/i,
  },
  {
    id: 'engine-in-app',
    kind: 'feature',
    what:
      'The engine inside the app, for the new interface: its calls through the one `ag` command '
      + '(mods/engine_bridge.rs), its events as one Tauri event, frames as a WebView2 shared buffer '
      + 'on Windows and raw IPC bytes elsewhere (mods/engine_frame.rs). Opens a RAW from a file '
      + 'dialog or from the path the app was started with; the filmstrip is that photo\'s folder '
      + '(mods/shell_photos.rs). Writes nothing: edits stay in memory, .agdata files are never read '
      + 'or written. Shows the window itself rather than through their frontend_ready, so it rests '
      + 'on no file of theirs. Their Cargo.toml (identity) gains the engine and webview2-com, both '
      + 'already built with the app: the engine shares their rawler, wgpu, rayon and serde, '
      + 'webview2-com is Tauri\'s own.',
    ours: [
      'src-tauri/src/mods/engine_bridge.rs',
      'src-tauri/src/mods/engine_frame.rs',
      'src-tauri/src/mods/shell_photos.rs',
      'src/shell/engine/tauri.ts',
      'src/shell/engine/AppBar.tsx',
      'scripts/start-shell.mjs',
    ],
    dependsOn: [],
    tests: [
      'src-tauri/src/mods/engine_bridge.rs #[cfg(test)]: open, apply, view, frame and latest-wins on '
        + 'a real RAW (AG_RAW); refusals answered as the contract fixtures\' EngineErrors; photos past '
        + 'the limit let go and reopened with their edits',
      'src-tauri/src/mods/engine_frame.rs and dispatch.rs: the engine\'s frameBlock fixture as the '
        + 'block sent, every call\'s arguments as the interface sends them',
      'src/shell/tests/tauriClient.test.ts: the client\'s calls, the frameBlock fixture read as bytes '
        + 'and as a shared buffer handed back once, and EngineErrors passed through',
      'Manual: npm run start:shell opens the new interface; Open picks a RAW, its folder fills the '
        + 'filmstrip, and the top bar says edits are not saved.',
    ],
    keywords: /shared.?buffer|webview2|binary ipc|ipc::response/i,
  },
  {
    id: 'editor-layout-modern',
    kind: 'feature',
    what:
      'The Modern window, the only arrangement (DEC-31, DEC-33): the whole app in one '
      + 'window built from today\'s own panels and stores. One bar per edge: a top bar that is also the '
      + 'title bar (folder history, an editable path, the grid\'s or the photo\'s view tools, panel '
      + 'toggles, a Settings gear that never hides, window controls); a left column with the folders '
      + '(Favourites, Recent, This computer with every disk, each with its photo count, and the '
      + 'tree keys), the Navigator (docked there or floating over the photo) and the history; '
      + 'the centre showing the '
      + 'folder\'s grid or the open photo; the filmstrip under the centre with the status line in its '
      + 'header; one right panel with the histogram, an Info line (ISO, aperture, shutter, focal length) that opens the metadata at full height, and six tabs over today\'s panels '
      + '(Edit, Crop, Looks, Masks, Retouch, Export). No start screen: it opens on the folder last open. '
      + 'Settings opens in the centre with its own section list. Drawn in its own palettes (Grey, Deep, '
      + 'Light) and text sizes (Small, Normal, Large) by modern.css. The Layout choice is gone, and so is '
      + 'Classic\'s arrangement: the Modern window draws at every window size.',
    ours: [
      'src/argentum/modern/ModernWindow.tsx',
      'src/argentum/modern/TopBar.tsx',
      'src/argentum/modern/WindowControls.tsx',
      'src/argentum/modern/Menu.tsx',
      'src/argentum/modern/LeftColumn.tsx',
      'src/argentum/modern/RecentFolders.tsx',
      'src/argentum/modern/RightPanel.tsx',
      'src/argentum/modern/gridAdjustments.ts',
      'src/argentum/modern/FilmstripArea.tsx',
      'src/argentum/modern/ModernEmpty.tsx',
      'src/argentum/modern/ModernSettings.tsx',
      'src/argentum/modern/ModernThemeChoice.tsx',
      'src/argentum/modern/folders.ts',
      'src/argentum/modern/folderCounts.ts',
      'src/argentum/modern/treeKeys.ts',
      'src/argentum/modern/NavigatorDock.tsx',
      'src/argentum/FoldingPane.tsx',
      'src/argentum/modern/look.ts',
      'src/argentum/modern/modern.css',
      'src/argentum/modern/index.ts',
      'src/argentum/layout/Navigator.tsx',
      'src/argentum/layout/HistogramSection.tsx',
      'src/argentum/layout/HistoryPanel.tsx',
      'src/argentum/layout/historyNames.ts',
      'src/argentum/layout/editorLayout.ts',
      'src-tauri/src/mods/editor_layout.rs',
      'src-tauri/src/mods/drives.rs',
      'src-tauri/src/mods/recent_folders.rs',
      'src-tauri/src/mods/modern_start.rs',
      'src-tauri/src/mods/folder_counts.rs',
    ],
    dependsOn: [
      { file: 'src/App.tsx', how: 'calls',
        note: 'One import line from src/argentum/modern. App renders ModernWindow at every window '
          + 'size and on every platform, passing its own handlers on; Classic\'s title bar, side panels, '
          + 'views and narrow-window arrangement are gone. Its tree refresh is refreshModernTrees and '
          + 'unfolding a folder counts no photos, since the roots are whole disks.' },
      { file: 'src/components/views/EditorView.tsx', how: 'replaces',
        note: 'Deleted: Classic\'s editor view (the photo over its bottom bar, or the compact panel '
          + 'for a narrow window). The Modern window draws Editor.tsx itself.' },
      { file: 'src/components/panel/SidePanelArea.tsx', how: 'replaces',
        note: 'Deleted: Classic\'s left and right panel areas, split in two, with tabs dragged '
          + 'between them. The Modern window\'s left column and right panel hold the same panels.' },
      { file: 'src/components/panel/PanelSwitcher.tsx', how: 'replaces',
        note: 'Deleted: the tab strips of those areas and the narrow window\'s panel bar. The tabs '
          + 'are RightPanel.tsx\'s.' },
      { file: 'src/window/TitleBar.tsx', how: 'replaces',
        note: 'Deleted: Classic\'s title bar (the app name and the window controls). The Modern top bar '
          + 'is the title bar; its window controls are WindowControls.tsx, adapted from this file. Its '
          + 'one rebranded line, the name Argentum, went with it.' },
      { file: 'src/components/panel/BottomBar.tsx', how: 'replaces',
        note: 'Deleted: Classic\'s bar under the grid and the photo (stars and flags, copy and paste, '
          + 'the quick filter, zoom, panel toggles, the filmstrip). The Modern window has the filmstrip '
          + '(FilmstripArea.tsx), copy and paste in the Edit header\'s menu, zoom and the panel toggles '
          + 'in the top bar. It carried the 1.6.4 catch-up\'s Quick Filter persistence, which went with it; '
          + 'uiVisibility.quickFilter is left in their store and types.' },
      { file: 'src/components/views/LibraryView.tsx', how: 'extends',
        note: 'No bottom bar (deleted with Classic), and MainLibrary in its modern form.' },
      { file: 'src/components/panel/MainLibrary.tsx', how: 'replaces',
        note: 'The grid alone: its header (path, grid/list/cull switch, search, view options, community '
          + 'and home buttons) and its start screen (splash, continue, open folder, settings, version '
          + 'and update check) are deleted; the top bar, the folder column and Settings hold what the '
          + 'window uses. ModernEmpty until a folder is picked.' },
      { file: 'src/components/panel/library/LibraryHeader.tsx', how: 'replaces',
        note: 'Deleted: the search box and view options of that header. The top bar\'s Sort and View '
          + 'menus set the same sortCriteria and filterCriteria; the search has no home in the Modern '
          + 'window, and Ctrl+F still focuses the Community page\'s own search.' },
      { file: 'src/components/panel/Editor.tsx', how: 'extends',
        note: 'Drawn as the Modern window wants it, always: no toolbar (its tools are in the top bar) and '
          + 'a square frame with 20 px round the photo, where the frameless prop used to choose. The '
          + 'transform handle on transformWrapperRef is what the Navigator reads.' },
      { file: 'src/components/panel/right/FolderTree.tsx', how: 'extends',
        note: 'Under Modern: a slim FOLDERS header with search and the options menu, Favourites (the '
          + 'pinned folders, starred), Recent, This computer with each disk\'s name and free space; no '
          + 'albums, no Add folder, no Sources header. Each row shows its photos from folderCounts.ts '
          + 'and is reachable from the keyboard (treeKeys.ts: a depth prop and data attributes on '
          + 'the row, the handler on the scroller); no sorting by count, since counts arrive late.' },
      { file: 'src/components/panel/right/ControlsPanel.tsx', symbol: 'readOnly', how: 'extends',
        note: 'Optional props: showWaveform, and readOnly, the selected photo\'s settings shown inert in the '
          + 'grid. The section reset button is ours (.ag-section-reset), shown only in Modern.' },
      { file: 'src/components/panel/right/MasksPanel.tsx', symbol: 'showWaveform', how: 'extends',
        note: 'Same as ControlsPanel\'s showWaveform.' },
      { file: 'src/components/panel/right/CropPanel.tsx', how: 'extends',
        note: 'Its five panes (Aspect Ratio, Rotation, Orientation, Transform, Lens Correction) are '
          + 'FoldingPane.tsx: their heading with a chevron, folding the pane, the buttons beside it '
          + 'kept. A sixth pane of theirs would not fold until added there. The Transform and Lens '
          + 'Correction cards no longer fold inside their panes: isTransformExpanded and isLensExpanded '
          + 'are always true, and their cropSectionsState is no longer read.' },
      { file: 'src/components/panel/right/MetadataPanel.tsx', symbol: 'photo', how: 'extends',
        note: 'Optional prop: a photo not open in the editor, the grid\'s selection, shown from the '
          + 'path and EXIF the library already read. Without it, the open photo as before.' },
      { file: 'src/components/panel/SettingsPanel.tsx', symbol: 'modernCategory', how: 'extends',
        note: 'Optional prop: the Modern section to show, without the page\'s own header; AI and models '
          + 'is the General page\'s AI card plus the data-argentum="ai" slot. The Theme row offers the '
          + 'Modern palettes there. The Layout row still comes from the General slot. Their folder '
          + 'image count switch is hidden in Modern, which counts its own way.' },
      { file: 'src/components/panel/Filmstrip.tsx', how: 'calls',
        note: 'Drawn as it is under the centre, in both views.' },
      { file: 'src/components/panel/editor/EditorToolbar.tsx', how: 'replaces',
        note: 'Deleted: the toolbar over the photo, which the Modern window kept mounted but hidden. '
          + 'Its undo, redo and Before are in the top bar, History lists the steps and Esc goes back '
          + 'to the grid. Its step names live in layout/historyNames.ts; a release that changes their '
          + 'naming is copied there by hand. bench/replay.js clicks the data-bench-id hooks that the '
          + 'top bar and ModernWindow.tsx now carry.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'handleZoomChange', how: 'calls',
        note: 'The top bar\'s Zoom menu and the Navigator, in the units today\'s zoom control uses; '
          + 'toggleShowOriginal for Before.' },
      { file: 'src/store/useUIStore.ts', symbol: 'movePanel', how: 'replaces',
        note: 'Deleted with Classic\'s side panel areas: movePanel, movePanelToIndex, setLayoutDragItem and '
          + 'setPanelSwitcherPlacement, which moved tabs between areas. panelLayout, panelSwitcherPlacement '
          + 'and activeLayoutDragItem stay: the saved workspace still carries them, setPanel finds a panel '
          + 'through panelLayout, and FolderTree.tsx still reads activeLayoutDragItem (now always null).' },
      { file: 'src/store/useUIStore.ts', symbol: 'activePanel', how: 'calls',
        note: 'The tabs read and set activePanel, as today\'s D, P, R, M, K and E keys do; the panel '
          + 'toggles are uiVisibility, the widths leftPanelWidth, rightPanelWidth and bottomPanelHeight.' },
      { file: 'src/store/useLibraryStore.ts', symbol: 'sortCriteria', how: 'calls',
        note: 'The top bar\'s Sort and View menus set today\'s sortCriteria and filterCriteria.' },
      { file: 'src/components/panel/editor/Waveform.tsx', how: 'calls',
        note: 'The Histogram section draws it with the props and store values ControlsPanel gives it.' },
    ],
    tests: [
      'src-tauri/src/mods/editor_layout.rs #[cfg(test)]: Classic until chosen, Grey until another '
        + 'palette is, choices read back, unknown names fall back, other preferences untouched.',
      'src-tauri/src/mods/{drives,recent_folders,modern_start}.rs #[cfg(test)]: disks listed with '
        + 'their size, recent folders newest first and capped, where the column opens.',
      'src-tauri/src/mods/folder_counts.rs #[cfg(test)]: only photos directly inside count; a '
        + 'missing folder holds none. editor_layout.rs: panels saved before History and the floating '
        + 'Navigator still read.',
      'Manual: the window opens on the last folder with no start '
        + 'screen; the gear opens Settings in the centre (Esc closes, Ctrl+, opens) with AI and models, '
        + 'My Gear and About; Ctrl+Shift+B, Ctrl+B and Ctrl+J hide the panels and the top bar brings '
        + 'them back; double-click a photo and only the centre changes, Esc goes back; every tab opens '
        + 'today\'s tool; the grid shows the selected photo\'s sliders read-only. Settings > General > '
        + 'Panels > Text size makes the panels\' text smaller or larger at once.',
    ],
    keywords: /layout|workspace|side.?panel|panel.?switcher|toolbar|navigator|histogram|waveform|filmstrip|title.?bar|settings/i,
  },
  {
    id: 'lens-builtin',
    kind: 'feature',
    what:
      'The camera\'s built-in lens correction: the distortion, lateral chromatic aberration and '
      + 'vignetting tables a Fujifilm RAF (FujiIFD 0xF00B, 0xF00F, 0xF010) or a Sony ARW (raw SubIFD '
      + '0x7032, 0x7035, 0x7037) stores per shot, or a DNG holds as FixVignetteRadial and '
      + 'WarpRectilinear opcodes (dng_opcodes.rs), read by the engine (rm_lens_builtin.rs, from '
      + 'RAWmakase @ a8c5d6d7) and applied in the lens warp in place of the lensfun profile while its '
      + 'switch, at the top of Crop > Lens Correction, is on. Fujifilm\'s is on for a photo with no '
      + 'edit yet (an edit saved earlier keeps its look until switched on); Sony\'s starts off and '
      + 'keeps the camera\'s framing (fill_scale_within over the developed image); a DNG\'s starts on.',
    ours: [
      'src-tauri/engine/src/rm_lens_builtin.rs',
      'src-tauri/engine/src/dng_opcodes.rs',
      'src-tauri/src/mods/lens_builtin.rs',
      'src/argentum/lensBuiltin.ts',
      'src/argentum/LensBuiltinSwitch.tsx',
    ],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_geometry_params_from_json', how: 'extends',
        note: 'GeometryParams.lens_builtin, read from the edit; lensfun\'s numbers are left out while it is '
          + 'set. warp_image_geometry, unwarp_image_geometry, inverse_transform_point and '
          + 'is_geometry_identity each apply it where lensfun\'s distortion is applied. Render path.' },
      { file: 'src-tauri/src/lib.rs', symbol: 'calculate_transform_hash', how: 'calls',
        note: 'The preview\'s geometry caches fold lensBuiltin and lensBuiltinParams in '
          + '(lens_builtin::fold), since GEOMETRY_KEYS in their cache_utils.rs has no room for them. '
          + 'Their thumbnail and AI-mask caches key on GEOMETRY_KEYS alone, so after the switch is '
          + 'flipped those keep the old geometry until another geometry change. Render path.' },
      { file: 'src/components/panel/right/CropPanel.tsx', how: 'extends',
        note: 'The switch above Auto/Manual; the lensfun choice greyed while it is on; the Distortion, '
          + 'Chromatic Aberration and Vignetting rows read the camera\'s tables then (Vignetting also '
          + 'for a DNG gain map); both resets put the switch back to the photo\'s default. Today\'s '
          + 'interface.' },
    ],
    tests: [
      'src-tauri/engine/src/rm_lens_builtin.rs #[cfg(test)]: the Fujifilm reader on a synthetic RAF, '
        + 'the Sony reader on synthetic ARWs (the ZV-1\'s padded tables, malformed counts), '
        + 'the DNG opcodes (RAWmakase\'s test, and a DNG with its vignetting in OpcodeList3), '
        + 'OpcodeList3 gain maps (RAWmakase\'s test, a DNG\'s map over its active area, each '
        + 'channel\'s gain where it samples, edits without a map), '
        + 'interpolation, fill scale and Sony\'s camera-frame fill, amounts, the inverse map; '
        + 'a_real_files_tables (ignored) prints a real file\'s tables.',
      'src-tauri/src/mods/lens_builtin.rs #[cfg(test)]: only a switched-on edit with tables applies it, '
        + 'the panel\'s amounts, the hash fold; gain_map_sheet (ignored) renders a drone DNG without '
        + 'and with its gain map.',
    ],
    keywords: /lens|distortion|vignett|chromatic|fringe|fujifilm|raf|built.?in/i,
  },
  {
    id: 'lightroom-grading',
    kind: 'feature',
    what:
      'Color Grading as Lightroom renders it: RAWmakase\'s measured grading (engine '
      + 'rm_color_grade.rs @ 5a2414b4; its tables generated by scripts/rawmakase-grade-curves.mjs) '
      + 'for every block marked agLightroom: 1 inside its colorGrading, the photo\'s or a mask\'s, '
      + 'in ag_stage_grade (modules.wgsl), after the tone curve, Curves and a LUT. The panel marks a '
      + 'block the first time one that grades nothing is changed; Looks are marked as they are read. '
      + 'A block graded before has no mark and keeps their apply_color_grading, so old edits keep '
      + 'their look.',
    ours: [
      'src-tauri/engine/src/rm_color_grade.rs',
      'src-tauri/engine/src/rm_color_grade_curves.tsv',
      'src-tauri/src/mods/color_grade.rs',
      'src/argentum/colorGrading.ts',
      'scripts/rawmakase-grade-curves.mjs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'ag_stage_grade', how: 'calls',
        note: 'One call after their display-referred LUT and before grain. Render path.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'ag_grade, its last field (AgGrade in modules.wgsl); shader_check holds the sizes on '
          + 'both sides. Render path.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'apply_color_grading', how: 'shadows',
        note: 'Still runs for unmarked blocks. For a marked one color_grade::move_to_curves zeroes '
          + 'its settings in their uniforms first, so it adds nothing. Render path.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'extends',
        note: 'Fills ag_grade from the photo\'s block and each visible mask\'s, in their mask order; '
          + 'lightroom_grading reads a block back from their uniforms in Lightroom\'s units (SCALES). '
          + 'Render path.' },
      { file: 'src-tauri/src/gpu_processing.rs', symbol: 'ag_grade_curves_buffer', how: 'extends',
        note: 'Binding 15, the marked blocks\' curves, written in run() before the tiles. Render path.' },
      { file: 'src/components/adjustments/Color.tsx', symbol: 'ColorGradingPanel', how: 'extends',
        note: 'Both handlers pass the new block through markedGrading. Today\'s interface.' },
    ],
    tests: [
      'src-tauri/engine/src/rm_color_grade.rs #[cfg(test)]: the tables read whole; gains equal to '
        + 'RAWmakase\'s own at sampled points (matched bit for bit on 4,000 random gradings); '
        + 'RAWmakase\'s own tests of Balance, Blending, Luminance and channel independence.',
      'src-tauri/src/mods/color_grade.rs #[cfg(test)]: unmarked blocks keep their uniforms, marked '
        + 'ones move to curves, masks marked in shader order, Looks marked.',
      'scripts/test-rawmakase-grade-curves.mjs: the table is what the script makes from the release '
        + 'it names.',
      'Manual: on a photo with no grading, set Shadows Saturation 25: the tint reaches the mid-greys. '
        + 'Open a photo graded before this: it looks as it did.',
    ],
    keywords: /grad(e|ing)|split.?ton|tint|sepia/i,
  },
  {
    id: 'film-look-input',
    kind: 'behaviour-change',
    what:
      'The film looks that come with the app (spektrafilm LUTs, made for V-Log over V-Gamut) get '
      + 'the picture converted to V-Gamut before their V-Log encode, in ag_film_lut_input '
      + '(modules.wgsl), and a RAW in DNG Standard or Linear at the camera\'s exposure offset its '
      + 'curve carries (dng_standard::offset_of; an enlargement carries the number, '
      + 'agLutExposureEv), each channel first capped at the sensor\'s white in ProPhoto as DNG '
      + 'Standard caps it (ag_prophoto_to_white), so blown highlights print white, when the edit '
      + 'carries agLutDeclaredInput: true. The Looks tab writes it '
      + 'whenever it puts a LUT on or previews one, and Copy settings carries it; an edit that put '
      + 'a film look on before has no mark and keeps their input, linear sRGB under the curve.',
    ours: [
      'src-tauri/src/mods/film_lut.rs',
      'src-tauri/src/shaders/modules.wgsl',
      'src/argentum/looks/filmLookInput.ts',
      'src/argentum/looks/LooksPanel.tsx',
    ],
    dependsOn: [
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'linear_to_vlog', how: 'calls',
        note: 'Their scene-referred LUT path encodes ag_film_lut_input(composite_rgb_linear); '
          + 'shader_check holds the one call. If they move the encode, the call moves with it. '
          + 'Render path.' },
      { file: 'src-tauri/src/shaders/shader.wgsl', symbol: 'GlobalAdjustments', how: 'extends',
        note: 'ag_film_lut_input and ag_film_lut_ev take their _pad_raw_tone1 and _pad_raw_tone2 '
          + 'slots, so the layout is unchanged. '
          + 'Render path.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'get_all_adjustments_from_json', how: 'extends',
        note: 'Fills ag_film_lut_input and ag_film_lut_ev from the edit (film_lut::declared_input, '
          + 'exposure_ev, from the raw tone points it reads). Render path.' },
      { file: 'src-tauri/src/image_processing.rs', symbol: 'primaries_to_xyz_matrix', how: 'calls',
        note: 'The test that checks the shader\'s matrix builds it from the primaries; xy_to_xyz '
          + 'now takes a negative y (V-Gamut\'s blue) instead of treating it as missing. Render path.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'handleLutSelect', how: 'shadows',
        note: 'The Looks tab puts a LUT on and previews it as handleLutSelect and '
          + 'setLutPreviewOverride do, with the mark (filmLookInput.ts lutKeys); theirs are left for '
          + 'the Effects LUT block, which the Looks tab replaced. A key they add has to be added '
          + 'to lutKeys.' },
      { file: 'src-tauri/src/lut_processing.rs', symbol: 'generate_lut_previews', how: 'retypes',
        note: 'Its swatch edit, lutPath "preview", is read as a LUT going on now '
          + '(film_lut::declared_input), so the swatches show a film look as it goes on. If they '
          + 'rename the sentinel, the swatches show the old input.' },
      { file: 'src-tauri/src/app_settings.rs', symbol: 'all_available_adjustments', how: 'extends',
        note: 'The mark is pasted by default, on the line with our other keys (mods/copy_keys.rs).' },
      { file: 'src/utils/adjustments.ts', symbol: 'ADJUSTMENT_GROUPS', how: 'extends',
        note: 'filmLookInput.ts adds the mark to their LUT copy group and COPYABLE_ADJUSTMENT_KEYS at '
          + 'import, so presets and Copy settings carry it with the LUT. Found by the group\'s label, '
          + 'modals.copyPaste.groups.lut.' },
    ],
    tests: [
      'src-tauri/src/mods/film_lut.rs #[cfg(test)]: the shader\'s matrix is the one V-Gamut\'s and '
        + 'sRGB\'s primaries define; only a marked edit is fed it; through the whole render with an '
        + 'identity LUT, a marked edit is fed V-Log over V-Gamut and an unmarked one V-Log over sRGB '
        + '(GPU, ignored), and a RAW in DNG Standard or Linear at its offset (GPU, ignored); the '
        + 'offset reads back from either curve (dng_standard.rs); an enlargement carries it '
        + '(super_resolution.rs).',
      'src-tauri/src/mods/shader_check.rs #[cfg(test)]: the one call, and the struct layout.',
      'Manual: put K-Portra 400 on a photo, and open a photo that had a film look before: the new '
        + 'one is less saturated, the old one looks as it did.',
    ],
    keywords: /lut|film|v.?log|v.?gamut|spektrafilm|scene.?referred/i,
  },
  {
    id: 'history-on-release',
    kind: 'behaviour-change',
    what:
      'One History step per drag, recorded when the slider is let go, instead of one at every '
      + 'pause of 500 ms while it is still held.',
    ours: ['src/argentum/historyOnRelease.ts'],
    dependsOn: [
      { file: 'src/store/useEditorStore.ts', symbol: 'pushHistory', how: 'shadows',
        note: 'Replaced in the store at import by one that holds the step while isSliderDragging '
          + 'is on and calls theirs otherwise; the release records the held one. Relies on '
          + 'every drag of theirs setting isSliderDragging and clearing it on release.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'debouncedSetHistory', how: 'calls',
        note: 'Their debounce calls the store\'s pushHistory, so ours. On release it is flushed, '
          + 'so a change in the drag\'s last 500 ms is recorded at once rather than after it.' },
    ],
    tests: [
      'npx vitest run src/argentum/historyOnRelease.test.ts: nothing recorded while held, one step '
        + 'on release, a last change recorded once, an empty drag records nothing, a held step '
        + 'dropped when another photo opened, their 500 ms kept without a drag.',
      'Manual: drag LUT Intensity (or any slider), pausing a second or two without letting go, '
        + 'then release: History gains one step, and only on release. Undo takes the whole drag off.',
      'Manual: click a slider\'s track without dragging: one step. Type a value: one step.',
    ],
    keywords: /history|undo|redo|debounce/i,
  },
  {
    id: 'history-kept',
    kind: 'feature',
    what:
      'Each photo keeps its History: switching to another photo and back, or restarting, gives it back '
      + 'at the step it was at, so undo and redo carry on. Kept in the sidecar as agHistory beside the '
      + 'edit: each step its name and only what it changed (JSON Pointer to value), the step the edit '
      + 'is at, and a fingerprint of the saved edit, so it is used only with that edit. The model of '
      + 'darktable (src/common/exif.cc @ f973fb0f: history and history_end); RAWmakase keeps one in its '
      + 'catalog (develop_history.rs @ bf5346d), and its way of saving a change not yet a step.',
    ours: [
      'src/argentum/history/historyKeeper.ts',
      'src/argentum/history/historyRecord.ts',
      'src/argentum/layout/historyNames.ts',
      'src-tauri/src/mods/history.rs',
    ],
    dependsOn: [
      { file: 'src-tauri/src/image_processing.rs', symbol: 'ImageMetadata', how: 'extends',
        note: 'agHistory, an optional field beside adjustments, read leniently: one this version cannot '
          + 'read counts as none, never as an unreadable sidecar. Their save of the edit '
          + '(save_metadata_and_update_thumbnail) reads the sidecar and replaces only adjustments, which '
          + 'keeps it; if it starts writing a fresh record, History is lost on every save.' },
      { file: 'src-tauri/src/exif_processing.rs', symbol: 'write_sidecar', how: 'calls',
        note: 'History is written through it, so a sidecar left holding only defaults is not kept: '
          + 'History alone never gives a photo opened and left unedited a sidecar.' },
      { file: 'src/store/useEditorStore.ts', symbol: 'resetHistory', how: 'calls',
        note: 'Watched, not changed: a History of one state equal to the edit is their fresh start '
          + '(opening, reset, paste), which the kept one replaces through setState when it fits.' },
      { file: 'src/hooks/useEditorActions.ts', symbol: 'debouncedSave', how: 'calls',
        note: 'Their save comes 300 ms after the last change and their step 500 ms after it; History '
          + 'is written 600 ms after, so the two never write the sidecar at once. Shorter on their '
          + 'side is fine; longer than 600 ms and the two can overlap.' },
      { file: 'src/hooks/useAppNavigation.ts', symbol: 'handleImageSelect', how: 'calls',
        note: 'Flushes their save of the edit before the photo in the store changes, which is when '
          + 'History writes the photo left, so the sidecar holds the edit its History ends on.' },
    ],
    tests: [
      'cargo test --lib mods::history::: kept beside the edit, the rest of the sidecar untouched; only '
        + 'good for the edit it was written with (a rating keeps it, a changed edit drops it); an '
        + 'unedited photo gets no sidecar; one this version cannot read leaves the edit readable; '
        + 'the record the editor writes (src/argentum/history/record.fixture.json) reads as written.',
      'npx vitest run src/argentum/history/: round trip of states, step and names; a changed edit, or '
        + 'one the steps do not fit, starts fresh; the last 50 states kept, as many as the editor holds; '
        + '50 steps within 6,032 bytes plus a new mask once; the record matches the fixture; back after '
        + 'another photo with redo; written after their save; a change not yet a step kept as one; '
        + 'nothing written for an unedited photo; names as saved.',
      'Manual: edit a photo (a few sliders, undo one), open another and come back: History is as left, '
        + 'Redo works. Restart, open it: the same. Paste settings onto it from another photo: fresh History.',
    ],
    keywords: /history|undo|redo|agHistory/i,
  },
];

/**
 * The name a retirement uses for a review: a release review's `release`, or a
 * per-commit review's `through` sha.
 */
export const reviewName = (review) => review.release ?? review.through;

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
 * `through` of null means the window being prepared but not yet recorded. Every
 * recorded retirement is behind it, so retired entries are out.
 *
 * An unrecognised `recordedIn` keeps the entry live. It is not this function's
 * job to decide whether a retirement is real; validateRetirements says so, and
 * the safe reading in the meantime is that nothing has been retired.
 */
export function activeFor(entries, reviews, through) {
  const order = new Map(reviews.map((r, i) => [reviewName(r), i]));
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
 * record of it in that review.
 *
 * A retirement is how an entry stops being checked, so it is the obvious thing
 * to fake. It has to name a review that exists, and that review has to say so:
 * a release review lists the id in `retires`; a per-commit review, from before
 * 2026-10-18, carries a `retire:<id>` decision with a verdict and a reason.
 */
export function validateRetirements(entries, reviews) {
  const byName = new Map(reviews.map((r) => [reviewName(r), r]));
  const problems = [];
  for (const entry of entries) {
    if (!entry.retired) continue;
    const { recordedIn, why } = entry.retired;
    const review = byName.get(recordedIn);
    if (!review) {
      problems.push({
        id: entry.id,
        detail: `retired: { recordedIn: '${String(recordedIn).slice(0, 12)}' } names no review in REVIEWS`,
        fix: 'recordedIn is the `release` of the review that decided the retirement.',
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
    if (review.release) {
      if (!(review.retires ?? []).includes(entry.id)) {
        problems.push({
          id: entry.id,
          detail: `is retired in ${review.release}, and that review does not list it in retires`,
          fix: `Add '${entry.id}' to retires in the ${review.release} review. `
            + 'Retiring an entry is a decision and is recorded like one.',
        });
      }
      continue;
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
  // The other direction: a review naming an entry it did not retire.
  for (const review of reviews) {
    for (const id of review.retires ?? []) {
      const entry = entries.find((e) => e.id === id);
      if (entry?.retired?.recordedIn === review.release) continue;
      problems.push({
        id,
        detail: `is listed in the retires of ${review.release}, and the registry does not retire it there`,
        fix: `Set retired: { recordedIn: '${review.release}', why } on ${id}, or take it out of retires.`,
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
 * Every id that ever existed was added by some commit, so it appears as an added
 * line in that commit's patch. One `git log -p` reads them all; the first
 * version ran `git show` once per revision of this file, seventy and counting.
 * `-m` diffs each merge against its parents too: two entries in this file's
 * history arrived in a merge and nowhere else.
 */
export function historicalIds(run, path = 'scripts/upstream-registry.mjs') {
  let out = '';
  try {
    out = run(['log', '-m', '--format=', '-p', '--unified=0', '--no-renames', '--', path]);
  } catch {
    return null;
  }
  const ids = new Set();
  for (const m of out.matchAll(/^\+\s{4}id: '([A-Za-z0-9-]+)',\r?$/gm)) ids.add(m[1]);
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
  /** The ids of the entries that name this file, directly or by pattern. */
  const claimants = (file) => [...new Set([
    ...(byFile.get(file) ?? []).map(({ entry }) => entry.id),
    ...patterns.filter(({ dep }) => dep.pattern.test(file)).map(({ entry }) => entry.id),
  ])];
  return { byFile, patterns, prs, claims, claimants };
}

/** Upstream code we leave in place and step around. */
export const shadowsOf = (entries) => entries.flatMap((entry) => entry.dependsOn
  .filter((dep) => dep.how === 'shadows' && dep.symbol)
  .map((dep) => ({ entry, file: dep.file, symbol: dep.symbol, note: dep.note })));

/** Borrowed fixes, as {entry, file, pr}. */
export const borrowsOf = (entries) => entries.flatMap((entry) => entry.dependsOn
  .filter((dep) => dep.how === 'borrows' && dep.pr)
  .map((dep) => ({ entry, file: dep.file, pr: dep.pr })));
