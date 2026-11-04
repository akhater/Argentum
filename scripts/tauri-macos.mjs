// `tauri` for the macOS release builds: the app keeps its signature, the DMG does not. Ours.
//
// The certificate is self-signed (build.yml says why). Tauri signs the DMG with it too, and
// Gatekeeper checks a signed DMG before opening it: a certificate Apple does not know means
// "Apple could not verify ... is free of malware" and the DMG never opens. An unsigned DMG opens
// as before. Tauri has no setting to sign the app and not the DMG
// (tauri-bundler/src/bundle/macos/dmg/mod.rs, tauri-cli-v2.12.1), so this takes it off after the
// build, and fails before tauri-action uploads anything if a DMG is still signed.
//
// Run:  node scripts/tauri-macos.mjs build [--target <triple>] ...   (build.yml, macOS only)
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readdirSync, renameSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
execFileSync('npx', ['tauri', ...args], { stdio: 'inherit' });
if (args[0] !== 'build') process.exit();

const target = args[args.indexOf('--target') + 1];
const dmgDir = join('src-tauri', 'target', args.includes('--target') ? target : '', 'release', 'bundle', 'dmg');
const dmgs = existsSync(dmgDir) ? readdirSync(dmgDir).filter((name) => name.endsWith('.dmg')) : [];
if (dmgs.length === 0) {
  console.error(`::error::no DMG in ${dmgDir}`);
  process.exitCode = 1;
}
for (const name of dmgs) {
  const dmg = join(dmgDir, name);
  // `codesign --remove-signature` refuses disk images; a converted copy is a new image, unsigned.
  const unsigned = join(dmgDir, `unsigned-${name}`);
  execFileSync('hdiutil', ['convert', dmg, '-format', 'UDZO', '-o', unsigned], { stdio: 'inherit' });
  renameSync(unsigned, dmg);
  const shown = spawnSync('codesign', ['-d', dmg], { encoding: 'utf8' });
  if (!/not signed at all/.test(shown.stderr)) {
    console.error(`::error::${dmg} is still signed:\n${shown.stderr}`);
    process.exitCode = 1;
  } else {
    console.log(`${dmg}: signature removed`);
  }
}
