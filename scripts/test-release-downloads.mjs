// Tests for scripts/release-downloads.mjs.
//
// The one that matters: from 26.43.4's attached files, the script writes the
// Download section its release page opens with, as it was written by hand,
// less the line about the `_tethering` files releases no longer have.
//
// Run:  npm run test:checks   (or node scripts/test-release-downloads.mjs)

import assert from 'node:assert/strict';

import { downloadSection, withDownloads } from './release-downloads.mjs';

let ran = 0;
let failed = 0;
const test = (name, fn) => {
  ran += 1;
  try {
    fn();
    console.log(`  ok    ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`  FAIL  ${name}`);
    for (const line of String(error.message ?? error).split('\n')) console.log(`          ${line}`);
  }
};

/** The files attached to release 26.43.4. */
const ASSETS = [
  '01_Argentum_v26.43.4_windows-11-arm_arm64.exe',
  '01_Argentum_v26.43.4_windows-11-arm_arm64.msi',
  '01_Argentum_v26.43.4_windows_x64.exe',
  '01_Argentum_v26.43.4_windows_x64.msi',
  '02_Argentum_v26.43.4_macos-14_aarch64.app.tar.gz',
  '02_Argentum_v26.43.4_macos-14_aarch64.dmg',
  '02_Argentum_v26.43.4_macos-15-intel_x64.app.tar.gz',
  '02_Argentum_v26.43.4_macos-15-intel_x64.dmg',
  '02_tethering_Argentum_v26.43.4_macos-14_aarch64.app.tar.gz',
  '02_tethering_Argentum_v26.43.4_macos-14_aarch64.dmg',
  '02_tethering_Argentum_v26.43.4_macos-15-intel_x64.app.tar.gz',
  '02_tethering_Argentum_v26.43.4_macos-15-intel_x64.dmg',
  '03_Argentum_v26.43.4_ubuntu-22.04-arm_aarch64.AppImage',
  '03_Argentum_v26.43.4_ubuntu-22.04-arm_aarch64.rpm',
  '03_Argentum_v26.43.4_ubuntu-22.04-arm_arm64.deb',
  '03_Argentum_v26.43.4_ubuntu-22.04_amd64.AppImage',
  '03_Argentum_v26.43.4_ubuntu-22.04_amd64.deb',
  '03_Argentum_v26.43.4_ubuntu-22.04_x86_64.rpm',
  '03_Argentum_v26.43.4_ubuntu-24.04-arm_aarch64.AppImage',
  '03_Argentum_v26.43.4_ubuntu-24.04-arm_aarch64.rpm',
  '03_Argentum_v26.43.4_ubuntu-24.04-arm_arm64.deb',
  '03_Argentum_v26.43.4_ubuntu-24.04_amd64.AppImage',
  '03_Argentum_v26.43.4_ubuntu-24.04_amd64.deb',
  '03_Argentum_v26.43.4_ubuntu-24.04_x86_64.rpm',
  '03_tethering_Argentum_v26.43.4_ubuntu-22.04_amd64.AppImage',
  '03_tethering_Argentum_v26.43.4_ubuntu-22.04_amd64.deb',
  '03_tethering_Argentum_v26.43.4_ubuntu-22.04_x86_64.rpm',
  '03_tethering_Argentum_v26.43.4_ubuntu-24.04_amd64.AppImage',
  '03_tethering_Argentum_v26.43.4_ubuntu-24.04_amd64.deb',
  '03_tethering_Argentum_v26.43.4_ubuntu-24.04_x86_64.rpm',
];

/** The Download section 26.43.4's notes open with, as written by hand, without its `_tethering` line. */
const SECTION_26_43_4 =
  '## Download\n\n| Your computer | File |\n|---|---|\n| Windows | [01_Argentum_v26.43.4_windows_x64.exe](https://github.com/akhater/Argentum/releases/download/26.43.4/01_Argentum_v26.43.4_windows_x64.exe) |\n| Windows on ARM | [01_Argentum_v26.43.4_windows-11-arm_arm64.exe](https://github.com/akhater/Argentum/releases/download/26.43.4/01_Argentum_v26.43.4_windows-11-arm_arm64.exe) |\n| Mac with Apple Silicon (M1 and later) | [02_Argentum_v26.43.4_macos-14_aarch64.dmg](https://github.com/akhater/Argentum/releases/download/26.43.4/02_Argentum_v26.43.4_macos-14_aarch64.dmg) |\n| Mac with Intel | [02_Argentum_v26.43.4_macos-15-intel_x64.dmg](https://github.com/akhater/Argentum/releases/download/26.43.4/02_Argentum_v26.43.4_macos-15-intel_x64.dmg) |\n| Linux | the `.AppImage`, `.deb` or `.rpm` named for your Ubuntu version, x64 (`amd64`, `x86_64`) or ARM (`arm64`, `aarch64`) |\n\n- **Windows:** the installer is not signed yet, so SmartScreen may say "Windows protected your PC". Choose **More info**, then **Run anyway**.\n- **Mac:** the app is not notarized yet; do [the two one-time steps](https://github.com/akhater/Argentum#macos-two-one-time-steps) after copying it to Applications.';

test("26.43.4's files give 26.43.4's Download section", () => {
  assert.equal(downloadSection('26.43.4', ASSETS), SECTION_26_43_4);
});

test("an older release's `_tethering` files are never linked, and a release without them reads the same", () => {
  assert.equal(downloadSection('26.43.4', [...ASSETS].reverse()), SECTION_26_43_4);
  const plain = ASSETS.filter((name) => !name.includes('_tethering'));
  assert.equal(downloadSection('26.43.4', plain), SECTION_26_43_4);
});

test('an Apple Silicon file is found whichever macOS version built it', () => {
  const built = ASSETS.map((name) => name.replace('_macos-14_', '_macos-15_'));
  assert.match(downloadSection('26.43.4', built), /\[02_Argentum_v26\.43\.4_macos-15_aarch64\.dmg\]/);
});

test('a missing installer stops it rather than linking nothing', () => {
  const noIntel = ASSETS.filter((name) => name !== '02_Argentum_v26.43.4_macos-15-intel_x64.dmg');
  assert.throws(() => downloadSection('26.43.4', noIntel), /Mac with Intel/);
});

test('the section goes first, or replaces the one already there', () => {
  const section = downloadSection('26.43.4', ASSETS);
  const notes = '## Added\n\n- A thing.\n';
  const once = withDownloads(notes, section);
  assert.equal(once, `${section}\n\n${notes}`);
  assert.equal(withDownloads(once, section), once);
  assert.equal(withDownloads(once.replace(/26\.43\.4/g, '26.43.3'), section), once);
});

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exitCode = failed > 0 ? 1 : 0;
