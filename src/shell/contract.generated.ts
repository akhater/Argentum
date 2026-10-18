// Generated from src-tauri/engine/src/contract.rs by
// src-tauri/engine/tests/contract.rs. Do not edit: change contract.rs, then
// run `AG_WRITE_CONTRACT=1 cargo test --test contract` in src-tauri/engine.

/** A setting's value: a number, a boolean, or the value of a choice. */
export type SettingValue = number | boolean | string;

export const EDIT_VERSION = 1;

export const HISTOGRAM_BINS = 256;

export const ENGINE_EVENT = "ag-engine";

export const FRAME_HEADER_BYTES = 12;

export const FRAME_SHARED = "shared";

export const FRAME_SHARED_KEY = "agFrame";

/**
 * Whose code a tool is: the first two letters of its id.
 */
export type Origin = "dt" | "rt" | "rr" | "rm" | "ag";

/**
 * Where in the pipeline a tool runs: on the sensor data before demosaic, or
 * on linear light after it. A stage for the picture shaped for a screen
 * comes with the first tool that needs one.
 */
export type Stage = "raw" | "scene";

/**
 * The panel a tool is shown in. More can be added; `SECTIONS` says their
 * order.
 */
export type Section = "basic" | "tone" | "color" | "detail" | "lens" | "effects";

/**
 * One section, as the interface shows it.
 */
export type SectionDescriptor = {
  id: Section,
  /**
   * An i18n key: `sections.<id>`.
   */
  label: string,
};

/**
 * What kind of control a setting is.
 */
export type SettingKind = "float" | "int" | "bool" | "choice";

/**
 * How a float setting's control moves: evenly, or by ratio - so that, on a
 * multiplier from 0.05 to 20, 1 sits in the middle.
 */
export type SettingScale = "linear" | "log";

export type Choice = {
  value: string,
  /**
   * An i18n key.
   */
  label: string,
};

/**
 * A setting that only means something while another setting has one value:
 * a manual radius, say, only while the automatic one is off.
 */
export type EnabledWhen = {
  /**
   * The other setting's key, in the same tool.
   */
  key: string,
  equals: SettingValue,
};

/**
 * One setting of a tool, as a panel shows it.
 */
export type SettingSpec = {
  /**
   * Its name in the tool's settings.
   */
  key: string,
  /**
   * An i18n key: `tools.<tool id>.<key>`.
   */
  label: string,
  kind: SettingKind,
  default: SettingValue,
  min?: number,
  max?: number,
  step?: number,
  unit?: string,
  /**
   * Float settings only.
   */
  scale?: SettingScale,
  choices?: Array<Choice>,
  enabledWhen?: EnabledWhen,
  /**
   * Shown under "More" rather than in the panel's first view.
   */
  advanced: boolean,
};

/**
 * One tool, as the interface builds its panel.
 */
export type ToolDescriptor = {
  /**
   * Stable for ever: origin prefix and a name, e.g. `rt_capture_sharpen`.
   */
  id: string,
  origin: Origin,
  /**
   * An i18n key: `tools.<tool id>.label`.
   */
  label: string,
  section: Section,
  /**
   * The tool's place in the pipeline, and in its section.
   */
  order: number,
  stage: Stage,
  /**
   * The version of its settings.
   */
  version: number,
  settings: Array<SettingSpec>,
};

/**
 * One tool's settings in an edit.
 */
export type ToolEdit = {
  /**
   * The version of the tool's settings these were written at.
   */
  v: number,
  settings: Record<string, SettingValue>,
};

/**
 * Every tool's settings for one photo.
 */
export type EditDocument = {
  /**
   * `EDIT_VERSION`.
   */
  version: number,
  tools: Record<string, ToolEdit>,
};

/**
 * A change to an edit: `{ toolId: { key: value } }`, merged into it. `null`
 * puts a setting back to its default. Only the tools it names, and the
 * tools after them, run again.
 */
export type EditPatch = Record<string, Record<string, SettingValue | null>>;

/**
 * Names an open photo.
 */
export type PhotoId = number;

/**
 * Names one requested render. A newer ticket cancels every older one.
 */
export type RenderTicket = number;

/**
 * What `apply` returns: the ticket of the render it started, and the edit
 * as the engine took it - clamped, with the defaults filled in. What the
 * interface shows should be this, not what it sent.
 */
export type Applied = {
  ticket: RenderTicket,
  edit: EditDocument,
};

/**
 * What `open` returns.
 */
export type Opened = {
  photoId: PhotoId,
  /**
   * The developed picture's size, in photo pixels.
   */
  width: number,
  height: number,
  edit: EditDocument,
};

/**
 * The part of the photo on screen.
 */
export type View = {
  /**
   * Screen pixels per photo pixel: 1 is 100%.
   */
  zoom: number,
  /**
   * The point of the photo at the centre of the view, 0..1 across and
   * down.
   */
  centerX: number,
  centerY: number,
  /**
   * The view's size in device pixels: CSS pixels times the device pixel
   * ratio, so a frame fills it pixel for pixel.
   */
  width: number,
  height: number,
};

/**
 * Where a frame sits in its view, in device pixels: its top left corner,
 * and its size. On an axis where the photo is smaller than the view, the
 * frame is all of it, centred; where it is larger, the frame fills the view.
 */
export type Placement = {
  xPx: number,
  yPx: number,
  widthPx: number,
  heightPx: number,
};

/**
 * What went wrong, as a kind the interface can act on.
 */
export type ErrorKind = "unknownPhoto" | "badView" | "unknownTool" | "unknownSetting" | "badValue" | "badEdit" | "decode" | "noGpu" | "render" | "internal";

/**
 * A refused call: what went wrong, and in words.
 */
export type EngineError = {
  kind: ErrorKind,
  message: string,
};

/**
 * What the engine tells the interface, as it happens.
 */
export type Event = {
  "event": "frame",
  ticket: RenderTicket,
  /**
   * The photo it is a view of: a late frame of the photo before can
   * be told apart and dropped.
   */
  photoId: PhotoId,
  placement: Placement,
} | {
  "event": "histogram",
  ticket: RenderTicket,
  photoId: PhotoId,
  bins: Array<number>,
} | {
  "event": "error",
  ticket?: RenderTicket,
  kind: ErrorKind,
  message: string,
};
