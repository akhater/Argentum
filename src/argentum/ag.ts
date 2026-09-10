/**
 * Calling Argentum's Rust side. Ours.
 *
 * Every Argentum command goes through one Tauri command, `ag`, because
 * registering them individually costs a line of upstream `lib.rs` per feature —
 * see `mods/dispatch.rs` for why that matters. This hides the envelope so
 * callers write what they mean.
 *
 *     const wb = await ag<SolvedWhiteBalance>('solve_white_balance_at_point', {
 *       x, y, jsAdjustments,
 *     });
 *
 * Arguments go over as the object you pass, camelCase intact; the dispatcher
 * deserialises them into a typed struct on the other side.
 */

import { invoke } from '@tauri-apps/api/core';

export function ag<T>(name: string, args: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>('ag', { name, args });
}
