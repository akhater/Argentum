import { describe, expect, it } from 'vitest';
import {
  CLOSE_WAIT_MS,
  EDIT_QUEUE_AFTER_MS,
  EDIT_SAVE_AFTER_MS,
  HISTORY_STEP_AFTER_MS,
  HISTORY_WRITE_AFTER_MS,
  PRESETS_SAVE_AFTER_MS,
} from './editTimings';

describe('edit timings', () => {
  it('keeps the values the editor was tuned with', () => {
    expect({
      EDIT_QUEUE_AFTER_MS,
      EDIT_SAVE_AFTER_MS,
      HISTORY_STEP_AFTER_MS,
      HISTORY_WRITE_AFTER_MS,
      PRESETS_SAVE_AFTER_MS,
      CLOSE_WAIT_MS,
    }).toEqual({
      EDIT_QUEUE_AFTER_MS: 50,
      EDIT_SAVE_AFTER_MS: 300,
      HISTORY_STEP_AFTER_MS: 500,
      HISTORY_WRITE_AFTER_MS: 600,
      PRESETS_SAVE_AFTER_MS: 500,
      CLOSE_WAIT_MS: 2000,
    });
  });

  it('queues an edit before it is saved', () => {
    expect(EDIT_QUEUE_AFTER_MS).toBeLessThan(EDIT_SAVE_AFTER_MS);
  });

  it('writes the History after the edit is saved and the step is made', () => {
    expect(HISTORY_WRITE_AFTER_MS).toBeGreaterThan(EDIT_SAVE_AFTER_MS);
    expect(HISTORY_WRITE_AFTER_MS).toBeGreaterThan(HISTORY_STEP_AFTER_MS);
  });
});
