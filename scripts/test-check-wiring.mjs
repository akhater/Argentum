// Tests for scripts/check-wiring.mjs.
//
// The ones that matter: today's tree is wired, and each kind of broken wiring
// (a call to a command that is gone, a shortcut with no handler) is reported.
//
// Run:  npm run test:checks   (or node scripts/test-check-wiring.mjs)

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  brokenWiring,
  compareBaseline,
  dispatchArms,
  eventNames,
  distKey,
  frontendCalls,
  frontendListens,
  invokesEnum,
  keybindActions,
  registeredCommands,
  shortcutHandlers,
} from './check-wiring.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (file) => readFileSync(join(root, file), 'utf8');

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

const registered = registeredCommands(read('src-tauri/src/lib.rs'));
const arms = dispatchArms(read('src-tauri/src/mods/dispatch.rs'));
const invokes = invokesEnum(read('src/components/ui/AppProperties.tsx'));
const actions = keybindActions(read('src/utils/keyboardUtils.ts'));
const handlers = shortcutHandlers(read('src/hooks/useKeyboardShortcuts.ts'));
const events = eventNames(read('src/argentum/ag.generated.ts'));
const wiring = (files, overrides = {}) => ({
  events,
  listens: frontendListens(files),
  registered,
  arms,
  invokes,
  actions,
  handlers,
  calls: frontendCalls(files, invokes),
  ...overrides,
});

test('the parsers find what the tree holds', () => {
  assert.ok(registered.size > 100 && registered.has('ag') && registered.has('load_settings'));
  assert.ok(!registered.has('lut_processing::list_luts') && registered.has('list_luts'));
  assert.ok(arms.has('engine_frame') && arms.has('detect_auto_white_balance'));
  assert.ok(!arms.has('sample_processed_pixel:'), 'a test string is not an arm');
  assert.equal(invokes.get('LoadSettings'), 'load_settings');
  assert.ok(actions.has('open_image') && actions.has('rate_0') && actions.has('undo'));
  assert.deepEqual([...handlers].sort(), [...actions].sort());
});

test("today's tree is wired", () => {
  const files = readdirSync(join(root, 'src'), { recursive: true })
    .filter((path) => /\.tsx?$/.test(path) && !/\.test\.tsx?$/.test(path))
    .map((path) => ({ path, text: read(join('src', path)) }));
  assert.deepEqual(brokenWiring(wiring(files)), []);
});

test('a call to a command lib.rs does not register is reported', () => {
  const files = [{ path: 'src/a.ts', text: "await invoke<string>('gone_command', { path });" }];
  assert.deepEqual(brokenWiring(wiring(files)), ['src/a.ts invokes gone_command, which lib.rs does not register']);
});

test('an Invokes value naming a command that is gone is reported', () => {
  const withGone = new Map([...invokes, ['Gone', 'gone_command']]);
  assert.deepEqual(brokenWiring(wiring([], { invokes: withGone })), [
    'Invokes.Gone names gone_command, which lib.rs does not register',
  ]);
});

test('an ag name with no arm is reported, a translation function named ag is not', () => {
  const files = [
    { path: 'src/b.ts', text: "import { ag } from './ag';\nawait ag<number>('gone_arm', {});" },
    { path: 'src/c.tsx', text: "const ag = useAgTranslation();\nreturn ag('gone_key');" },
  ];
  assert.deepEqual(brokenWiring(wiring(files)), [
    "src/b.ts calls ag('gone_arm'), which mods/dispatch.rs does not route",
  ]);
});

test('the event names are read from the generated file', () => {
  assert.ok(events.size > 30 && events.get('WGPU_FRAME_READY') === 'wgpu-frame-ready');
});

test('a listener for an event the app does not name, or typed as a string, is reported', () => {
  const files = [
    { path: 'src/e.ts', text: "listen('gone-event', () => {});" },
    { path: 'src/f.ts', text: "listen<number>('wgpu-frame-ready', () => {});" },
    { path: 'src/g.ts', text: 'listen(AG_EVENTS.GONE_EVENT, () => {});' },
    { path: 'src/h.ts', text: 'listen(AG_EVENTS.WGPU_FRAME_READY, () => {}); once(PROGRESS_EVENT, () => {});' },
  ];
  assert.deepEqual(brokenWiring(wiring(files)), [
    "src/e.ts listens for 'gone-event', which mods/events.rs does not list",
    "src/f.ts listens for 'wgpu-frame-ready' as a string: use AG_EVENTS (src/argentum/ag.generated.ts)",
    'src/g.ts uses AG_EVENTS.GONE_EVENT, which mods/events.rs does not list',
  ]);
});

test('a shortcut with no handler, and a handler with no shortcut, are reported', () => {
  const lost = new Set([...handlers].filter((a) => a !== 'undo'));
  const extra = new Set([...actions].filter((a) => a !== 'redo'));
  assert.deepEqual(brokenWiring(wiring([], { handlers: lost, actions: extra })), [
    'shortcut undo is in Settings but useKeyboardShortcuts has no handler',
    'useKeyboardShortcuts handles redo, which no shortcut sends',
  ]);
});

test('a command name held in a variable before the call counts as called', () => {
  const calls = frontendCalls(
    [{ path: 'src/d.ts', text: "const command = x ? 'copy_files' : 'move_files';" }],
    invokes,
  );
  assert.ok(calls.literals.has('copy_files') && calls.literals.has('move_files'));
});

test('the baseline comparison says what got worse and what was removed', () => {
  const before = {
    uncalledCommands: ['old'],
    uncalledAg: [],
    cargoPackages: 10,
    npmPackages: 5,
    dist: { 'assets/App.js': 1000 },
  };
  assert.deepEqual(compareBaseline(before, { ...before, dist: { 'assets/App.js': 1009 } }), { worse: [], better: [] });
  const { worse, better } = compareBaseline(before, {
    uncalledCommands: ['new'],
    uncalledAg: [],
    cargoPackages: 11,
    npmPackages: 4,
    dist: { 'assets/App.js': 1020 },
  });
  assert.deepEqual(worse, [
    'command nothing calls: new',
    'Cargo.lock packages: 10 -> 11',
    'dist: 1000 -> 1020 bytes (2.0%)',
  ]);
  assert.deepEqual(better, ['command nothing calls gone: old', 'package-lock.json packages: 5 -> 4']);
});

test("Vite's content hash is taken out of a built file's name", () => {
  assert.equal(distKey('assets/App-Dz6BWnDZ.js'), 'assets/App.js');
  assert.equal(distKey('assets/undo-2-Ab_c-123.js'), 'assets/undo-2.js');
  assert.equal(distKey('splash-dark.jpg'), 'splash-dark.jpg');
});

console.log('');
console.log(`  ${ran - failed}/${ran} passed`);
process.exitCode = failed > 0 ? 1 : 0;
