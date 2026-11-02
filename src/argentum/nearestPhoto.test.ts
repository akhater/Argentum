import { describe, expect, it } from 'vitest';
import { lastOrderHolding, nearestThatStays, stepFrom } from './nearestPhoto';

describe('the nearest photo that stays', () => {
  const order = ['a', 'b', 'c', 'd'];
  const except =
    (...gone: string[]) =>
    (path: string) =>
      !gone.includes(path);

  it('is the next one, else the one before', () => {
    expect(nearestThatStays(order, 'b', except('b'))).toBe('c');
    expect(nearestThatStays(order, 'b', except('b', 'c', 'd'))).toBe('a');
  });

  it('is none when nothing stays, or the photo is not in the order', () => {
    expect(nearestThatStays(order, 'b', () => false)).toBeNull();
    expect(nearestThatStays(order, 'x', () => true)).toBeNull();
  });
});

describe('the photo Left and Right go to', () => {
  const stood = ['a', 'b', 'c', 'd', 'e'];

  it('steps through the list and wraps at its ends, as long as the photo is in it', () => {
    expect(stepFrom(['a', 'b', 'c'], [], 'b', 1)).toBe('c');
    expect(stepFrom(['a', 'b', 'c'], [], 'b', -1)).toBe('a');
    expect(stepFrom(['a', 'b', 'c'], [], 'c', 1)).toBe('a');
    expect(stepFrom(['a', 'b', 'c'], [], 'a', -1)).toBe('c');
  });

  it('goes either way from where a photo stood once it has left the list', () => {
    expect(stepFrom(['a', 'b', 'd', 'e'], stood, 'c', 1)).toBe('d');
    expect(stepFrom(['a', 'b', 'd', 'e'], stood, 'c', -1)).toBe('b');
  });

  it('skips the photos that left with it', () => {
    expect(stepFrom(['a', 'e'], stood, 'c', 1)).toBe('e');
    expect(stepFrom(['a', 'e'], stood, 'c', -1)).toBe('a');
  });

  it('takes the nearest photo the other way when none is left in that direction', () => {
    expect(stepFrom(['a', 'b'], stood, 'c', 1)).toBe('b');
    expect(stepFrom(['d', 'e'], stood, 'c', -1)).toBe('d');
  });

  it('starts from the end of the list when nothing says where the photo stood', () => {
    expect(stepFrom(['a', 'b'], [], 'x', 1)).toBe('a');
    expect(stepFrom(['a', 'b'], [], 'x', -1)).toBe('b');
  });

  it('goes nowhere when the list is empty', () => {
    expect(stepFrom([], stood, 'c', 1)).toBeNull();
    expect(stepFrom([], stood, 'c', -1)).toBeNull();
    expect(stepFrom([], [], 'c', 1)).toBeNull();
  });

  it('takes the first photo going forward and stays going back when no photo is on show', () => {
    expect(stepFrom(['a', 'b'], [], null, 1)).toBe('a');
    expect(stepFrom(['a', 'b'], stood, null, -1)).toBeNull();
    expect(stepFrom([], [], null, 1)).toBeNull();
  });
});

describe('the order a photo stood in', () => {
  it('is the last list that held it, however many lists have come since', () => {
    const before = ['a', 'b', 'c'];
    const without = lastOrderHolding([], before, 'b');
    expect(without).toBe(before);
    expect(lastOrderHolding(without, ['a', 'c'], 'b')).toBe(before);
    expect(lastOrderHolding(without, ['c'], 'b')).toBe(before);
  });

  it('keeps what it has when no photo is on show', () => {
    expect(lastOrderHolding(['a'], ['b'], null)).toEqual(['a']);
  });
});
