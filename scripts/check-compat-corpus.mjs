// The compatibility corpus has a folder for the version being released. Ours.
//
// docs/RELEASING.md step 2 writes `src-tauri/fixtures/compat/<version>/` and its snapshot; a
// forgotten step is found here, before any installer is built: release.yml runs this first and
// builds nothing if it fails. The same rule, for the version in Cargo.toml, is the ignored test
// `the_release_being_cut_has_its_folder` in src-tauri/src/mods/compat_corpus.rs.
//
// Run:  node scripts/check-compat-corpus.mjs [<tag>]   (the tag, if given, must be Cargo.toml's version)
// Test: node scripts/test-check-compat-corpus.mjs
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

/** The files every folder of today holds. */
export const FOLDER_FILES = ['settings.json', 'photo.agdata', 'argentum-processing.json'];

/** The version in a Cargo.toml's text. */
export const cargoVersion = (toml) => /^version\s*=\s*"([^"]+)"/m.exec(toml)?.[1];

/** What stands between `version` and a release, one line each; none when it is ready. */
export function problems(version, tag, compat) {
  const found = [];
  if (tag && tag !== version) {
    found.push(`the release is tagged ${tag} but src-tauri/Cargo.toml says ${version}`);
  }
  for (const file of FOLDER_FILES) {
    if (!existsSync(join(compat, version, file))) {
      found.push(`src-tauri/fixtures/compat/${version}/${file} is missing`);
    }
  }
  if (!existsSync(join(compat, 'snapshots', `${version}.txt`))) {
    found.push(`src-tauri/fixtures/compat/snapshots/${version}.txt is missing`);
  }
  return found;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const version = cargoVersion(readFileSync('src-tauri/Cargo.toml', 'utf8'));
  const found = problems(version, process.argv[2], 'src-tauri/fixtures/compat');
  if (found.length) {
    console.error(
      `The compatibility corpus is not ready for ${version}:\n  ${found.join('\n  ')}\n` +
        'Step 2 of docs/RELEASING.md writes them; commit them with the release.',
    );
    process.exit(1);
  }
  console.log(`The compatibility corpus has its folder and snapshot for ${version}.`);
}
