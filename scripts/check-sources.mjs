// What Argentum takes from other projects, and the wiring that must not
// silently come undone. Runs in CI as a required check
// (.github/workflows/upstream.yml) and with `npm run check:sources`.
//
// Argentum was forked from RapidRAW at 1.6.6 and no longer merges it (DEC-38):
// later releases are read and harvested by hand, like darktable's. So nothing
// here counts lines in their files any more. What is left:
//
//   * fails when one of the wiring lines below is gone. Each one is a call
//     whose absence breaks a feature without any error (sharpening that does
//     nothing, a TIFF without metadata, a mask with a hard edge). Each goes
//     once a real test covers its feature;
//   * warns when RapidRAW has released past the version the last harvest
//     review read (the CHANGELOG's "read up to" line): a review is due;
//   * warns when RAWmakase has released past a table generated from it (DEC-34),
//     and when RawTherapee changes the camera profiles after the commit
//     src-tauri/src/mods/rt_profiles.tsv names;
//   * warns when FlashRaw has released past the version the engine pins: a new
//     version is measured against rawler's image before the pin moves;
//   * warns when Shoebox's main pins a different rawler commit: the two apps
//     share the fork, so the pins move together, and neither release waits for
//     the other.
//
// A network failure is a warning, never a failure. No git history is needed.
//
// Run standalone:  node scripts/check-sources.mjs

import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join } from 'node:path';

import { releases } from './rawmakase-cameras.mjs';
import { tableWarning } from './rawmakase-cameras.mjs';
import { chartWarning } from './rawmakase-gray-chart.mjs';
import { curvesWarning } from './rawmakase-grade-curves.mjs';
import { lensWarning } from './rawmakase-lens-baselines.mjs';
import { namesWarning } from './rawmakase-camera-names.mjs';
import { profilesWarning } from './rawtherapee-profiles.mjs';

export const RAPIDRAW = 'https://github.com/CyberTimon/RapidRAW';
export const FLASHRAW = 'https://github.com/ybouane/flashraw';
const FLASHRAW_PIN = 'src-tauri/engine/Cargo.toml';
const RAWLER_PIN = 'src-tauri/Cargo.toml';
const SHOEBOX_CARGO = 'https://raw.githubusercontent.com/akhater/shoebox/main/src-tauri/Cargo.toml';

/** Calls that must stay in place. Moved here from the retired anchors (scripts/upstream-anchors.mjs). */
export const WIRING = [
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    pattern: /= ag_stage_scene_linear\(/,
    why: 'the camera profile and highlight recovery run in the scene-linear stage',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    pattern: /= ag_stage_tone\(/,
    why: 'the RAW tone rendering (DNG Standard, Base Curve) runs in the tone stage',
  },
  {
    file: 'src-tauri/src/shaders/shader.wgsl',
    pattern: /= ag_stage_display\(/,
    why: 'the clipping and sharpening mask views run in the display stage',
  },
  {
    file: 'src-tauri/src/gpu_processing.rs',
    pattern: /staged\.as_ref\(\)\.unwrap_or\(&cache\.texture_view\)/,
    why: 'the render has to read what the input stage returns, or sharpening silently does nothing',
  },
  {
    file: 'src-tauri/src/gpu_processing.rs',
    pattern: /input_stage::Staged { view: staged, request }/,
    why: 'the render has to use the request the input stage returns, or Noise Reduction runs twice',
  },
  {
    file: 'src-tauri/src/export_processing.rs',
    pattern: /write_export_metadata\(/,
    why: 'the export has to go through ours or a TIFF silently loses its metadata',
  },
  {
    file: 'src-tauri/src/mask_generation.rs',
    pattern: /mask_falloff::linear\(/,
    why: 'the linear mask has to fade through ours or it gets its hard edges back',
  },
  {
    file: 'src-tauri/src/mask_generation.rs',
    pattern: /mask_falloff::radial\(/,
    why: 'the radial mask has to fade through ours or its feather gets a hard inner edge back',
  },
  {
    file: 'src-tauri/src/ai_processing.rs',
    pattern: /session_builder\(\)\?\.commit_from_file\(encoder_path\)/,
    why: 'the mask models have to be built through ours or they keep gigabytes of working memory',
  },
  {
    file: 'src-tauri/src/ai_processing.rs',
    pattern: /session_builder\(\)\?\.commit_from_file\(depth_path\)/,
    why: 'the depth model, the other large one, has to be built through ours too',
  },
  {
    file: 'src/components/panel/right/ExportPanel.tsx',
    pattern: /from\s+['"][^'"]*argentum\/exportDepth['"]/,
    why: 'the size estimate has to import the TIFF depth',
  },
  {
    file: 'src/components/panel/right/ExportPanel.tsx',
    pattern: /\buseTiffDepth\(\)/,
    why: 'the size estimate has to read the TIFF depth',
  },
  {
    // An array literal that closes a call is the shape of a hook dependency
    // list: a mere mention of the name in a comment does not satisfy it.
    file: 'src/components/panel/right/ExportPanel.tsx',
    pattern: /\[[^\]]*\btiffDepth\b[^\]]*\]\s*\)/,
    why: 'the size estimate has to list the TIFF depth as a dependency, or it freezes on the old depth',
  },
  {
    file: 'src-tauri/src/formats.rs',
    pattern: /tif_raw::is_camera_raw\(/,
    why: 'without it a .TIF raw silently opens as its embedded thumbnail',
  },
];

/** The wiring lines missing from the tree; `read` returns a file's text, or null when it is gone. */
export function missingWiring(read, wiring = WIRING) {
  const texts = new Map();
  return wiring.filter(({ file, pattern }) => {
    if (!texts.has(file)) texts.set(file, read(file));
    const text = texts.get(file);
    return text === null || !pattern.test(text);
  });
}

/** The RapidRAW release the last harvest review read, from the CHANGELOG's "read up to `vX`". */
export function reviewedRelease(changelog) {
  return /read up to `(v\d+(?:\.\d+)*)`/.exec(changelog)?.[1] ?? null;
}

/** A warning when RapidRAW has a release newer than the one reviewed, else null. */
export function harvestWarning(reviewed, tags) {
  if (!reviewed) return 'CHANGELOG.md names no RapidRAW release read up to ("read up to `vX`")';
  const newest = [...tags.keys()].at(-1);
  if (!newest || !tags.has(reviewed) || newest === reviewed) return null;
  return `RapidRAW ${newest} is out and the last harvest review read up to ${reviewed}: a review is due`;
}

async function rapidrawWarning(root) {
  const reviewed = reviewedRelease(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'));
  try {
    return harvestWarning(reviewed, await releases(RAPIDRAW));
  } catch (error) {
    return `could not list RapidRAW's releases (${error.code ?? error.message})`;
  }
}

/** The FlashRaw version the engine pins, from its `flashraw = { version = "=X", ... }`. */
export function pinnedFlashRaw(cargoToml) {
  return /^flashraw\s*=.*?"=(\d+(?:\.\d+)*)"/m.exec(cargoToml)?.[1] ?? null;
}

/** A warning when FlashRaw has a release newer than the engine's pin, else null. */
export function pinWarning(pinned, tags) {
  if (!pinned) return `${FLASHRAW_PIN} no longer pins FlashRaw ("=X"), so a new release would be taken unmeasured`;
  const newest = [...tags.keys()].at(-1);
  if (!newest || newest.slice(1).localeCompare(pinned, 'en', { numeric: true }) <= 0) return null;
  return `FlashRaw ${newest} is out and the engine pins ${pinned}: measure it before moving the pin (${FLASHRAW_PIN})`;
}

async function flashrawWarning(root) {
  // A checkout without the engine (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, FLASHRAW_PIN))) return null;
  const pinned = pinnedFlashRaw(readFileSync(join(root, FLASHRAW_PIN), 'utf8'));
  try {
    return pinWarning(pinned, await releases(FLASHRAW));
  } catch (error) {
    return `could not list FlashRaw's releases (${error.code ?? error.message})`;
  }
}

/** The rawler commit a Cargo.toml pins, from its `rawler = { git = "...", rev = "X" }`. */
export function pinnedRawler(cargoToml) {
  return /^rawler\s*=.*?\brev\s*=\s*"([^"]+)"/m.exec(cargoToml)?.[1] ?? null;
}

/** Shoebox's Cargo.toml on its main branch. */
async function fetchShoeboxCargo() {
  const response = await fetch(SHOEBOX_CARGO, { signal: AbortSignal.timeout(20000) });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.text();
}

/** A warning when Shoebox pins a different rawler commit from ours, else null. */
export async function rawlerWarning(root, readShoebox = fetchShoeboxCargo) {
  if (!existsSync(join(root, RAWLER_PIN))) return null;
  const ours = pinnedRawler(readFileSync(join(root, RAWLER_PIN), 'utf8'));
  if (!ours) return `${RAWLER_PIN} no longer pins rawler to a commit, so it cannot be compared with Shoebox's`;
  let shoebox;
  try {
    shoebox = pinnedRawler(await readShoebox());
  } catch (error) {
    return `could not read Shoebox's Cargo.toml (${error.message})`;
  }
  if (!shoebox) return `Shoebox's Cargo.toml names no rawler commit, so the two pins were not compared`;
  if (ours === shoebox) return null;
  return `rawler is pinned at ${ours.slice(0, 7)} here and ${shoebox.slice(0, 7)} in Shoebox: move both to the same commit`;
}

async function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const started = Date.now();
  const read = (file) => {
    try {
      return readFileSync(join(root, file), 'utf8');
    } catch {
      return null;
    }
  };

  const missing = missingWiring(read);
  const warnings = (
    await Promise.all([
      rapidrawWarning(root),
      tableWarning(root).catch((error) => `camera table not checked against RAWmakase: ${error.message}`),
      chartWarning(root).catch((error) => `B&W chart not checked against RAWmakase: ${error.message}`),
      curvesWarning(root).catch((error) => `colour grading curves not checked against RAWmakase: ${error.message}`),
      lensWarning(root).catch((error) => `lens baselines not checked against RAWmakase: ${error.message}`),
      namesWarning(root).catch((error) => `camera names not checked against RAWmakase: ${error.message}`),
      profilesWarning(root).catch((error) => `camera profiles not checked against RawTherapee: ${error.message}`),
      flashrawWarning(root),
      rawlerWarning(root),
    ])
  ).filter(Boolean);

  for (const warning of warnings) console.log(`  warning: ${warning}`);
  if (missing.length > 0) {
    console.error('\n  SOURCES CHECK FAILED: wiring that a feature depends on is gone\n');
    for (const { file, pattern, why } of missing) console.error(`    ${file}\n      ${pattern}: ${why}`);
    process.exitCode = 1;
  }
  console.log(`  check:sources ${((Date.now() - started) / 1000).toFixed(1)} s`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main().catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
