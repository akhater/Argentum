// The camera colour mark 26.44.1 to 26.45.x stored in new edits (agDngColour, mods/dng_colour.rs):
// every photo takes that colour now, so the editor starts without it and drops a stored one,
// and the next save leaves it out.
import { describe, expect, it } from 'vitest';
import { INITIAL_ADJUSTMENTS, normalizeLoadedAdjustments, type Adjustments } from '../utils/adjustments';

const loaded = (edit: Record<string, unknown>) => normalizeLoadedAdjustments(edit as unknown as Adjustments);

describe('the retired camera colour mark', () => {
  it('is not in a new edit', () => {
    expect('agDngColour' in INITIAL_ADJUSTMENTS).toBe(false);
  });

  it('is dropped from a stored edit, and nothing else is', () => {
    for (const mark of [true, false]) {
      const edit = loaded({ exposure: 0.5, agDngColour: mark });
      expect('agDngColour' in edit).toBe(false);
      expect(edit.exposure).toBe(0.5);
    }
  });
});
