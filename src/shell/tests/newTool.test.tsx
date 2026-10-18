// A tool the engine adds is in the panels with no change to the interface:
// no code, no strings, no registration. And the interface holds no code for
// any particular tool.

import { describe, expect, it } from 'vitest';

import type { ToolDescriptor } from '../contract';
import { FAKE_TOOLS } from '../engine/fake';
import { applies, openShell, renderPanels, settle } from './helpers';

/** A tool the shell has never heard of, with one setting of every kind. */
const GRAIN: ToolDescriptor = {
  id: 'rm_grain',
  origin: 'rm',
  label: 'tools.rm_grain.label',
  section: 'effects',
  order: 10,
  stage: 'display',
  version: 1,
  settings: [
    {
      key: 'amount',
      label: 'tools.rm_grain.amount',
      kind: 'float',
      default: 0,
      min: 0,
      max: 100,
      step: 1,
      advanced: false,
    },
    { key: 'size', label: 'tools.rm_grain.size', kind: 'int', default: 25, min: 1, max: 100, step: 1, advanced: false },
    { key: 'monochrome', label: 'tools.rm_grain.monochrome', kind: 'bool', default: true, advanced: false },
    {
      key: 'film',
      label: 'tools.rm_grain.film',
      kind: 'choice',
      default: 'fine',
      choices: [
        { value: 'fine', label: 'tools.rm_grain.film.fine' },
        { value: 'coarse', label: 'tools.rm_grain.film.coarse' },
      ],
      advanced: false,
    },
    {
      key: 'roughness',
      label: 'tools.rm_grain.roughness',
      kind: 'float',
      default: 50,
      min: 0,
      max: 100,
      advanced: true,
    },
  ],
};

describe('a new engine tool', () => {
  it('gets its panel, labels and widgets from its descriptor alone', async () => {
    const { store } = await openShell({ tools: [...FAKE_TOOLS, GRAIN] });
    const html = renderPanels(store);
    expect(html).toContain('data-section="effects"');
    expect(html).toContain('data-tool="rm_grain"');
    expect(html).toContain('Effects');
    expect(html).toContain('Grain');
    for (const kind of ['float', 'int', 'bool', 'choice']) {
      expect(html).toMatch(new RegExp(`data-setting="[a-z]+" data-kind="${kind}"`));
    }
    expect(html).toContain('aria-label="Monochrome"');
    expect(html).toContain('<option value="coarse">Coarse</option>');
    expect(html).not.toContain('data-setting="roughness"');
  });

  it('is edited, undone and sent to the engine like any other', async () => {
    const { engine, store } = await openShell({ tools: [...FAKE_TOOLS, GRAIN] });
    store.getState().setSetting('rm_grain', 'film', 'coarse');
    store.getState().setSetting('rm_grain', 'amount', 30);
    store.getState().undo();
    await settle();
    expect(applies(engine).slice(-3)).toEqual([
      { rm_grain: { film: 'coarse' } },
      { rm_grain: { amount: 30 } },
      { rm_grain: { amount: 0 } },
    ]);
  });

  it('needs nothing in the interface, which names no tool', () => {
    // Every source file of the shell, as text. The stand-in engine and the
    // locale strings may know tools; nothing else may.
    const sources = import.meta.glob(
      ['../**/*.{ts,tsx,css}', '!../engine/**', '!../tests/**', '!../contract.generated.ts'],
      {
        query: '?raw',
        import: 'default',
        eager: true,
      },
    ) as Record<string, string>;
    expect(Object.keys(sources).length).toBeGreaterThan(10);
    for (const [file, source] of Object.entries(sources)) {
      for (const tool of FAKE_TOOLS) {
        expect(source.includes(tool.id), `${file} names ${tool.id}`).toBe(false);
      }
    }
  });
});
