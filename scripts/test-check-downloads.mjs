// Tests for scripts/check-downloads.mjs, against a local server and the committed list.
//
// Run:  npm run test:checks   (or node scripts/test-check-downloads.mjs)

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';

import { answers, check, exitCodeFor, readList } from './check-downloads.mjs';

let ran = 0;
let failed = 0;
const test = async (name, fn) => {
  ran += 1;
  try {
    await fn();
    console.log(`  ok    ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`  FAIL  ${name}`);
    for (const line of String(error.message ?? error).split('\n')) console.log(`          ${line}`);
  }
};

const server = createServer((request, response) => {
  if (request.url === '/file') {
    response.writeHead(request.headers.range ? 206 : 200).end('x');
  } else {
    response.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;

await test('an address that serves its file answers; a missing one says why', async () => {
  assert.equal(await answers(`${base}/file`), null);
  assert.equal(await answers(`${base}/gone`), 'HTTP 404');
  assert.ok(await answers('http://127.0.0.1:9/nothing-listens-here'));
});

await test('a file is lost only when none of its addresses answers', async () => {
  const entries = [
    { name: 'kept.onnx', url: `${base}/gone` },
    { name: 'kept.onnx', url: `${base}/file` },
    { name: 'lost.onnx', url: `${base}/gone` },
    { name: 'lost.onnx', url: `${base}/also-gone` },
  ];
  const { results, lost } = await check(entries);
  assert.deepEqual(
    results.map((r) => r.error),
    ['HTTP 404', null, 'HTTP 404', 'HTTP 404'],
  );
  assert.deepEqual(lost, ['lost.onnx']);
});

await test('the run fails only when a file has no address left', () => {
  assert.equal(exitCodeFor([]), 0);
  assert.equal(exitCodeFor(['lost.onnx']), 1);
  assert.equal(exitCodeFor(['lost.onnx', 'also-lost.onnx']), 1);
});

await test('the committed list gives each file a second address', () => {
  const text = readFileSync(new URL('../src-tauri/download-addresses.tsv', import.meta.url), 'utf8');
  const entries = readList(text);
  assert.ok(entries.length > 40, `${entries.length} addresses`);
  const byName = new Map();
  for (const e of entries) byName.set(e.name, [...(byName.get(e.name) ?? []), e]);
  for (const [name, urls] of byName) {
    assert.ok(
      urls.every((e) => e.url.startsWith('https://')),
      name,
    );
    if (name.startsWith('omnilens_')) continue; // Argentum's own export: our copy is the only one
    assert.ok(urls.length >= 2, `${name} has one address`);
  }
});

server.close();
console.log(`\n  ${ran - failed}/${ran} passed`);
process.exitCode = failed ? 1 : 0;
