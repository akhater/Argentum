import { describe, expect, it } from 'vitest';
import { typingInto } from './typingInto';

// No DOM in these tests: a target is whatever has the properties the rule reads.
const el = (tagName: string, extra: Record<string, unknown> = {}) => ({ tagName, isContentEditable: false, ...extra });

describe('typingInto', () => {
  it.each([
    ['text input', el('INPUT', { type: 'text' })],
    ['input without a type', el('INPUT')],
    ['number input', el('INPUT', { type: 'number' })],
    ['search input', el('INPUT', { type: 'search' })],
    ['textarea', el('TEXTAREA')],
    ['contenteditable', el('DIV', { isContentEditable: true })],
    ['select', el('SELECT')],
    ['range slider (its own arrow keys, Home/End, PageUp/PageDown stay with it)', el('INPUT', { type: 'range' })],
  ])('%s is typing', (_name, target) => {
    expect(typingInto(target)).toBe(true);
  });

  it.each([
    ['checkbox (a switch)', el('INPUT', { type: 'checkbox' })],
    ['radio', el('INPUT', { type: 'radio' })],
    ['button', el('BUTTON')],
    ['body', el('BODY')],
    ['plain div', el('DIV')],
    ['nothing focused', null],
  ])('%s is not typing', (_name, target) => {
    expect(typingInto(target)).toBe(false);
  });
});
