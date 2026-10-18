// Panels come from descriptors: sections, tools, rows and widgets.

import { describe, expect, it } from 'vitest';

import { FAKE_TOOLS } from '../engine/fake';
import { buildSections } from '../panels/model';
import { openShell, renderPanels } from './helpers';

const count = (html: string, needle: string) => html.split(needle).length - 1;

describe('panels from descriptors', () => {
  it('group tools by section, in pipeline order', () => {
    const sections = buildSections(FAKE_TOOLS);
    expect(sections.map((s) => [s.id, s.tools.map((t) => t.id)])).toEqual([
      ['basic', ['dt_highlights', 'ag_exposure_wb']],
      ['detail', ['rt_capture_sharpen']],
    ]);
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
