// The Download table a release's notes open with, written from the files
// actually attached to it. Ours.
//
// A release page lists about 20 files. The table names the one to pick for
// each computer, with a direct link, then two lines: the Windows SmartScreen
// step and the macOS one-time steps.
// 26.43.4's notes were written this way by hand; release.yml now runs this
// script once every installer is uploaded, so each release gets it.
//
// Run:  node scripts/release-downloads.mjs <tag>   (needs gh, GH_REPO or a checkout)
// Test: node scripts/test-release-downloads.mjs
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const REPO = 'akhater/Argentum';

/** Each computer, and the attached file it gets. Exact, so an older release's `_tethering` files never match. */
export const ROWS = [
  ['Windows', /^01_Argentum_v.+_windows_x64\.exe$/],
  ['Windows on ARM', /^01_Argentum_v.+_windows-11-arm_arm64\.exe$/],
  ['Mac with Apple Silicon (M1 and later)', /^02_Argentum_v.+_macos-\d+_aarch64\.dmg$/],
  ['Mac with Intel', /^02_Argentum_v.+_macos-15-intel_x64\.dmg$/],
];

const LINUX =
  '| Linux | the `.AppImage`, `.deb` or `.rpm` named for your Ubuntu version, x64 (`amd64`, `x86_64`) or ARM (`arm64`, `aarch64`) |';

const NOTES = [
  '- **Windows:** the installer is not signed yet, so SmartScreen may say "Windows protected your PC". Choose **More info**, then **Run anyway**.',
  `- **Mac:** the app is not notarized yet; do [the two one-time steps](https://github.com/${REPO}#macos-two-one-time-steps) after copying it to Applications.`,
];

/** The Download section for release `tag` with these attached file names. Throws when a row has no file. */
export function downloadSection(tag, assets, repo = REPO) {
  const rows = ROWS.map(([computer, pattern]) => {
    const file = assets.find((name) => pattern.test(name));
    if (!file) throw new Error(`${tag}: no attached file for "${computer}" (${pattern})`);
    return `| ${computer} | [${file}](https://github.com/${repo}/releases/download/${tag}/${file}) |`;
  });
  return ['## Download', '', '| Your computer | File |', '|---|---|', ...rows, LINUX, '', ...NOTES].join('\n');
}

/** `body` opening with `section`: an existing Download section is replaced, else it goes first. */
export function withDownloads(body, section) {
  const rest = body.replace(/\r\n/g, '\n');
  const start = rest.indexOf('## Download');
  if (start === -1) return `${section}\n\n${rest.trimStart()}`;
  const next = rest.indexOf('\n## ', start + 1);
  const after = next === -1 ? '' : rest.slice(next + 1);
  return `${rest.slice(0, start)}${section}\n\n${after}`.replace(/\n{3,}/g, '\n\n');
}

function gh(args, input) {
  return execFileSync('gh', args, { encoding: 'utf8', input });
}

function main(tag) {
  if (!tag) throw new Error('usage: node scripts/release-downloads.mjs <tag>');
  const release = JSON.parse(gh(['release', 'view', tag, '--json', 'assets,body']));
  const assets = release.assets.map((a) => a.name);
  const body = withDownloads(release.body ?? '', downloadSection(tag, assets));
  gh(['release', 'edit', tag, '--notes-file', '-'], body);
  console.log(`  ${tag}: Download table written from ${assets.length} attached files`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  try {
    main(process.argv[2]);
  } catch (error) {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  }
}
