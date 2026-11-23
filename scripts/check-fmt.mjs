// rustfmt over the app's own Rust modules (src-tauri/src/mods/*.rs), the same
// command CI runs in .github/workflows/lint.yml, so a pull request that fails
// formatting fails here first. Inherited files are not yet formatted and are
// deliberately not checked; only mods/ is held.
//
//   npm run check:fmt        check (no files are changed)

import { spawnSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const tauri = join(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri');
const files = readdirSync(join(tauri, 'src', 'mods'))
  .filter((name) => name.endsWith('.rs'))
  .sort()
  .map((name) => join('src', 'mods', name));

const result = spawnSync('rustfmt', ['--edition', '2024', '--check', ...files], {
  cwd: tauri,
  stdio: 'inherit',
});
if (result.error) {
  console.error(`check:fmt: could not run rustfmt: ${result.error.message}`);
  process.exitCode = 1;
} else {
  process.exitCode = result.status ?? 1;
}
