// What the stub's "backend" answers, one line per command the app calls while
// it is walked. Fixed data, the same on every run and every machine: a disk
// with one folder of demo photos (photos.js), the app's default settings, no
// albums, presets or LUTs.
//
// A command missing here answers null and is listed by the walk as unanswered;
// an `ag` name missing here rejects, as the app's dispatcher does for a name
// it does not route. To answer a new command, add its line to COMMANDS (or AG
// for an `ag` name): `name: (args, backend) => value`.

import { DEMO_DRIVE, DEMO_FOLDER, DEMO_PARENT, PHOTOS, photoAt } from './photos.js';

/** The app's defaults (src-tauri/src/app_settings.rs), as Windows has them, but with the photo drawn by the page. */
const SETTINGS = {
  language: 'en',
  theme: 'dark',
  decorations: false,
  rootFolders: [],
  pinnedFolders: [],
  lastRootPath: null,
  lastFolderState: null,
  thumbnailSize: 'medium',
  thumbnailAspectRatio: 'contain',
  libraryViewMode: 'flat',
  editorPreviewResolution: 1920,
  enableZoomHifi: true,
  enableLivePreviews: true,
  livePreviewQuality: 'high',
  enableAiTagging: false,
  aiProvider: 'cpu',
  processingBackend: 'auto',
  exifOverlay: 'off',
  enableFolderImageCounts: false,
  displayEditIcon: true,
  enableXmpSync: true,
  createXmpIfMissing: false,
  isWaveformVisible: false,
  // The one change from the defaults: the GPU draws the photo behind a see-through window in the app,
  // and there is no GPU window here, so the page draws it as it does on Linux.
  useWgpuRenderer: false,
  canvasInputMode: 'mouse',
  keybinds: {},
  myLenses: [],
  folderTreeSort: { key: 'name', order: 'asc' },
  libraryDisplayMode: 'grid',
};

const clone = (value) => JSON.parse(JSON.stringify(value));
let settings = clone(SETTINGS);
let openPath = null;

const folder = (path, name, children = []) => ({
  name,
  path,
  children,
  isDir: true,
  imageCount: path === DEMO_FOLDER ? PHOTOS.length : 0,
  hasSubdirs: children.length > 0,
  modified: 1767348000,
  created: 1767348000,
});

const TREE = folder(DEMO_DRIVE, 'D:', [folder(DEMO_PARENT, 'Photos', [folder(DEMO_FOLDER, 'Demo')])]);

function findFolder(node, path) {
  if (node.path === path) return node;
  for (const child of node.children) {
    const found = findFolder(child, path);
    if (found) return found;
  }
  return null;
}

const imageFile = (photo) => ({
  path: photo.path,
  modified: photo.modified,
  is_edited: false,
  has_provider_look: false,
  rating: 0,
  flag: null,
  tags: [],
  exif: photo.exif,
  is_virtual_copy: false,
  is_cloud_placeholder: false,
  is_raw: photo.isRaw,
  group_id: null,
});

const imagesIn = (path) => (path === DEMO_FOLDER ? PHOTOS.map(imageFile) : []);

const metadataOf = () => ({ version: 1, rating: 0, adjustments: null, tags: [] });

/** The photo the editor asked for, or the one open. */
const photoFor = (args) => photoAt(args.path ?? openPath) ?? photoAt(openPath) ?? PHOTOS[0];

const none = () => null;
const empty = () => [];

const COMMANDS = {
  // Start.
  load_settings: () => clone(settings),
  save_settings: ({ settings: next }) => {
    settings = clone(next ?? settings);
    return null;
  },
  get_supported_file_types: () => ({
    raw: ['dng', 'cr2', 'cr3', 'nef', 'arw', 'raf', 'orf', 'rw2'],
    nonRaw: ['jpg', 'jpeg', 'png', 'tif', 'tiff'],
  }),
  is_tethering_supported: () => false,
  is_raw9_available: () => false,
  frontend_ready: () => {
    // `?open` starts the app as "open with" a photo does: straight into the editor.
    const open = new URLSearchParams(window.location.search).has('open');
    return open ? { openWithFile: PHOTOS[0].path } : {};
  },
  frontend_log: none,

  // The library.
  get_pinned_folder_trees: ({ paths }) => (paths ?? []).map((path) => findFolder(TREE, path)).filter(Boolean),
  get_folder_tree: ({ path }) => findFolder(TREE, path),
  get_folder_children: ({ path }) => findFolder(TREE, path)?.children ?? [],
  list_images_in_dir: ({ path }) => imagesIn(path),
  list_images_recursive: ({ path }) => (path && DEMO_FOLDER.startsWith(path) ? PHOTOS.map(imageFile) : []),
  read_exif_for_paths: ({ paths }) =>
    Object.fromEntries((paths ?? []).map((path) => [path, photoAt(path)?.exif ?? {}])),
  update_thumbnail_queue: ({ paths }, backend) => {
    backend.later(() => {
      for (const path of paths ?? []) {
        const photo = photoAt(path);
        if (photo) backend.emit('thumbnail-generated', { path, data: photo.dataUrl, rating: 0, is_edited: false });
      }
    });
    return null;
  },
  start_background_indexing: none,
  cancel_thumbnail_generation: none,
  get_albums: empty,
  get_album_images: empty,
  load_presets: empty,
  list_luts: empty,
  list_lut_libraries: empty,
  fetch_community_presets: empty,
  // What changes photos on disk returns nothing (Rust's Ok(())), which reaches the page as null.
  set_rating_for_paths: none,
  set_flag_for_paths: none,
  set_color_label_for_paths: none,
  apply_adjustments_to_paths: none,
  reset_adjustments_for_paths: none,
  apply_auto_adjustments_to_paths: none,
  apply_auto_lens_correction_to_paths: none,
  apply_auto_straighten_to_paths: none,
  copy_files: none,
  delete_files_from_disk: none,
  delete_folder: none,
  show_in_finder: none,
  // The copy's path: VIRTUAL_COPY and a six-character id; the file copied to "<name>_copy" beside it.
  create_virtual_copy: ({ sourceVirtualPath }) => `${sourceVirtualPath}?vc=a1b2c3`,
  duplicate_file: ({ path }) => path.replace(/(\.[^.\\]+)$/, '_copy$1'),

  // The editor.
  clear_session_caches: none,
  load_metadata: metadataOf,
  load_image: (args) => {
    const photo = photoFor(args);
    openPath = photo.path;
    return {
      width: photo.width,
      height: photo.height,
      metadata: metadataOf(),
      exif: photo.exif,
      is_raw: photo.isRaw,
      as_shot_white_balance: { temperature: 5200, tint: 0 },
    };
  },
  apply_adjustments: () => photoFor({}).bytes.slice(0),
  generate_uncropped_preview: () => photoFor({}).dataUrl,
  generate_preview_for_path: (args) => photoFor(args).bytes.slice(0),
  preview_negative_conversion: (args) => photoFor(args).dataUrl,
  is_image_cached: () => false,
  get_image_dimensions: (args) => ({ width: photoFor(args).width, height: photoFor(args).height }),
  // The analysis's keys (auto_results_to_json in image_processing.rs); no lens keys, as for a lens it cannot resolve.
  calculate_auto_adjustments: () => ({
    exposure: 0.3,
    brightness: 0,
    contrast: 10,
    highlights: -20,
    shadows: 15,
    vibrance: 10,
    vignetteAmount: 0,
    clarity: 5,
    centré: 0,
    dehaze: 0,
    sectionVisibility: { basic: true, color: true, effects: true },
    whites: 5,
    blacks: -5,
  }),
  save_metadata_and_update_thumbnail: none,
  update_wgpu_transform: none,
  // The demo lens is in no lens database, and the demo photos have no lines to level from: both answer None.
  autodetect_lens: none,
  calculate_auto_straighten: none,

  // Export: the estimated size in bytes of the photos named.
  estimate_export_sizes: ({ paths }) => 2_500_000 * (paths?.length ?? 1),

  // Settings. The connector's status comes as an event, never here; there is no connector to report on.
  check_ai_connector_status: none,
  get_log_file_path: () => 'C:\\Users\\Walk\\AppData\\Local\\co.argentum.editor\\logs\\app.log',
  get_lensfun_makers: empty,
  get_lensfun_lenses_for_maker: empty,

  // The window and the plugins.
  'plugin:app|version': () => '0.0.0',
  'plugin:app|name': () => 'Argentum',
  'plugin:window|is_fullscreen': () => false,
  'plugin:window|is_maximized': () => false,
  'plugin:window|is_minimized': () => false,
  'plugin:window|is_focused': () => true,
  'plugin:window|is_visible': () => true,
  'plugin:window|scale_factor': () => 1,
  'plugin:window|inner_size': () => ({ width: window.innerWidth, height: window.innerHeight }),
  'plugin:window|outer_size': () => ({ width: window.innerWidth, height: window.innerHeight }),
  'plugin:os|locale': () => 'en-US',
  'plugin:os|hostname': () => 'walk',
  'plugin:dialog|open': none,
  'plugin:dialog|save': none,
  'plugin:dialog|ask': () => false,
  'plugin:dialog|confirm': () => false,
  'plugin:dialog|message': none,
};

/** Where each demo photo's camera focused: a grid of points with one in focus and one chosen on the
 *  first, focused by hand on the second, nothing recorded on the rest. */
const FOCUS = {
  'Demo-01.dng': {
    mode: 'Single point',
    points: Array.from({ length: 45 }, (_, i) => ({
      x: 0.15 + (i % 9) * 0.08,
      y: 0.2 + Math.floor(i / 9) * 0.13,
      w: 0.035,
      h: 0.05,
      state: i === 15 ? 'inFocus' : i === 24 ? 'chosen' : 'other',
    })),
  },
  'Demo-02.jpg': { mode: 'Manual focus', points: [] },
};

const AG = {
  modern_start: () => ({
    drives: [{ path: DEMO_DRIVE, label: 'Demo', removable: false, freeBytes: 0, totalBytes: 0 }],
    recent: [DEMO_FOLDER],
    startFolder: DEMO_FOLDER,
    expanded: [DEMO_DRIVE, DEMO_PARENT],
  }),
  add_recent_folder: ({ path }) => [path],
  upgrade_white_balance: empty,
  // The open photo's Argentum state: nothing kept yet.
  snapshots: empty,
  set_snapshots: none,
  history: none,
  set_history: none,
  // The Looks tab: no presets of its own, nothing starred.
  builtin_presets: empty,
  looks_favourites: empty,
  // No provider set, as for everyone who has not added one.
  provider_looks: none,
  lens_builtin: () => ({ correction: null, defaultOn: false }),
  dng_standard_held_back: none,
  highlight_recovery: () => false,
  set_highlight_recovery: none,
  focus_points: ({ path }) => FOCUS[path.split('\\').pop()] ?? null,
  focus_points_shown: () => false,
  set_focus_points_shown: none,
  sharpen_auto: () => ({ radius: null, contrast: null }),
  camera_profile_status: () => ({ camera: 'Demo Camera One', make: 'Demo', available: [], publishedInstalled: false }),
  // Settings: no camera seen, no model downloaded.
  list_cameras: empty,
  list_ai_models: () => ({ models: [], totalBytes: 0, otherBytes: 0, other: [], downloading: null }),
  tiff_bit_depth: () => 16,
  folder_photo_count: ({ path }) => (path === DEMO_FOLDER ? PHOTOS.length : 0),
};

/** The answer to one call; a promise that rejects for an `ag` name the stub does not route. */
export async function answer(cmd, args, backend) {
  if (cmd === 'ag') {
    const route = AG[args.name];
    if (!route) throw `ag: the walk's stub does not answer ${args.name}`;
    return route(args.args ?? {}, backend);
  }
  const route = COMMANDS[cmd];
  if (route) return route(args, backend);
  backend.unanswered();
  return null;
}
