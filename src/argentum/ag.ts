/**
 * Calling Argentum's Rust side. Ours.
 *
 * Every Argentum command goes through one Tauri command, `ag`, rather than
 * being registered individually in `lib.rs` — see `mods/dispatch.rs`. This
 * hides the envelope so callers write what they mean.
 *
 *     const wb = await ag('detect_auto_white_balance', { jsAdjustments, mode });
 *
 * The name, the arguments and the answer are typed by `AgCommands`, which
 * `ag.generated.ts` writes from the Rust side's own types (the `commands!`
 * table in `mods/dispatch.rs`): a name the app does not route, an argument it
 * does not take, or an answer read as something it is not fails to compile.
 */

import { invoke } from '@tauri-apps/api/core';
import type { AgCommands } from './ag.generated';

export type AgName = keyof AgCommands;
export type AgArgs<K extends AgName> = AgCommands[K]['args'];
export type AgAnswer<K extends AgName> = AgCommands[K]['answer'];

/** A command that takes nothing is called with its name alone. */
type ArgsOf<K extends AgName> = AgArgs<K> extends Record<string, never> ? [] : [args: AgArgs<K>];

export function ag<K extends AgName>(name: K, ...[args]: ArgsOf<K>): Promise<AgAnswer<K>> {
  return agUntyped<AgAnswer<K>>(name, args);
}

/**
 * The envelope itself, for a caller that types its commands another way: the
 * new interface's engine client (`src/shell/engine/tauri.ts`), whose `engine_*`
 * commands are typed by the engine's contract and are not in `AgCommands`.
 */
export function agUntyped<T>(name: string, args: object = {}): Promise<T> {
  return invoke<T>('ag', { name, args });
}
