// Settings > Reset Layout and what start-up reads back: the two column widths are all the window keeps.

import { describe, expect, it } from 'vitest';
import { Panel } from '../components/ui/AppProperties';
import { DEFAULT_LEFT_PANEL_WIDTH_PX, DEFAULT_RIGHT_PANEL_WIDTH_PX, readWorkspace, useUIStore } from './useUIStore';

// A panel's key or menu item opens the column the Modern window shows it in (argentum/modern/panels.ts).
describe('setPanel', () => {
  const hidden = { filmstrip: true, leftPanel: false, rightPanel: false };

  it('opens the right column on a right panel tab, Export and Info included', () => {
    for (const panel of [Panel.Export, Panel.Metadata, Panel.Crop]) {
      useUIStore.setState({ uiVisibility: hidden, activePanel: Panel.Adjustments });
      useUIStore.getState().setPanel(panel);
      const { uiVisibility, activePanel } = useUIStore.getState();
      expect([panel, uiVisibility.rightPanel, uiVisibility.leftPanel]).toEqual([panel, true, false]);
      expect(activePanel).toBe(panel);
    }
  });

  it('opens the left column for the folders, leaving the right panel tab as it was', () => {
    useUIStore.setState({ uiVisibility: hidden, activePanel: Panel.Crop });
    useUIStore.getState().setPanel(Panel.FolderTree);
    const { uiVisibility, activePanel } = useUIStore.getState();
    expect([uiVisibility.leftPanel, uiVisibility.rightPanel, activePanel]).toEqual([true, false, Panel.Crop]);
  });
});

describe('Reset Layout', () => {
  it('gives back only the two default widths, the designed 268 and 330', () => {
    useUIStore.setState({ leftPanelWidth: 500, rightPanelWidth: 600 });
    const defaults = { leftPanelWidth: 268, rightPanelWidth: 330 };
    expect(useUIStore.getState().resetWorkspaceLayout()).toEqual(defaults);
    expect(useUIStore.getState().leftPanelWidth).toBe(DEFAULT_LEFT_PANEL_WIDTH_PX);
    expect(useUIStore.getState().rightPanelWidth).toBe(DEFAULT_RIGHT_PANEL_WIDTH_PX);
    expect(useUIStore.getState().activePanel).toBe(Panel.Adjustments);
  });

  it('leaves the Camera section as the machine allows', () => {
    useUIStore.setState({ isTetheringSupported: true });
    useUIStore.getState().resetWorkspaceLayout();
    expect(useUIStore.getState().isTetheringSupported).toBe(true);
  });
});

describe('readWorkspace', () => {
  it('keeps saved widths from a file written with only the widths', () => {
    expect(readWorkspace({ leftPanelWidth: 280, rightPanelWidth: 360 })).toEqual({
      leftPanelWidth: 280,
      rightPanelWidth: 360,
    });
  });

  it('drops what the Classic layout saved', () => {
    const saved = { leftPanelWidth: 280, rightPanelWidth: 360, leftTopHeight: 400, panelLayout: { leftTop: [] } };
    expect(readWorkspace(saved)).toEqual({ leftPanelWidth: 280, rightPanelWidth: 360 });
  });

  it('uses the default widths for a missing workspace', () => {
    expect(readWorkspace(undefined)).toEqual({ leftPanelWidth: 268, rightPanelWidth: 330 });
  });

  it('starts a saved legacy default (350) at each column default', () => {
    expect(readWorkspace({ leftPanelWidth: 350, rightPanelWidth: 350 })).toEqual({
      leftPanelWidth: 268,
      rightPanelWidth: 330,
    });
  });

  it('keeps 320 and any other saved width as they are', () => {
    expect(readWorkspace({ leftPanelWidth: 320, rightPanelWidth: 320 })).toEqual({
      leftPanelWidth: 320,
      rightPanelWidth: 320,
    });
    expect(readWorkspace({ leftPanelWidth: 300, rightPanelWidth: 360 })).toEqual({
      leftPanelWidth: 300,
      rightPanelWidth: 360,
    });
  });
});

describe('reopening a hidden column', () => {
  it('opens a column narrower than its minimum at its own default', () => {
    useUIStore.setState({
      uiVisibility: { filmstrip: true, leftPanel: false, rightPanel: false },
      leftPanelWidth: 200,
    });
    useUIStore.getState().setPanel(Panel.FolderTree);
    expect(useUIStore.getState().leftPanelWidth).toBe(268);
  });
});

// Showing a hidden column keeps the width the user chose; only one below the column's minimum starts again at its default.
describe('showing a hidden column', () => {
  const hiddenBoth = { filmstrip: true, leftPanel: false, rightPanel: false };
  const shown = () => useUIStore.getState();

  it('keeps a left width of 300 and a right width of 340 through hide and show', () => {
    useUIStore.setState({
      uiVisibility: { ...hiddenBoth, leftPanel: true, rightPanel: true },
      leftPanelWidth: 300,
      rightPanelWidth: 340,
    });
    for (const column of ['left', 'right'] as const) {
      shown().toggleColumn(column);
      expect(shown().uiVisibility[column === 'left' ? 'leftPanel' : 'rightPanel']).toBe(false);
      expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([300, 340]);
      shown().toggleColumn(column);
    }
    expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([300, 340]);
  });

  it('starts a column narrower than its minimum, or invalid, at its default', () => {
    for (const [left, right] of [
      [223, 299],
      [0, NaN],
      [-5, 120],
    ]) {
      useUIStore.setState({ uiVisibility: hiddenBoth, leftPanelWidth: left, rightPanelWidth: right });
      shown().toggleColumn('left');
      shown().toggleColumn('right');
      expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([268, 330]);
    }
  });

  it('keeps the limits themselves (224 and 300)', () => {
    useUIStore.setState({ uiVisibility: hiddenBoth, leftPanelWidth: 224, rightPanelWidth: 300 });
    shown().toggleColumn('left');
    shown().toggleColumn('right');
    expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([224, 300]);
  });

  it('gives the same width when a panel button shows the column as when the key does', () => {
    useUIStore.setState({ uiVisibility: hiddenBoth, leftPanelWidth: 300, rightPanelWidth: 340 });
    shown().setPanel(Panel.FolderTree);
    shown().setPanel(Panel.Adjustments);
    expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([300, 340]);
    useUIStore.setState({ uiVisibility: hiddenBoth, leftPanelWidth: 100, rightPanelWidth: 100 });
    shown().setPanel(Panel.FolderTree);
    shown().setPanel(Panel.Adjustments);
    expect([shown().leftPanelWidth, shown().rightPanelWidth]).toEqual([268, 330]);
  });
});
