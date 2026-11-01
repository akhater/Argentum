// Generated from the `commands!` table in src-tauri/src/mods/dispatch.rs
// by src-tauri/src/mods/ag_contract.rs. Do not edit: change the Rust types,
// then run `AG_WRITE_CONTRACT=1 cargo test --lib ag_contract` in src-tauri.

/**
 * `black_white_auto`'s answer: the B&W mix's bands as the edit names them,
 * -100..100. `unknown` to the interface: it is a piece of the edit, whose band
 * names the interface defines (`BlackWhite.tsx`), and the edit is untyped on
 * both sides.
 */
export type AutoMix = unknown;

/**
 * The automatic values in effect for a photo: the radius its RAW gave, and
 * the contrast threshold its last measurement found. Either is null until
 * there is one - the radius until the photo is decoded, the contrast until
 * a render at 100% (or the mask view) has needed it.
 *
 * `f64`, widened from the `f32` each is kept as, so they read in the
 * interface exactly as they always have.
 */
export type AutoValues = { radius: number | null, contrast: number | null, };

export type AutoWbArgs = { 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, mode: DetectMode, };

/**
 * What the detector found, and the white balance that removes it.
 */
export type AutoWhiteBalance = { 
/**
 * Detected illuminant chromaticity (CIE 1931 2° observer).
 */
x: number, y: number, 
/**
 * Correlated colour temperature of that illuminant in kelvin, for the log.
 */
temperatureK: number, 
/**
 * Kelvin and tint in RapidRAW 1.6.5's units, absolute, as their picker
 * returns them. The frontend writes it the way their picker does.
 */
whiteBalance: WhiteBalance, };

/**
 * A box dragged on the photo, in the same frame as a click.
 */
export type Bounds = { startX: number, startY: number, endX: number, endY: number, };

/**
 * What the editor needs when a photo opens.
 */
export type BuiltinLens = { 
/**
 * The photo's own tables; `None` when it carries none.
 */
correction: LensCorrection | null, 
/**
 * The switch's starting position, for an edit that has none yet.
 */
defaultOn: boolean, 
/**
 * The photo has no saved edit, so My Lenses' "Correct new photos"
 * decides how it opens (argentum/newPhotoDefaults.ts).
 */
fresh: boolean, };

export type BuiltinPreset = { 
/**
 * `rawmakase:<folder>/<file name>`: the same on every machine, so a star stays on it.
 */
id: string, name: string, adjustments: unknown, };

/**
 * A camera the user has actually opened a photo from.
 */
export type Camera = { 
/**
 * The maker, shown the way My Lenses shows a lens maker.
 */
make: string, model: string, 
/**
 * The profile file new photos from this body open with (My Cameras);
 * none, the built-in matrix. Only a photo with no saved edit takes it
 * (argentum/newPhotoDefaults.ts), so choosing one never changes a photo.
 */
defaultProfile?: string, };

/**
 * What the UI needs to show for one photo: which camera, and whether the
 * library has a profile for it.
 */
export type CameraProfileStatus = { 
/**
 * The body, as the file reports it. `None` if the file cannot be read.
 */
camera: string | null, 
/**
 * The maker, for display.
 */
make: string | null, 
/**
 * Every profile the library holds for this body.
 */
available: Array<Installed>, 
/**
 * Whether RawTherapee's own file for this body is already here, so the
 * panel knows there is nothing left for "Find one" to fetch.
 */
publishedInstalled: boolean, };

/**
 * What My Gear needs to draw one camera's row.
 */
export type CameraProfiles = { profiles: Array<Installed>, 
/**
 * Whether RawTherapee's own file for this body is already here, so the
 * row knows there is nothing left for "Find one" to fetch.
 */
publishedInstalled: boolean, };

export type Cleared = { freedBytes: number, 
/**
 * What could not be deleted, each with why.
 */
left: Array<string>, };

/**
 * One click on the photo: where, in the canvas's frame of the whole photo
 * (rotated, flipped and turned as it is shown, before the crop), and whether
 * it adds or leaves out.
 */
export type Click = { x: number, y: number, positive: boolean, };

/**
 * The app's RAW decoder.
 */
export type DecoderArgs = { decoder: RawDecoder, };

/**
 * A camera and the profile its new photos open with (none: built-in).
 */
export type DefaultProfileArgs = { model: string, file: string | null, };

/**
 * A TIFF bit depth as the interface names it: 8 or 16.
 *
 * A plain `u8` rather than an enum over the wire, because the value crossing
 * this boundary comes from a browser and any number is possible. It is narrowed
 * to a depth we support by `TiffDepth::from_u8`, which treats anything
 * unrecognised as 16 - what an export did before the setting existed.
 */
export type DepthArgs = { depth: number, };

export type DetectMode = "surfaces" | "edges";

/**
 * A disk as the folder column lists it.
 */
export type Drive = { 
/**
 * Where it is mounted, as the folder tree writes paths: `E:\` on
 * Windows, `/Volumes/Photos` on macOS.
 */
path: string, 
/**
 * The volume's own name, empty when it has none.
 */
label: string, 
/**
 * A memory card or USB stick rather than a fixed disk.
 */
removable: boolean, 
/**
 * Zero where this system does not report it.
 */
freeBytes: number, 
/**
 * Zero where this system does not report it.
 */
totalBytes: number, };

/**
 * A Face mask: the parts to draw, the edit's geometry, and the frame.
 */
export type FaceMaskArgs = { 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, parts: ReadonlyArray<string>, rotation: number, flipHorizontal: boolean, flipVertical: boolean, orientationSteps: number, };

export type FileArgs = { file: string, };

/**
 * A folder's path before and after a rename.
 */
export type FolderRenameArgs = { oldPath: string, newPath: string, };

/**
 * Space under a compact slider row, in pixels.
 */
export type GapArgs = { gap: number, };

/**
 * A camera mode that recorded the photo darker, and by how many stops.
 */
export type HeldBack = { mode: Mode, ev: number, };

/**
 * A photo and the History to keep for it; none takes it out.
 */
export type HistoryArgs = { path: string, record: HistoryRecord | null, };

/**
 * A photo's History as its sidecar keeps it.
 */
export type HistoryRecord = { 
/**
 * The step the edit is at, darktable's history_end. Those after it were
 * undone and can be redone.
 */
end: number, 
/**
 * Oldest first. Step 0 is the state the History starts from.
 */
steps: Array<HistoryStep>, 
/**
 * The fingerprint of the edit saved beside it when it was written. Set
 * here, never by the editor.
 */
edit?: string, };

/**
 * A step of the History: its name and what it changed.
 */
export type HistoryStep = { 
/**
 * As the History tab names it.
 */
name: string, 
/**
 * Each value the step set, under its place in the edit.
 */
set?: Record<string, unknown>, 
/**
 * The places it took out.
 */
unset?: Array<string>, };

/**
 * A catalogue id from `mods::model_catalog`, never a path.
 */
export type IdArgs = { id: string, };

/**
 * Looks' ids: a built-in preset's, a preset's own, a LUT's path.
 */
export type IdsArgs = { ids: Array<string>, };

/**
 * A profile in the library, with where it came from.
 */
export type Installed = { 
/**
 * File name in the library, which is also its id.
 */
file: string, 
/**
 * The profile's own name, if it has one.
 */
name: string | null, 
/**
 * The body it is for, as matched against.
 */
camera: string | null, };

/**
 * Which part of the photo to select.
 */
export type Kind = "subject" | "sky" | "background";

/**
 * What the Modern arrangement's own parts were left as. The interface
 * owns the tab ids, the widths and what they start as; this only keeps them.
 */
export type LayoutPanels = { 
/**
 * The Navigator section is open.
 */
navigatorOpen: boolean, 
/**
 * The tab open beside the adjustments, or none while the tab panel is
 * folded.
 */
tab: string | null, 
/**
 * The tab panel's width, in CSS pixels.
 */
tabWidthPx: number, 
/**
 * The History section is open. Saved before it could fold: open.
 */
historyOpen: boolean, 
/**
 * History's height in the left column, in CSS pixels; none until dragged.
 */
historyHeightPx: number | null, 
/**
 * The Snapshots list's height in the left column, in CSS pixels; none
 * until dragged.
 */
snapshotsHeightPx: number | null, 
/**
 * The Navigator floats over the photo instead of sitting in the left column.
 */
navigatorFloating: boolean, 
/**
 * Where the floating Navigator was put: its top-left corner, as fractions
 * of the centre's width and height; none until moved.
 */
navigatorAt: [number, number] | null, 
/**
 * The panes folded by their headings (src/argentum/FoldingPane.tsx), by
 * id, such as "crop.rotation". None until one is folded.
 */
folded: Array<string>, };

export type LayoutPanelsArgs = { panels: LayoutPanels, };

export type LearningArgs = { on: boolean, };

/**
 * Something in the models folder no catalogue entry claims.
 */
export type Leftover = { 
/**
 * Where it is inside the models folder, `/` between folder and name.
 */
name: string, bytes: number, };

/**
 * Built-in lens correction for one photo.
 */
export type LensCorrection = { 
/**
 * Where the data came from, for display, e.g. "Fujifilm built-in".
 */
source: string, 
/**
 * Lightroom applies this correction by default. True where verified
 * against Adobe's own rendering.
 */
default_on: boolean, 
/**
 * Gain that restores illumination at each radius (1 = none).
 */
vignetting: Radial | null, 
/**
 * Source radius divided by output radius for the green channel (1 = none).
 */
distortion: Radial | null, 
/**
 * Additional source radius scale for red and blue relative to green (1 = none).
 */
chromatic: [Radial, Radial] | null, 
/**
 * The parts the camera always corrects, as Lightroom applies them from
 * the file whatever its profile setting. A lens profile corrects the
 * other parts, and the camera's own tables for them only stand in where
 * the profile has none. `None`, as in every correction stored before this
 * field: all three, so no profile is ever used beside it.
 */
always?: Parts, 
/**
 * The optical centre the radial functions are measured from, in
 * fractions of the developed image's width and height: where the
 * camera's data places it, the image centre where it records none
 * (`read` fills that in). `None`, as in every correction stored before
 * this field: the image centre, framed as it was then
 * (`fill_scale_with`), so a saved edit renders as it did.
 */
centre?: [number, number], };

export type Light = "lamp" | "daylight";

export type Listing = { models: Array<ModelStatus>, 
/**
 * Everything in the folder, listed or not.
 */
totalBytes: number, 
/**
 * The part of that `other` adds up to.
 */
otherBytes: number, 
/**
 * What no entry claims: what Clear deletes.
 */
other: Array<Leftover>, 
/**
 * The download running now, for a page opened after it began.
 */
downloading: Progress | null, };

/**
 * A look's file that is already there.
 */
export type LookFile = { camera: string, look: string, path: string, };

/**
 * What became of one photo's request.
 */
export type LookOutcome = "done" | "cancelled" | "failed" | "unsupported" | "folder";

/**
 * The open photo and the LUT just put on it (its path).
 */
export type LookPickArgs = { path: string, lut: string, };

/**
 * One photo's result, as the interface hears it.
 */
export type LookProgress = { jobId: number, path: string, camera: string, look: string, options: { [key in string]?: boolean }, ownWb: OwnWb | null, outcome: LookOutcome, 
/**
 * For Preview all: nothing goes on a photo.
 */
preview: boolean, };

/**
 * A white balance as a look file is named for it: whole kelvin and whole tint,
 * finer than a step of either slider, so equal ones give one name on every run.
 */
export type LookWb = { temperature: number, tint: number, };

/**
 * What became of a photo's look files and previews.
 */
export type LooksDeleted = { 
/**
 * Look files and previews, so right after Preview all it is the previews.
 */
deleted: number, 
/**
 * An edit uses these.
 */
kept: number, 
/**
 * These would not go.
 */
failed: number, };

/**
 * A photo to measure Auto white balance on, and the photo a pasted Auto came from.
 */
export type MeasureWbArgs = { path: string, from: string | null, };

/**
 * The camera's white balance and the one Auto found, in kelvin and tint.
 */
export type Measured = { asShot: WhiteBalance, whiteBalance: WhiteBalance, };

/**
 * The modes, named as the Profile note names them (src/argentum/locales).
 */
export type Mode = "fujifilmDr200" | "fujifilmDr400" | "canonHighlightTonePriority" | "nikonHlg" | "nikonActiveDLighting" | "olympusGradation" | "pentaxDynamicRange";

export type ModelArgs = { 
/**
 * Absent for commands that only identify a camera by model.
 */
make?: string, model: string, };

export type ModelStatus = { id: string, label: string, purpose: string, onDisk: OnDisk, 
/**
 * Size on disk now.
 */
bytes: number, 
/**
 * What a download fetches now: the files not yet there at their size.
 */
downloadBytes: number, 
/**
 * Deleting it was refused while Argentum runs; it goes at the next start.
 */
removingAtNextStart: boolean, };

/**
 * Whether a feature's files are all here, and how much is left to fetch.
 */
export type ModelsStatus = { installed: boolean, missingBytes: number, totalBytes: number, };

/**
 * What the interface needs to draw the folder column the moment it opens.
 */
export type ModernStart = { drives: Array<Drive>, 
/**
 * Recent folders that are there now, newest first.
 */
recent: Array<string>, 
/**
 * The folder to open, or none.
 */
startFolder: string | null, 
/**
 * The folders to show unfolded: those saved, plus every folder above
 * `start_folder` up to its disk, so that it shows.
 */
expanded: Array<string>, };

/**
 * The library's saved folder state, for `modern_start`.
 */
export type ModernStartArgs = { lastFolder: string | null, expanded: Array<string>, };

/**
 * The Modern window's palette. Crosses to the interface as its lowercase
 * name. Classic keeps the app's own theme setting and never reads this.
 */
export type ModernTheme = "grey" | "deep" | "light";

/**
 * Leftovers in the models folder, as `list_ai_models` named them.
 */
export type NamesArgs = { names: Array<string>, };

/**
 * Object: the clicks and the box on the photo, the edit's geometry, and the frame.
 */
export type ObjectArgs = { 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, clicks: Array<Click>, bounds: Bounds | null, rotation: number, flipHorizontal: boolean, flipVertical: boolean, orientationSteps: number, };

/**
 * A camera the program offers.
 */
export type OfferedCamera = { id: string, name: string, make: string, model: string, };

/**
 * A look the program offers.
 */
export type OfferedLook = { id: string, name: string, 
/**
 * The heading it shows under, named by the program; with none, it has no heading.
 */
group: string | null, };

/**
 * An on/off choice the program offers. What it means is the program's: only
 * its label is shown and only its id and value are sent.
 */
export type OfferedOption = { id: string, label: string, default: boolean, 
/**
 * With it on, looks follow the photo's white balance. Programs from before
 * this flag have none.
 */
sendsCurrentWb: boolean, };

/**
 * An old edit found on disk, and what it becomes.
 */
export type OldWhiteBalanceEdit = { path: string, 
/**
 * The whole edit, converted.
 */
adjustments: unknown, 
/**
 * The old white balance numbers, globally and per mask in order: how the
 * frontend recognises the edit still open in the editor unconverted.
 */
was: unknown, };

export type OnArgs = { on: boolean, };

export type OnDisk = "downloaded" | "partial" | "missing";

/**
 * The open photo, as the editor names it, and its edit.
 */
export type OpenPhotoArgs = { path: string, 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, };

export type Outcome = "deleted" | "atNextStart" | "notThere";

/**
 * A look whose option has it follow the photo's white balance.
 */
export type OwnWb = { 
/**
 * The option that asks for it. It tells no files apart: at the camera's white
 * balance the look is the one without it, in the same file.
 */
option: string, 
/**
 * What the file was made for; None for the camera's.
 */
wb: LookWb | null, };

/**
 * One flag for each part of a lens correction.
 */
export type Parts = { distortion: boolean, chromatic: boolean, vignetting: boolean, };

export type PathArgs = { path: string, };

export type PathsArgs = { paths: Array<string>, };

/**
 * A photo's RAW decoder; none is the app's.
 */
export type PhotoDecoderArgs = { path: string, decoder: RawDecoder | null, };

/**
 * What the open photo can have.
 */
export type PhotoLooks = { 
/**
 * Its own camera on the list.
 */
camera: string, files: Array<LookFile>, 
/**
 * The temporary files of Preview all: to look at, never a look on a photo.
 */
previews: Array<LookFile>, 
/**
 * Its look files and previews, for any camera, look and options, listed
 * or not: what its Delete all previews looks at.
 */
clearable: number, 
/**
 * How the look files reported follow its white balance.
 */
ownWb: OwnWb | null, };

/**
 * The folder a file dialog opens in: the open photo's, when there is one.
 */
export type PickArgs = { from: string | null, };

/**
 * Arguments are sent as the frontend writes them, so camelCase throughout.
 */
export type PointArgs = { x: number, y: number, 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, };

export type PresetGroup = { name: string, presets: Array<BuiltinPreset>, };

export type PreviewPayload = { result: string, original: string, };

export type Progress = { 
/**
 * The catalogue entry being downloaded.
 */
id: string, done: number, total: number, };

/**
 * A provider look as an edit holds it, under `agProviderLook`.
 */
export type ProviderLook = { camera: string, look: string, 
/**
 * Its options away from their default. Edits saved before options have none.
 */
options?: { [key in string]?: boolean }, 
/**
 * Set while an option has it follow the photo's white balance. The photo
 * keeps the file this names until a new one is ready, however the sliders
 * have moved since.
 */
ownWb?: OwnWb, before: RenderingBefore, };

export type ProviderLookCancelArgs = { jobId: number, };

export type ProviderLookDeleteArgs = { path: string, 
/**
 * The look on the open edit: its save may still be on its way.
 */
onPhoto: ProviderLook | null, };

export type ProviderLookFileArgs = { path: string, camera: string, look: string, options: { [key in string]?: boolean }, ownWb: OwnWb | null, };

export type ProviderLookMakeArgs = { paths: Array<string>, 
/**
 * None for each photo's own camera.
 */
camera: string | null, look: string, options: { [key in string]?: boolean }, 
/**
 * What each photo's saved edit does not hold yet.
 */
changes: unknown, remake: boolean, front: boolean, };

export type ProviderLookPreviewArgs = { path: string, camera: string, options: { [key in string]?: boolean }, 
/**
 * What its saved edit does not hold yet.
 */
changes: unknown, };

/**
 * What the provider offers.
 */
export type ProviderLooks = { cameras: Array<OfferedCamera>, looks: Array<OfferedLook>, options: Array<OfferedOption>, 
/**
 * The switch.
 */
on: boolean, 
/**
 * None for no photo, a photo that is not a RAW, or one from no camera on
 * the list: no provider looks show for it.
 */
photo: PhotoLooks | null, };

export type ProviderLooksArgs = { path: string | null, 
/**
 * The options whose look files to report.
 */
options: { [key in string]?: boolean }, 
/**
 * What its saved edit does not hold yet.
 */
changes: unknown, };

export type ProviderLooksSwitchArgs = { on: boolean, };

/**
 * A radial function sampled at increasing radii. Radius 0 is the
 * correction's optical centre (`LensCorrection::centre`) and 1 is half of
 * the image diagonal, so an off-centre correction's farthest corner lies
 * beyond 1 (`reach`). Values are linearly interpolated and held constant
 * beyond the outermost knots.
 */
export type Radial = { knots: Array<number>, values: Array<number>, };

export type RawDecoder = "rawler" | "flashraw";

export type RawToneArgs = { path: string, mode: string, };

/**
 * The rendering a provider look replaced on its photo.
 */
export type RenderingBefore = { rawToneRendering: string | null, rawToneCurve: Array<ToneCurvePoint> | null, rawToneCurveMode: string | null, };

export type RestoreMaskArgs = { path: string, mask: number, };

export type RestoreReblendArgs = { mask: number, };

/**
 * What the mask stores: the fields of their `AiForegroundMaskParameters`, and
 * whether anything was found at all.
 */
export type Selection = { maskDataBase64: string, rotation: number, flipHorizontal: boolean, flipVertical: boolean, orientationSteps: number, found: boolean, };

/**
 * A one-click selection: which part, the edit's geometry, and the frame it is stored in.
 */
export type SelectionArgs = { kind: Kind, 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, rotation: number, flipHorizontal: boolean, flipVertical: boolean, orientationSteps: number, };

export type Snapshot = { id: string, name: string, 
/**
 * When it was made or last updated, in milliseconds since 1970.
 */
at: number, 
/**
 * The whole edit as it was.
 */
adjustments: unknown, };

/**
 * A photo and the snapshots to keep for it.
 */
export type SnapshotsArgs = { path: string, snapshots: Array<Snapshot>, };

/**
 * What the Looks tab shows for the open photo.
 */
export type Suggested = { learning: boolean, 
/**
 * How many photos have a pick.
 */
picks: number, 
/**
 * The open photo's light.
 */
light: Light, 
/**
 * Most likely first; empty while learning is off or picks are too few.
 */
suggestions: Array<Suggestion>, 
/**
 * Of the picks made while there were suggestions, how many took one of
 * the three shown (`right`) out of how many (`of`).
 */
right: number, of: number, };

export type Suggestion = { 
/**
 * The LUT's path.
 */
lut: string, 
/**
 * How many of the photo's nearest picks in its light chose it.
 */
votes: number, 
/**
 * How many photos it was picked for, in all.
 */
picks: number, 
/**
 * The light its votes came from, or with none, most of its picks.
 */
light: Light, };

export type SuperResolutionBatchArgs = { paths: Array<string>, scale: number, restore: number | null, };

export type SuperResolutionPreviewArgs = { path: string, scale: number, 
/**
 * Restore's Mask slider, when Restore is on.
 */
restore: number | null, };

export type SuperResolutionSaveArgs = { path: string, };

/**
 * A Text mask: the words, the edit's geometry, and the frame.
 */
export type TextMaskArgs = { 
/**
 * The edit, as the interface holds it: untyped on both sides.
 */
jsAdjustments: unknown, words: string, rotation: number, flipHorizontal: boolean, flipVertical: boolean, orientationSteps: number, };

/**
 * How large the panels' text is drawn. Crosses to the interface as its
 * lowercase name; the interface owns what each step scales by.
 */
export type TextSize = "small" | "normal" | "large";

/**
 * A text size's name, narrowed by `TextSize::from_name`, Normal if unknown.
 */
export type TextSizeArgs = { size: string, };

/**
 * A Modern palette's name, narrowed by `ModernTheme::from_name`.
 */
export type ThemeArgs = { theme: string, };

export type ToneCurvePoint = { x: number, y: number, };

export type WhiteBalance = { temperature: number, tint: number, };

/** Every `ag` command: what it takes and what it answers. */
export interface AgCommands {
  add_recent_folder: { args: PathArgs; answer: Array<string> };
  batch_super_resolution: { args: SuperResolutionBatchArgs; answer: Array<string> };
  black_white_auto: { args: OpenPhotoArgs; answer: AutoMix };
  builtin_presets: { args: Record<string, never>; answer: Array<PresetGroup> };
  camera_default_profile: { args: ModelArgs; answer: string | null };
  camera_profile_status: { args: PathArgs; answer: CameraProfileStatus };
  clear_ai_model_leftovers: { args: NamesArgs; answer: Cleared };
  compact_slider_gap: { args: Record<string, never>; answer: number };
  delete_ai_model: { args: IdArgs; answer: Outcome };
  detect_auto_white_balance: { args: AutoWbArgs; answer: AutoWhiteBalance };
  discard_super_resolution: { args: Record<string, never>; answer: null };
  dng_standard_held_back: { args: PathArgs; answer: HeldBack | null };
  download_ai_model: { args: IdArgs; answer: number };
  download_face_mask_models: { args: Record<string, never>; answer: null };
  download_selection_models: { args: Record<string, never>; answer: null };
  download_text_mask_models: { args: Record<string, never>; answer: null };
  editor_layout_panels: { args: Record<string, never>; answer: LayoutPanels | null };
  face_mask: { args: FaceMaskArgs; answer: Selection };
  face_mask_models_status: { args: Record<string, never>; answer: ModelsStatus };
  flashraw_handles: { args: PathArgs; answer: boolean };
  folder_photo_count: { args: PathArgs; answer: number };
  forget_camera: { args: ModelArgs; answer: null };
  forget_look_picks: { args: Record<string, never>; answer: null };
  get_profile_online: { args: ModelArgs; answer: string | null };
  highlight_recovery: { args: Record<string, never>; answer: boolean };
  history: { args: PathArgs; answer: HistoryRecord | null };
  import_camera_profile: { args: PathArgs; answer: Installed };
  lens_builtin: { args: PathArgs; answer: BuiltinLens };
  list_ai_models: { args: Record<string, never>; answer: Listing };
  list_cameras: { args: Record<string, never>; answer: Array<Camera> };
  look_pick: { args: LookPickArgs; answer: Suggested };
  look_suggestions: { args: PathArgs; answer: Suggested };
  look_unpick: { args: PathArgs; answer: Suggested };
  looks_favourites: { args: Record<string, never>; answer: Array<string> };
  measure_auto_white_balance: { args: MeasureWbArgs; answer: Measured };
  modern_start: { args: ModernStartArgs; answer: ModernStart };
  modern_theme: { args: Record<string, never>; answer: ModernTheme };
  profiles_for_camera: { args: ModelArgs; answer: CameraProfiles };
  provider_look_cancel: { args: ProviderLookCancelArgs; answer: null };
  provider_look_delete: { args: ProviderLookDeleteArgs; answer: LooksDeleted };
  provider_look_file: { args: ProviderLookFileArgs; answer: boolean };
  provider_look_make: { args: ProviderLookMakeArgs; answer: number };
  provider_look_preview: { args: ProviderLookPreviewArgs; answer: number };
  provider_looks: { args: ProviderLooksArgs; answer: ProviderLooks | null };
  provider_looks_switch: { args: ProviderLooksSwitchArgs; answer: null };
  raw_decoder: { args: Record<string, never>; answer: RawDecoder };
  raw_tone_curve: { args: RawToneArgs; answer: Array<ToneCurvePoint> };
  recover_lens_name: { args: PathArgs; answer: string | null };
  redecode_open_photo: { args: Record<string, never>; answer: boolean };
  refresh_image_metadata: { args: PathArgs; answer: { [key in string]?: string } };
  remove_camera_profile: { args: FileArgs; answer: null };
  rename_recent_folder: { args: FolderRenameArgs; answer: Array<string> };
  restore_mask: { args: RestoreMaskArgs; answer: string };
  restore_reblend: { args: RestoreReblendArgs; answer: string };
  sample_processed_pixel: { args: PointArgs; answer: [number, number, number] };
  save_super_resolution: { args: SuperResolutionSaveArgs; answer: string };
  selection_mask: { args: SelectionArgs; answer: Selection };
  selection_models_status: { args: Record<string, never>; answer: ModelsStatus };
  selection_object: { args: ObjectArgs; answer: Selection };
  set_camera_default_profile: { args: DefaultProfileArgs; answer: null };
  set_compact_slider_gap: { args: GapArgs; answer: null };
  set_editor_layout_panels: { args: LayoutPanelsArgs; answer: null };
  set_highlight_recovery: { args: OnArgs; answer: null };
  set_history: { args: HistoryArgs; answer: null };
  set_look_learning: { args: LearningArgs; answer: null };
  set_looks_favourites: { args: IdsArgs; answer: null };
  set_modern_theme: { args: ThemeArgs; answer: null };
  set_photo_raw_decoder: { args: PhotoDecoderArgs; answer: null };
  set_raw_decoder: { args: DecoderArgs; answer: null };
  set_snapshots: { args: SnapshotsArgs; answer: null };
  set_text_size: { args: TextSizeArgs; answer: null };
  set_tiff_bit_depth: { args: DepthArgs; answer: null };
  sharpen_auto: { args: PathArgs; answer: AutoValues };
  shell_folder: { args: PathArgs; answer: Array<string> };
  shell_launch_photo: { args: Record<string, never>; answer: string | null };
  shell_pick: { args: PickArgs; answer: string | null };
  shell_show_window: { args: Record<string, never>; answer: null };
  snapshots: { args: PathArgs; answer: Array<Snapshot> };
  stop_ai_model_download: { args: Record<string, never>; answer: null };
  super_resolution_preview: { args: SuperResolutionPreviewArgs; answer: PreviewPayload };
  text_mask: { args: TextMaskArgs; answer: Selection };
  text_mask_models_status: { args: Record<string, never>; answer: ModelsStatus };
  text_size: { args: Record<string, never>; answer: TextSize };
  tiff_bit_depth: { args: Record<string, never>; answer: number };
  upgrade_white_balance: { args: PathsArgs; answer: Array<OldWhiteBalanceEdit> };
}
