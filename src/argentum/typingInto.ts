/**
 * Is focus where keys are text? The one rule every shortcut path uses. Ours.
 *
 * Typing: a text-like input, a textarea, a select (its keys pick an option), a
 * range input (its arrow keys, Home, End and PageUp/PageDown move the slider, so
 * shortcuts stay out as they always did) and a contenteditable. Not typing: a
 * checkbox or radio (a switch, whose keys are not text), a button, the body.
 * Read by property, so it answers the same for a real element and for the plain
 * object a test passes.
 */

const NOT_TEXT_INPUT = new Set(['checkbox', 'radio']);

interface Focusable {
  tagName?: string;
  isContentEditable?: boolean;
  type?: string;
}

export function typingInto(target: unknown): boolean {
  const el = target as Focusable | null;
  if (!el || typeof el.tagName !== 'string') return false;
  if (el.isContentEditable) return true;
  switch (el.tagName.toUpperCase()) {
    case 'TEXTAREA':
    case 'SELECT':
      return true;
    case 'INPUT':
      return !NOT_TEXT_INPUT.has((el.type ?? 'text').toLowerCase());
    default:
      return false;
  }
}
