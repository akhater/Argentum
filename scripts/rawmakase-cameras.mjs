// RAWmakase's camera table, regenerated.
//
// DNG Standard's exposure offsets (src-tauri/src/mods/dng_baseline.tsv) are
// RAWmakase's data/cameras.toml, row for row. This script writes the table from
// one RAWmakase release, so the table follows its source by rule, not by hand:
//
//   * one source version at a time, named on the table's "# Source:" line;
//   * the rows are generated, never edited by hand;
//   * a value of ours that differs from RAWmakase's is one entry in EXCEPTIONS
//     below, with the reason and what removes it;
//   * check:merge warns when RAWmakase releases past the version named.
//
// The table's other comments are ours and kept as they are.
//
// Run:  node scripts/rawmakase-cameras.mjs <release tag, or full commit>
//       e.g. node scripts/rawmakase-cameras.mjs v0.4.0
// Test: node scripts/test-rawmakase-cameras.mjs

import { execFile } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

export const REPO = 'https://github.com/pch/rawmakase';
export const TABLE = 'src-tauri/src/mods/dng_baseline.tsv';

const FIELDS = ['baseline_exposure', 'lowest_iso', 'levels'];

/**
 * Where a row of ours differs from RAWmakase's, and why. Each entry:
 *
 *   { make: 'Canon', model: 'EOS R5', field: 'baseline_exposure', value: '0.45',
 *     reason: 'what is wrong with theirs, and how we know',
 *     removedWhen: 'what has to happen upstream for this entry to go' }
 *
 * field is one of FIELDS; value is written as it appears in the table (levels
 * as "black,white,maximum", empty for none). An entry whose row is gone, or
 * whose value RAWmakase now has too, stops the script: delete it.
 */
export const EXCEPTIONS = [];

/**
 * The [[camera]] rows of data/cameras.toml: the fields this table takes, each
 * as written there.
 *
 * Not a TOML parser: the file is flat `key = value` lines under [[camera]]
 * headers, and anything else - another table, a value over several lines -
 * stops here, so a change of format is read by a person rather than misread.
 */
export function parseCameras(toml) {
  const cameras = [];
  let camera = null;
  toml.split('\n').forEach((raw, i) => {
    const line = raw.trim();
    const where = `cameras.toml line ${i + 1}`;
    if (line === '' || line.startsWith('#')) return;
    if (line === '[[camera]]') {
      camera = { aliases: [] };
      cameras.push(camera);
      return;
    }
    const pair = /^([A-Za-z0-9_]+)\s*=\s*(.+)$/.exec(line);
    if (!pair || !camera || /"""|'''/.test(line)) {
      throw new Error(`${where}: not a key = value line under [[camera]]: ${line}`);
    }
    const [, key, text] = pair;
    if (key === 'make' || key === 'model') camera[key] = string(text, where);
    else if (key === 'aliases') camera.aliases = list(text, where).map((s) => string(s, where));
    else if (key === 'baseline_exposure' || key === 'lowest_iso') camera[key] = number(text, where);
    else if (key === 'levels') camera.levels = list(text, where).map((n) => number(n, where)).join(',');
  });
  return cameras;
}

const withoutComment = (text) => text.replace(/\s+#[^"]*$/, '').trim();

function string(text, where) {
  const m = /^"([^"\\\t|]*)"$/.exec(withoutComment(text));
  if (!m) throw new Error(`${where}: expected a plain string (no tab, | or escape): ${text}`);
  return m[1];
}

/** A number in its shortest form, as the table writes it: 0.0 is 0, -0.50 is -0.5. */
function number(text, where) {
  const t = withoutComment(text);
  if (!/^-?\d+(\.\d+)?$/.test(t)) throw new Error(`${where}: expected a number: ${text}`);
  return String(Number(t));
}

function list(text, where) {
  const m = /^\[(.*)\]$/.exec(withoutComment(text));
  if (!m) throw new Error(`${where}: expected a one-line array: ${text}`);
  return m[1].split(',').map((s) => s.trim()).filter((s) => s !== '');
}

/** One line per body: make, model|aliases, offset, lowest ISO, levels. */
export function tableRows(cameras, exceptions = EXCEPTIONS) {
  const rows = new Map();
  for (const c of cameras) {
    if (!c.make || !c.model || c.baseline_exposure === undefined) {
      throw new Error(`a [[camera]] without make, model or baseline_exposure: ${JSON.stringify(c)}`);
    }
    const id = `${c.make} ${c.model}`;
    if (rows.has(id)) throw new Error(`${id} is in cameras.toml twice`);
    rows.set(id, { ...c });
  }
  for (const e of exceptions) {
    const row = rows.get(`${e.make} ${e.model}`);
    if (!FIELDS.includes(e.field) || !e.reason || !e.removedWhen) {
      throw new Error(`exception for ${e.make} ${e.model} needs a field (${FIELDS.join(', ')}), a reason and removedWhen`);
    }
    if (!row) throw new Error(`exception for ${e.make} ${e.model}: no such body in cameras.toml any more - delete it`);
    if ((row[e.field] ?? '') === e.value) {
      throw new Error(`exception for ${e.make} ${e.model}: RAWmakase now has ${e.field} = ${e.value} too - delete it`);
    }
    row[e.field] = e.value;
  }
  return [...rows.values()].map((c) => [
    c.make,
    [c.model, ...c.aliases].join('|'),
    c.baseline_exposure,
    c.lowest_iso ?? '',
    c.levels ?? '',
  ].join('\t'));
}

/** The "# Source:" line: which RAWmakase commit, and its release. */
export function sourceLine({ sha, tag }, rows, exceptions = EXCEPTIONS) {
  const makes = new Set(rows.map((r) => r.split('\t')[0])).size;
  return `# Source: RAWmakase data/cameras.toml @ ${sha}${tag ? ` (${tag})` : ''}, `
    + `${rows.length} bodies across ${makes} makes, ${exceptions.length} exceptions`;
}

/** The commit and release a table names, or null if it names none. */
export function recordedSource(tsv) {
  const m = /^# Source: RAWmakase data\/cameras\.toml @ ([0-9a-f]{40})(?: \((v[^)]+)\))?,/m.exec(tsv);
  return m ? { sha: m[1], tag: m[2] ?? null } : null;
}

/** The table rewritten from cameras.toml: our comments kept, the Source line and rows replaced. */
export function regenerate(tsv, toml, source, exceptions = EXCEPTIONS) {
  const lines = tsv.split('\n');
  const header = lines.slice(0, lines.findIndex((l) => !l.startsWith('#')));
  const at = header.findIndex((l) => l.startsWith('# Source:'));
  if (at === -1) throw new Error(`${TABLE} has no "# Source:" line in its header`);
  const rows = tableRows(parseCameras(toml), exceptions);
  header[at] = sourceLine(source, rows, exceptions);
  return `${[...header, ...rows].join('\n')}\n`;
}

/** cameras.toml at one commit. */
export async function fetchCameras(sha) {
  const url = `https://raw.githubusercontent.com/pch/rawmakase/${sha}/data/cameras.toml`;
  const response = await fetch(url, { signal: AbortSignal.timeout(20000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return response.text();
}

let listed;

/** RAWmakase's release tags (vX.Y.Z), oldest first, each with its commit; listed once per run. */
export function releases() {
  listed ??= listReleases();
  return listed;
}

async function listReleases() {
  const { stdout } = await promisify(execFile)('git', ['ls-remote', '--tags', `${REPO}.git`], {
    timeout: 15000,
    env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
  });
  const tags = new Map();
  for (const line of stdout.split('\n')) {
    const m = /^([0-9a-f]{40})\trefs\/tags\/(v\d+(?:\.\d+)*)(\^\{\})?$/.exec(line.trim());
    // An annotated tag is listed twice; its ^{} line is the commit.
    if (m && (m[3] || !tags.has(m[2]))) tags.set(m[2], m[1]);
  }
  return new Map([...tags].sort(([a], [b]) => a.localeCompare(b, 'en', { numeric: true })));
}

/** For check:merge: a warning when RAWmakase has released past the table, else null. */
export async function tableWarning(root) {
  // A checkout without the table (the e2e test's throwaway forks) has nothing to check.
  if (!existsSync(join(root, TABLE))) return null;
  const recorded = recordedSource(readFileSync(join(root, TABLE), 'utf8'));
  if (!recorded) return `${TABLE} names no RAWmakase source - node scripts/rawmakase-cameras.mjs <release>`;
  let tags;
  try {
    tags = await releases();
  } catch (error) {
    return `could not list RAWmakase's releases, so ${TABLE} is unchecked (${error.code ?? error.message})`;
  }
  const [newest, sha] = [...tags].at(-1) ?? [];
  if (!newest || sha === recorded.sha) return null;
  const at = recorded.tag ?? `commit ${recorded.sha.slice(0, 8)}, not a release`;
  return `RAWmakase ${newest} is out and the camera table is at ${at} - node scripts/rawmakase-cameras.mjs ${newest}`;
}

async function main(ref) {
  if (!ref) throw new Error('usage: node scripts/rawmakase-cameras.mjs <release tag, or full commit>');
  const tags = await releases();
  let source;
  if (tags.has(ref)) source = { sha: tags.get(ref), tag: ref };
  else if (/^[0-9a-f]{40}$/.test(ref)) source = { sha: ref, tag: [...tags].find(([, s]) => s === ref)?.[0] ?? null };
  else throw new Error(`${ref} is not a RAWmakase release tag or a full 40-character commit`);

  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const path = join(root, TABLE);
  const before = readFileSync(path, 'utf8');
  const after = regenerate(before, await fetchCameras(source.sha), source);
  writeFileSync(path, after);
  console.log(`  ${TABLE}: ${source.tag ?? 'commit'} ${source.sha.slice(0, 8)}`);
  console.log(before === after ? '  unchanged' : '  changed - git diff it, and show the changed bodies on photos before merging');
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  main(process.argv[2]).catch((error) => {
    console.error(`  ${error.message}`);
    process.exitCode = 1;
  });
}
