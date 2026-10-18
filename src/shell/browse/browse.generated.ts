// Generated from src-tauri/src/mods/browse/contract.rs by its test. Do not
// edit: change contract.rs, then run, in src-tauri,
// `AG_WRITE_CONTRACT=1 cargo test --lib mods::browse::contract`.

/** The long edge of a thumbnail, in pixels: a grid cell's size at 2x. */
export const THUMB_PX = 400;

/**
 * A folder the tree shows.
 */
export type Folder = { 
/**
 * Its own name, or a drive's, as the tree shows it.
 */
name: string, path: string, 
/**
 * Whether it holds folders, so the tree offers to open it.
 */
hasFolders: boolean, };

/**
 * Where browsing starts: the computer's drives and folders, and the folders
 * remembered.
 */
export type Places = { 
/**
 * The drives on Windows; the root elsewhere.
 */
drives: Array<Folder>, 
/**
 * Pictures and the home folder, where there are such.
 */
standard: Array<Folder>, 
/**
 * Pinned folders, in the order they were pinned.
 */
favourites: Array<string>, 
/**
 * Folders opened, the latest first.
 */
recent: Array<string>, };

/**
 * A photo in a folder: one stat of the file, and whether a sidecar sits
 * beside it.
 */
export type Photo = { name: string, path: string, bytes: number, 
/**
 * When the file was last written, in milliseconds since 1970.
 */
modifiedMs: number, 
/**
 * An Argentum sidecar (`.agdata`) is beside it. Never read here.
 */
edited: boolean, };

/**
 * Names one listing of a folder. A newer listing stops every older one.
 */
export type ListingId = number;

/**
 * What went wrong, as a kind the interface can act on.
 */
export type BrowseErrorKind = "notFound" | "notAFolder" | "denied" | "noPreview" | "io" | "internal";

/**
 * A refused call: what went wrong, and in words.
 */
export type BrowseError = { kind: BrowseErrorKind, message: string, };

/**
 * What browsing tells the interface, as it happens.
 */
export type BrowseEvent = { "event": "listed", listing: ListingId, photos: Array<Photo>, done: boolean, } | { "event": "thumb", path: string, } | { "event": "noThumb", path: string, error: BrowseError, };
