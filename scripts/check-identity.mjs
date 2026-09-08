// Tauri bakes the asset-protocol scope ($APPCACHE/thumbnails/*) into the
// generated schemas in src-tauri/gen at build time. Change `identifier` in
// tauri.conf.json and those schemas go stale, so every thumbnail fails with:
//
//   asset protocol not configured to allow the path: .../thumbnails/....jpg
//
// Full images keep working, because they go through Rust commands rather than
// the asset protocol - so it looks like broken RAW decoding, not a config
// problem. It cost us two debugging rounds. Now it just fixes itself.

import { readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const confPath = join(root, 'src-tauri', 'tauri.conf.json');
const genDir = join(root, 'src-tauri', 'gen');
const stampPath = join(root, 'src-tauri', '.identity');

let identifier;
try {
  identifier = JSON.parse(readFileSync(confPath, 'utf8')).identifier;
} catch (err) {
  console.error(`\n  tauri.conf.json is not valid JSON: ${err.message}`);
  console.error('  (a UTF-8 BOM will do this - PowerShell\'s -Encoding utf8 writes one)\n');
  process.exit(1);
}

const previous = existsSync(stampPath) ? readFileSync(stampPath, 'utf8').trim() : null;

if (previous && previous !== identifier && existsSync(genDir)) {
  rmSync(genDir, { recursive: true, force: true });
  console.log(`  identifier changed: ${previous} -> ${identifier}`);
  console.log('  cleared src-tauri/gen so the asset scope regenerates\n');
}

writeFileSync(stampPath, identifier + '\n');
