// RawTherapee's camera profiles, as "Find one" offers them, regenerated.
//
// Argentum does not ship camera profiles: it downloads RawTherapee's on request
// (src-tauri/src/mods/profiles_online.rs). This table says which exist, at one
// RawTherapee commit, with each file's size and git blob hash, so the app knows
// what is there without asking GitHub at click time (its API allows 60 calls an
// hour per network), downloads that exact file, and refuses one that differs.
// It also carries RawTherapee's camera_model_aliases.json: the other names of
// the same body (a Kiss X7 or Rebel SL1 takes the EOS 100D profile).
//
//   * one source commit at a time, named on the table's "# Source:" line;
//   * the rows are generated, never edited by hand;
//   * check:sources warns when RawTherapee changes the folder after that commit.
//
// Run:  node scripts/rawtherapee-profiles.mjs <full commit of RawTherapee>
// Test: node scripts/test-rawtherapee-profiles.mjs
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const REPO = 'RawTherapee/RawTherapee';
export const FOLDER = 'rtdata/dcpprofiles';
export const ALIASES = 'camera_model_aliases.json';
export const TABLE = 'src-tauri/src/mods/rt_profiles.tsv';

const headers = () => ({
  'User-Agent': 'Argentum',
  Accept: 'application/vnd.github+json',
  ...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}),
});

/**
 * One row per profile: file, size, git blob hash, and the other names of its
 * body (';' between them), in file name order. Left out: aliases of a body with
 * no profile in the folder, which would name nothing to download, and another
 * name that has a profile of its own (RawTherapee names the D800 under the
 * D800E, and the D800 has its own), so a body with its own profile is never
 * offered another body's.
 */
export function tableRows(listing, aliases) {
  const names = new Map(Object.entries(aliases).map(([body, other]) => [body.toLowerCase(), other]));
  const profiles = listing.filter((f) => f.type === 'file' && f.name.toLowerCase().endsWith('.dcp'));
  const own = new Set(profiles.map((f) => f.name.slice(0, -'.dcp'.length).toLowerCase()));
  return profiles
    .map((f) => {
      if (!/^[0-9a-f]{40}$/.test(f.sha)) throw new Error(`${f.name}: no git hash`);
      if (/[\t;]/.test(f.name)) throw new Error(`${f.name}: a tab or ';' in a file name`);
      const stem = f.name.slice(0, -'.dcp'.length);
      const other = (names.get(stem.toLowerCase()) ?? []).filter(
        (n) => !/[\t;]/.test(n) && !own.has(n.toLowerCase()),
      );
      return [f.name, f.size, f.sha, other.join(';')].join('\t');
    })
    .sort((a, b) => a.localeCompare(b, 'en'));
}

/** The "# Source:" line: which RawTherapee commit, and what the rows hold. */
export function sourceLine(sha, rows) {
  const aliases = rows.reduce((n, r) => n + (r.split('\t')[3] ? r.split('\t')[3].split(';').length : 0), 0);
  return `# Source: RawTherapee ${FOLDER} @ ${sha}, ${rows.length} profiles, ${aliases} other names`;
}

/** The commit a table names, or null if it names none. */
export function recordedSource(tsv) {
  const m = /^# Source: RawTherapee \S+ @ ([0-9a-f]{40}),/m.exec(tsv);
  return m ? m[1] : null;
}

/** The table rewritten from the source: our comments kept, the Source line and rows replaced. */
export function regenerate(tsv, listing, aliases, sha) {
  const lines = tsv.split('\n');
  const end = lines.findIndex((l) => !l.startsWith('#'));
  const header = lines.slice(0, end === -1 ? lines.length : end);
  const at = header.findIndex((l) => l.startsWith('# Source:'));
  if (at === -1) throw new Error(`${TABLE} has no "# Source:" line in its header`);
  const rows = tableRows(listing, aliases);
  header[at] = sourceLine(sha, rows);
  return `${[...header, ...rows].join('\n')}\n`;
}

/** The folder's listing and its aliases at one commit. */
export async function fetchSource(sha) {
  const api = `https://api.github.com/repos/${REPO}/contents/${FOLDER}?ref=${sha}&per_page=1000`;
  const listed = await fetch(api, { headers: headers(), signal: AbortSignal.timeout(20000) });
  if (!listed.ok) throw new Error(`${api}: HTTP ${listed.status}`);
  const raw = `https://raw.githubusercontent.com/${REPO}/${sha}/${FOLDER}/${ALIASES}`;
  const named = await fetch(raw, { signal: AbortSignal.timeout(20000) });
  if (!named.ok) throw new Error(`${raw}: HTTP ${named.status}`);
  return { listing: await listed.json(), aliases: await named.json() };
}

/** The newest RawTherapee commit that changed the folder, on its default branch. */
export async function newestChange() {
  const api = `https://api.github.com/repos/${REPO}/commits?path=${FOLDER}&per_page=1`;
  const response = await fetch(api, { headers: headers(), signal: AbortSignal.timeout(15000) });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const [newest] = await response.json();
  return newest?.sha ?? null;
}

/** For check:sources: a warning when RawTherapee changed the profiles after the table's commit, else null. */
export async function profilesWarning(root) {
  // A checkout without the table (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, TABLE))) return null;
  const recorded = recordedSource(readFileSync(join(root, TABLE), 'utf8'));
  if (!recorded) return `${TABLE} names no RawTherapee commit - node scripts/rawtherapee-profiles.mjs <commit>`;
  let newest;
  try {
    newest = await newestChange();
  } catch (error) {
    return `could not ask GitHub for RawTherapee's profile changes, so ${TABLE} is unchecked (${error.cause?.code ?? error.message})`;
  }
  if (!newest || newest === recorded) return null;
  return `RawTherapee changed its camera profiles after the table's commit - node scripts/rawtherapee-profiles.mjs ${newest}`;
}

async function main(sha) {
  if (!/^[0-9a-f]{40}$/.test(sha ?? '')) throw new Error('usage: node scripts/rawtherapee-profiles.mjs <full 40-character commit>');
  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const path = join(root, TABLE);
  const before = readFileSync(path, 'utf8');
  const { listing, aliases } = await fetchSource(sha);
  const after = regenerate(before, listing, aliases, sha);
  writeFileSync(path, after);
  console.log(`  ${TABLE}: ${sha.slice(0, 8)}`);
  console.log(before === after ? '  unchanged' : '  changed - git diff it before merging');
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv[2]).catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
