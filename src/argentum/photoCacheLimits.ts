// The limits of Settings > Processing > Decoded Image Cache. They mirror
// src-tauri/src/mods/photo_cache_budget.rs, and a Rust test fails if they differ.
export const DECODED_PHOTO_CACHE_MIN_MB = 512;
export const DECODED_PHOTO_CACHE_MAX_MB = 8192;
export const DECODED_PHOTO_CACHE_STEP_MB = 128;
// The size shown before the app has stored one (a 16 GB machine's default).
export const DECODED_PHOTO_CACHE_FALLBACK_MB = 1536;
// One decoded photo of 24 megapixels: the hint on the setting counts in these.
export const TYPICAL_PHOTO_MB = 288;
