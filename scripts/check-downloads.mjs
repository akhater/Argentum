// Asks every address Argentum downloads from whether it still answers. Ours.
//
// The addresses are the ones the build and the app use: a test in
// src-tauri/src/mods/mirror.rs writes them to src-tauri/download-addresses.tsv
// and fails when the committed list is out of date. Each file has two, our copy
// first and its original second, so one that stops answering is a WARNING and
// never a failure: hosts and networks flake, and the other address still serves
// the file. A file with no address left is the one to act on: the run exits 1.
//
// Run:  npm run check:downloads   (weekly in CI: .github/workflows/downloads.yml)
// Test: node scripts/test-check-downloads.mjs
import { appendFileSync, readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const LIST = new URL('../src-tauri/download-addresses.tsv', import.meta.url);

/** How long one address gets to answer. */
const TIMEOUT_MS = 30_000;

/** Addresses asked at once. */
const AT_ONCE = 6;

/** The list's `name<TAB>address` lines as [{ name, url }], comments and blanks left out. */
export const readList = (text) =>
  text
    .split(/\r?\n/)
    .filter((line) => line.trim() && !line.startsWith('#'))
    .map((line) => {
      const [name, url] = line.split('\t');
      return { name, url };
    });

/** Whether `url` serves its file: its first byte is asked for, nothing more. */
export async function answers(url) {
  try {
    const response = await fetch(url, {
      headers: { Range: 'bytes=0-0', 'User-Agent': 'Argentum check-downloads' },
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
    await response.body?.cancel();
    return response.status === 200 || response.status === 206 ? null : `HTTP ${response.status}`;
  } catch (error) {
    return String(error.cause?.code ?? error.message ?? error);
  }
}

/**
 * Every entry asked through `ask` (url -> null, or why it failed), a few at a time. Returns
 * the entries with their `error`, and the names left with no address that answers.
 */
export async function check(entries, ask = answers) {
  const results = [];
  for (let at = 0; at < entries.length; at += AT_ONCE) {
    const batch = entries.slice(at, at + AT_ONCE);
    results.push(...(await Promise.all(batch.map(async (e) => ({ ...e, error: await ask(e.url) })))));
  }
  const names = [...new Set(results.map((r) => r.name))];
  const lost = names.filter((name) => results.every((r) => r.name !== name || r.error));
  return { results, lost };
}

/** The run's exit code: 1 when a file has no address left (its download fails), else 0. */
export const exitCodeFor = (lost) => (lost.length > 0 ? 1 : 0);

async function main() {
  const { results, lost } = await check(readList(readFileSync(LIST, 'utf8')));
  const failed = results.filter((r) => r.error);
  for (const r of failed) console.log(`  warning: ${r.name} does not answer at ${r.url} (${r.error})`);
  for (const name of lost) console.log(`  WARNING: ${name} has no address that answers: the download fails`);
  process.exitCode = exitCodeFor(lost);
  console.log(`Download addresses: ${results.length - failed.length} of ${results.length} answer.`);
  if (process.env.GITHUB_ACTIONS && failed.length) {
    for (const r of failed) console.log(`::warning::${r.name} does not answer at ${r.url} (${r.error})`);
    const lines = failed.map((r) => `- \`${r.name}\`: ${r.url} (${r.error})`);
    const head = lost.length ? `${lost.length} file(s) with no address left` : 'every file still has an address';
    appendFileSync(
      process.env.GITHUB_STEP_SUMMARY,
      `## Download addresses: ${failed.length} not answering, ${head}\n\n${lines.join('\n')}\n`,
    );
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) await main();
