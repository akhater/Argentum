/**
 * Where the photo on show gives way to, once it has gone or left the list. Ours.
 *
 * Paths, in the order the library shows them, so the rules are the same for a delete, for a rating
 * that hides a photo, and for Left and Right.
 */

/** The nearest path to `path` in `order` that stays: the next one, else the one before it; else none. */
export function nearestThatStays(
  order: readonly string[],
  path: string,
  stays: (path: string) => boolean,
): string | null {
  const index = order.indexOf(path);
  if (index === -1) return null;
  return order.slice(index + 1).find(stays) ?? order.slice(0, index).reverse().find(stays) ?? null;
}

/**
 * The photo Left (`step` -1) or Right (1) goes to from `path`. In `list`, the step wraps at its ends.
 * A photo that has left `list` steps from where it stood in `lastOrder` to the nearest photo still
 * in `list`, that way first; with no record of where it stood, Right takes the first photo and Left
 * the last. With no photo on show (`path` null), Right takes the first photo and Left stays, as there
 * is nothing before the start. An empty list goes nowhere.
 */
// RAWmakase 49755bc4: src/app/library/metadata.rs (a photo the filter hid gives way to the one shown)
export function stepFrom(
  list: readonly string[],
  lastOrder: readonly string[],
  path: string | null,
  step: 1 | -1,
): string | null {
  if (path === null) return step === 1 ? (list[0] ?? null) : null;
  const index = list.indexOf(path);
  if (index !== -1) return list[(index + step + list.length) % list.length];
  const shown = new Set(list);
  const ahead = step === 1 ? lastOrder : [...lastOrder].reverse();
  return nearestThatStays(ahead, path, (p) => shown.has(p)) ?? list.at(step === 1 ? 0 : -1) ?? null;
}

/** What to keep as the last list that held `path`, as `outgoing` makes way for the next list. */
export const lastOrderHolding = (kept: readonly string[], outgoing: readonly string[], path: string | null) =>
  path !== null && outgoing.includes(path) ? outgoing : kept;
