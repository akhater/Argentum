// Start the app in development straight into the new editor interface
// (src/shell): `npm run start:shell`, or `npm run start:shell -- <photo>` to
// open a RAW at once. `npm run tauri dev` starts today's interface as before.
//
// Vite hands VITE_SHELL to the page, and src/shell/switch.tsx opens the shell
// when it is "new". An environment variable rather than a Tauri config, so
// going between the two interfaces never rebuilds the Rust side.

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';

const tauri = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const photo = process.argv.slice(2);
// Arguments after a second `--` go to the app, which opens a path it is given.
const args = [tauri, 'dev', ...(photo.length ? ['--', '--', ...photo] : [])];
const { status, error } = spawnSync(process.execPath, args, {
  stdio: 'inherit',
  env: { ...process.env, VITE_SHELL: 'new' },
});
if (error) console.error(error.message);
process.exit(status ?? 1);
