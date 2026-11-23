// The wiring between the interface and the rest of the app, and what the app
// is built from. A deletion that leaves a caller behind compiles and passes
// every test: the call fails only when someone clicks it. This catches that,
// and makes every removal and every new dependency show in a pull request:
//
//   * every command the frontend invokes is registered in lib.rs, every `ag`
//     name it calls has an arm in mods/dispatch.rs, and every `Invokes` value
//     names a registered command;
//   * every event the frontend listens for is one of the app's, named through
//     AG_EVENTS (generated from src-tauri/src/mods/events.rs), not typed as a string;
//   * every shortcut in Settings has a handler, and every handler a shortcut;
//   * the commands and `ag` arms nothing calls, the packages in Cargo.lock and
//     package-lock.json, and (with --dist) the bytes of the built frontend match
//     scripts/wiring-baseline.json. More fails: say why in the pull request, then
//     --update. Fewer also fails until --update, so a removal is kept.
//
//   node scripts/check-wiring.mjs                   check
//   node scripts/check-wiring.mjs --dist            also check dist/ (after `npm run build`)
//   node scripts/check-wiring.mjs --update [--dist] rewrite the baseline from the tree

import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join, relative, sep } from 'node:path';

/** The built frontend may grow or shrink by this much before the baseline must move. */
export const DIST_TOLERANCE = 0.01;

/** Command names in `tauri::generate_handler![...]`, without their module path. */
export function registeredCommands(libRs) {
  const list = /generate_handler!\[([^\]]*)\]/.exec(libRs)?.[1] ?? '';
  return new Set(
    list
      .split(',')
      .map((item) => item.trim().split('::').at(-1))
      .filter(Boolean),
  );
}

/** Names the `ag` command routes: the `"name" =>` arms (the `commands!` table's too), outside the tests module. */
export function dispatchArms(dispatchRs) {
  const code = dispatchRs.split(/^#\[cfg\(test\)\]\s*mod tests\b/m)[0];
  return new Set([...code.matchAll(/^\s*"([a-z0-9_]+)"\s*=>/gm)].map((m) => m[1]));
}

/** The `Invokes` enum: member -> command name. */
export function invokesEnum(appProperties) {
  const body = /export enum Invokes \{([^}]*)\}/.exec(appProperties)?.[1] ?? '';
  return new Map([...body.matchAll(/(\w+)\s*=\s*'([^']+)'/g)].map((m) => [m[1], m[2]]));
}

/**
 * What the frontend calls. `files` is [{ path, text }]. Returns the commands it
 * invokes by name and the `ag` names it calls (each name -> the files), and
 * every lowercase string literal it holds: a name kept in a variable before the
 * call (`const command = ... ? 'a' : 'b'`) still counts as a caller.
 */
export function frontendCalls(files, invokes) {
  const invoked = new Map();
  const agCalled = new Map();
  const literals = new Set();
  const add = (map, name, path) => map.set(name, [...(map.get(name) ?? []), path]);

  for (const { path, text } of files) {
    const code = text.replace(/export enum Invokes \{[^}]*\}/, '');
    for (const m of code.matchAll(/\binvoke(?:<[^()]*?>)?\(\s*['"]([a-z0-9_]+)['"]/g)) add(invoked, m[1], path);
    for (const m of code.matchAll(/\bInvokes\.(\w+)/g)) add(invoked, invokes.get(m[1]) ?? `Invokes.${m[1]}`, path);
    // A file that names its translation function `ag` is not calling the command.
    if (!/\bconst ag\s*=/.test(code)) {
      for (const m of code.matchAll(/(?<![\w$])(?:transport\.)?ag(?:<[^()]*?>)?\(\s*['"]([a-z0-9_]+)['"]/g)) {
        add(agCalled, m[1], path);
      }
    }
    for (const m of code.matchAll(/['"`]([a-z][a-z0-9_]*)['"`]/g)) literals.add(m[1]);
  }
  return { invoked, agCalled, literals };
}

/** `AG_EVENTS` in the generated contract: constant -> event name. */
export function eventNames(generated) {
  const body = /export const AG_EVENTS = \{([^}]*)\}/.exec(generated)?.[1] ?? '';
  return new Map([...body.matchAll(/(\w+):\s*'([^']+)'/g)].map((m) => [m[1], m[2]]));
}

/**
 * The events the frontend listens for: a name typed as a string (`listen('x'`,
 * `once('x'`), and each `AG_EVENTS.KEY` it uses. Each maps to the files.
 */
export function frontendListens(files) {
  const typed = new Map();
  const keys = new Map();
  const add = (map, name, path) => map.set(name, [...(map.get(name) ?? []), path]);
  for (const { path, text } of files) {
    for (const m of text.matchAll(/(?<![\w$.])(?:listen|once)(?:<[^()]*?>)?\(\s*['"]([^'"]+)['"]/g)) {
      add(typed, m[1], path);
    }
    for (const m of text.matchAll(/\bAG_EVENTS\.(\w+)/g)) add(keys, m[1], path);
  }
  return { typed, keys };
}

/** Actions listed in KEYBIND_DEFINITIONS. */
export function keybindActions(keyboardUtils) {
  return new Set([...keyboardUtils.matchAll(/\baction:\s*'([a-z0-9_]+)'/g)].map((m) => m[1]));
}

/** Actions useKeyboardShortcuts handles: the keys of its `actions` object. */
export function shortcutHandlers(useKeyboardShortcuts) {
  const start = useKeyboardShortcuts.indexOf('const actions: Record<string, any> = {');
  if (start < 0) return new Set();
  // The handlers sit one level inside the object, at six spaces.
  const rest = useKeyboardShortcuts.slice(start).split(/\n {4}\};/)[0];
  return new Set([...rest.matchAll(/^ {6}([a-z0-9_]+): \{/gm)].map((m) => m[1]));
}

export const cargoPackages = (cargoLock) => (cargoLock.match(/^\[\[package\]\]$/gm) ?? []).length;

export const npmPackages = (packageLock) => Object.keys(JSON.parse(packageLock).packages ?? {}).filter(Boolean).length;

/** A built file's path with Vite's content hash taken out: `assets/App-Dz6BWnDZ.js` -> `assets/App.js`. */
export const distKey = (path) => path.replace(/-[A-Za-z0-9_-]{8}(\.\w+)$/, '$1');

/** Bytes per file of a build folder, keyed by `distKey`. */
export function distBytes(dir) {
  const bytes = {};
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const full = join(folder, entry.name);
      if (entry.isDirectory()) walk(full);
      else {
        const key = distKey(relative(dir, full).split(sep).join('/'));
        bytes[key] = (bytes[key] ?? 0) + statSync(full).size;
      }
    }
  };
  walk(dir);
  return Object.fromEntries(Object.entries(bytes).sort());
}

const sum = (bytes) => Object.values(bytes).reduce((a, b) => a + b, 0);
const sorted = (set) => [...set].sort();

/** Everything measured, in the baseline's shape. */
export function measure({ registered, arms, calls, cargoLock, packageLock, dist }) {
  const mentioned = (name, called) => called.has(name) || calls.literals.has(name);
  return {
    uncalledCommands: sorted([...registered].filter((name) => name !== 'ag' && !mentioned(name, calls.invoked))),
    uncalledAg: sorted([...arms].filter((name) => !mentioned(name, calls.agCalled))),
    cargoPackages: cargoPackages(cargoLock),
    npmPackages: npmPackages(packageLock),
    ...(dist ? { dist } : {}),
  };
}

/** Wiring that is broken now, whatever the baseline says. */
export function brokenWiring({ registered, arms, invokes, calls, actions, handlers, events, listens }) {
  const broken = [];
  const eventList = new Set(events.values());
  for (const [name, paths] of listens.typed) {
    broken.push(
      eventList.has(name)
        ? `${paths[0]} listens for '${name}' as a string: use AG_EVENTS (src/argentum/ag.generated.ts)`
        : `${paths[0]} listens for '${name}', which mods/events.rs does not list`,
    );
  }
  for (const [key, paths] of listens.keys) {
    if (!events.has(key)) broken.push(`${paths[0]} uses AG_EVENTS.${key}, which mods/events.rs does not list`);
  }
  for (const [name, paths] of calls.invoked) {
    if (!registered.has(name)) broken.push(`${paths[0]} invokes ${name}, which lib.rs does not register`);
  }
  for (const [member, name] of invokes) {
    if (!registered.has(name)) broken.push(`Invokes.${member} names ${name}, which lib.rs does not register`);
  }
  for (const [name, paths] of calls.agCalled) {
    if (!arms.has(name)) broken.push(`${paths[0]} calls ag('${name}'), which mods/dispatch.rs does not route`);
  }
  for (const action of actions) {
    if (!handlers.has(action)) broken.push(`shortcut ${action} is in Settings but useKeyboardShortcuts has no handler`);
  }
  for (const action of handlers) {
    if (!actions.has(action)) broken.push(`useKeyboardShortcuts handles ${action}, which no shortcut sends`);
  }
  return broken;
}

/** Lines saying where `now` differs from `before`: { worse, better }. */
export function compareBaseline(before, now) {
  const worse = [];
  const better = [];
  for (const [key, what] of [
    ['uncalledCommands', 'command nothing calls'],
    ['uncalledAg', 'ag arm nothing calls'],
  ]) {
    for (const name of now[key]) if (!before[key].includes(name)) worse.push(`${what}: ${name}`);
    for (const name of before[key]) if (!now[key].includes(name)) better.push(`${what} gone: ${name}`);
  }
  for (const [key, what] of [
    ['cargoPackages', 'Cargo.lock packages'],
    ['npmPackages', 'package-lock.json packages'],
  ]) {
    if (now[key] > before[key]) worse.push(`${what}: ${before[key]} -> ${now[key]}`);
    if (now[key] < before[key]) better.push(`${what}: ${before[key]} -> ${now[key]}`);
  }
  if (now.dist && before.dist) {
    const was = sum(before.dist);
    const is = sum(now.dist);
    const line = `dist: ${was} -> ${is} bytes (${(((is - was) / was) * 100).toFixed(1)}%)`;
    if (is > was * (1 + DIST_TOLERANCE)) worse.push(line);
    if (is < was * (1 - DIST_TOLERANCE)) better.push(line);
  }
  return { worse, better };
}

function frontendFiles(root) {
  const files = [];
  const walk = (folder) => {
    for (const entry of readdirSync(join(root, folder), { withFileTypes: true })) {
      const path = `${folder}/${entry.name}`;
      if (entry.isDirectory()) {
        if (entry.name !== 'tests') walk(path);
      } else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
        files.push({ path, text: readFileSync(join(root, path), 'utf8') });
      }
    }
  };
  walk('src');
  return files;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..');
  const read = (file) => readFileSync(join(root, file), 'utf8');
  const baselinePath = join(root, 'scripts', 'wiring-baseline.json');
  const withDist = process.argv.includes('--dist');

  const registered = registeredCommands(read('src-tauri/src/lib.rs'));
  const arms = dispatchArms(read('src-tauri/src/mods/dispatch.rs'));
  const invokes = invokesEnum(read('src/components/ui/AppProperties.tsx'));
  const calls = frontendCalls(frontendFiles(root), invokes);
  const actions = keybindActions(read('src/utils/keyboardUtils.ts'));
  const handlers = shortcutHandlers(read('src/hooks/useKeyboardShortcuts.ts'));
  const events = eventNames(read('src/argentum/ag.generated.ts'));
  const listens = frontendListens(frontendFiles(root));
  const now = measure({
    registered,
    arms,
    calls,
    cargoLock: read('src-tauri/Cargo.lock'),
    packageLock: read('package-lock.json'),
    dist: withDist ? distBytes(join(root, 'dist')) : null,
  });
  const before = JSON.parse(readFileSync(baselinePath, 'utf8'));

  if (process.argv.includes('--update')) {
    writeFileSync(baselinePath, `${JSON.stringify({ ...now, dist: now.dist ?? before.dist }, null, 2)}\n`);
    // As Prettier writes it, or the frontend check counts it as unformatted.
    // Only --update needs it, and only where node_modules is installed.
    spawnSync(process.execPath, [
      join(root, 'node_modules', 'prettier', 'bin', 'prettier.cjs'),
      '--write',
      baselinePath,
    ]);
    console.log('Wrote scripts/wiring-baseline.json.');
    return;
  }

  const broken = brokenWiring({ registered, arms, invokes, calls, actions, handlers, events, listens });
  const { worse, better } = compareBaseline(before, now);
  if (broken.length > 0) {
    console.error(`Wiring check: ${broken.length} broken call(s):\n`);
    for (const line of broken) console.error(`  ${line}`);
    process.exitCode = 1;
  }
  if (worse.length > 0) {
    console.error(`\nWiring check: ${worse.length} change(s) the baseline does not have:\n`);
    for (const line of worse) console.error(`  ${line}`);
    console.error(
      '\nSay why in the pull request, then: node scripts/check-wiring.mjs --update' + (withDist ? ' --dist' : ''),
    );
    process.exitCode = 1;
  } else if (better.length > 0) {
    console.error(`\nWiring check: ${better.length} removal(s) to keep:\n`);
    for (const line of better) console.error(`  ${line}`);
    console.error('\nKeep the gain: node scripts/check-wiring.mjs --update' + (withDist ? ' --dist' : ''));
    process.exitCode = 1;
  }
  if (!process.exitCode) {
    const dist = now.dist ? `, dist ${sum(now.dist)} bytes` : '';
    console.log(
      `Wiring check: ${registered.size} commands, ${arms.size} ag arms, ${actions.size} shortcuts wired; ` +
        `${now.cargoPackages} crates, ${now.npmPackages} npm packages${dist}.`,
    );
  }
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) main();
