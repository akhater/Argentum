// Panels come from descriptors: sections, tools, rows and widgets.

import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import type { SettingSpec } from '../contract';
import { FAKE_SECTIONS, FAKE_TOOLS } from '../engine/fake';
import { buildSections, railPosition, railValue } from '../panels/model';
import { Slider } from '../widgets/Slider';
import { openShell, renderPanels } from './helpers';

const count = (html: string, needle: string) => html.split(needle).length - 1;
const rowOf = (html: string, key: string) => html.slice(html.indexOf(`data-setting="${key}"`)).split('</div></div>')[0];

describe('panels from descriptors', () => {
  it('show sections in the order and with the labels the engine gives, leaving out empty ones', () => {
    const sections = buildSections(FAKE_SECTIONS, FAKE_TOOLS);
    expect(sections.map((s) => [s.id, s.label, s.tools.map((t) => t.id)])).toEqual([
      ['basic', 'sections.basic', ['ag_exposure_wb']],
      ['tone', 'sections.tone', ['dt_highlights']],
      ['detail', 'sections.detail', ['rt_capture_sharpen']],
    ]);
    const reversed = buildSections([...FAKE_SECTIONS].reverse(), FAKE_TOOLS);
    expect(reversed.map((s) => s.id)).toEqual(['detail', 'tone', 'basic']);
  });

  it('give every tool its block and every setting a row with the widget of its kind', async () => {
    const { store } = await openShell();
    const html = renderPanels(store);
    for (const tool of FAKE_TOOLS) {
      expect(html).toContain(`data-tool="${tool.id}"`);
      for (const spec of tool.settings.filter((s) => !s.advanced)) {
        expect(html).toContain(`data-setting="${spec.key}" data-kind="${spec.kind}"`);
      }
    }
    const shown = FAKE_TOOLS.flatMap((t) => t.settings.filter((s) => !s.advanced));
    expect(count(html, 'role="slider"')).toBe(shown.filter((s) => s.kind === 'float' || s.kind === 'int').length);
    expect(count(html, 'role="switch"')).toBe(shown.filter((s) => s.kind === 'bool').length);
  });

  it('keep advanced settings under More', async () => {
    const { store } = await openShell();
    const html = renderPanels(store);
    expect(html).not.toContain('data-setting="iterations"');
    expect(html).toContain('More');
  });

  it('label tools and settings from the locale, and values from the edit', async () => {
    const { store } = await openShell();
    store.getState().setSetting('ag_exposure_wb', 'exposure', 1.25);
    const html = renderPanels(store);
    expect(html).toContain('Capture sharpening');
    expect(html).toContain('Exposure &amp; white balance');
    expect(html).toContain('value="1.25"');
    expect(html).toContain('aria-valuetext="+1.25 EV"');
  });

  it('fold a section', async () => {
    const { store } = await openShell();
    store.getState().toggleSection('detail');
    const html = renderPanels(store);
    expect(html).not.toContain('data-tool="rt_capture_sharpen"');
    expect(html).toContain('data-tool="ag_exposure_wb"');
  });
});

const multiplier: SettingSpec = {
  key: 'gain',
  label: 'gain',
  kind: 'float',
  default: 1,
  min: 0.05,
  max: 20,
  step: 0.001,
  scale: 'log',
  advanced: false,
};

describe('a log setting', () => {
  it('puts 1 mid-rail on a 0.05 to 20 range, and maps back', () => {
    expect(railPosition(multiplier, 1)).toBeCloseTo(0.5, 10);
    expect(railPosition(multiplier, 0.05)).toBe(0);
    expect(railPosition(multiplier, 20)).toBe(1);
    expect(railPosition(multiplier, 2) - 0.5).toBeCloseTo(0.5 - railPosition(multiplier, 0.5), 10);
    for (const p of [0, 0.2, 0.5, 0.9, 1])
      expect(railPosition(multiplier, railValue(multiplier, p))).toBeCloseTo(p, 10);
  });

  it('stays even when linear', () => {
    const linear = { ...multiplier, scale: 'linear' as const };
    expect(railPosition(linear, 1)).toBeCloseTo(0.95 / 19.95, 10);
    expect(railValue(linear, 0.5)).toBeCloseTo(10.025, 10);
  });

  it('draws its thumb by ratio, for any tool whose descriptor says log', async () => {
    const html = renderToStaticMarkup(
      <Slider spec={multiplier} label="Gain" value={1} onChange={() => {}} onDone={() => {}} />,
    );
    expect(html).toContain('class="ags-rail-thumb" style="left:50%"');
    const { store } = await openShell();
    const panels = renderPanels(store);
    const logs = FAKE_TOOLS.flatMap((t) => t.settings.filter((x) => x.scale === 'log' && !x.advanced));
    expect(logs.length).toBeGreaterThan(0);
    for (const spec of logs) expect(rowOf(panels, spec.key)).toContain('class="ags-rail-thumb" style="left:50%"');
  });
});

describe('enabledWhen', () => {
  const disabledKeys = (html: string) =>
    [...html.matchAll(/data-setting="([A-Za-z]+)"[^]*?aria-disabled="true"/g)].map((m) => m[1]);

  it('disables a setting while its condition is false, and enables it once it holds', async () => {
    const { store } = await openShell();
    const dependent = FAKE_TOOLS.flatMap((t) =>
      t.settings.filter((x) => x.enabledWhen && !x.advanced).map((x) => [t, x] as const),
    );
    expect(dependent.length).toBeGreaterThan(0);
    let html = renderPanels(store);
    for (const [, spec] of dependent) expect(rowOf(html, spec.key)).toContain('aria-disabled="true"');
    for (const [tool, spec] of dependent)
      store.getState().setSetting(tool.id, spec.enabledWhen!.key, spec.enabledWhen!.equals);
    html = renderPanels(store);
    for (const [, spec] of dependent) expect(rowOf(html, spec.key)).not.toContain('aria-disabled="true"');
    expect(disabledKeys(html)).toEqual([]);
  });
});
