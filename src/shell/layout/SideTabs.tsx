// The panel beside the photo: Presets, Snapshots or History, one at a time, as
// RAWmakase's Develop tab panel (src/app/side_tabs.rs, MIT). Presets and
// snapshots are applied as patches, so each is one step in History.

import { Plus } from 'lucide-react';

import type { SectionDescriptor, ToolDescriptor } from '../contract';
import { text } from '../i18n';
import { formatValue } from '../panels/model';
import { useShell, useShellStore } from '../ShellContext';
import type { HistoryLabel } from '../store';

function describe(label: HistoryLabel, tools: ToolDescriptor[], sections: SectionDescriptor[]): string {
  switch (label.kind) {
    case 'set': {
      const tool = tools.find((t) => t.id === label.toolId);
      const spec = tool?.settings.find((s) => s.key === label.key);
      return text('shell.history.set', {
        tool: tool ? text(tool.label) : label.toolId,
        setting: spec ? text(spec.label) : label.key,
        value: spec ? formatValue(spec, label.value) : String(label.value),
      });
    }
    case 'resetTool': {
      const tool = tools.find((t) => t.id === label.toolId);
      return text('shell.history.resetTool', { tool: tool ? text(tool.label) : label.toolId });
    }
    case 'resetSection':
      return text('shell.history.resetSection', {
        section: text(sections.find((s) => s.id === label.section)?.label ?? label.section),
      });
    case 'resetAll':
      return text('shell.resetAll');
    case 'preset':
      return text('shell.history.preset', { name: label.name });
    case 'snapshot':
      return text('shell.history.snapshot', { name: label.name });
  }
}

function HistoryTab() {
  const store = useShellStore();
  const past = useShell((s) => s.past);
  const future = useShell((s) => s.future);
  const tools = useShell((s) => s.tools);
  const sections = useShell((s) => s.sections);
  // Oldest first, then what was undone, in the order it would be redone; shown newest at the top.
  const steps = [...past, ...[...future].reverse()];
  return (
    <ol className="ags-list" reversed>
      {steps
        .map((entry, i) => (
          <li key={entry.id}>
            <button
              type="button"
              className={i === past.length - 1 ? 'is-current' : i >= past.length ? 'is-undone' : ''}
              onClick={() => store.getState().travel(i + 1)}
            >
              {describe(entry.label, tools, sections)}
            </button>
          </li>
        ))
        .reverse()}
      <li>
        <button
          type="button"
          className={past.length === 0 ? 'is-current' : ''}
          onClick={() => store.getState().travel(0)}
        >
          {text('shell.history.open')}
        </button>
      </li>
    </ol>
  );
}

function SnapshotsTab() {
  const store = useShellStore();
  const snapshots = useShell((s) => s.snapshots);
  const hasPhoto = useShell((s) => s.photo !== null);
  return (
    <>
      <button
        type="button"
        className="ags-tab-action"
        disabled={!hasPhoto}
        onClick={() => store.getState().addSnapshot(text('shell.snapshots.default', { n: snapshots.length + 1 }))}
      >
        <Plus size={13} />
        {text('shell.snapshots.add')}
      </button>
      {snapshots.length === 0 ? (
        <p className="ags-dim ags-tab-note">{text('shell.snapshots.empty')}</p>
      ) : (
        <ul className="ags-list">
          {snapshots.map((snap, i) => (
            <li key={i}>
              <button type="button" onClick={() => store.getState().applySnapshot(i)}>
                {snap.name}
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function PresetsTab() {
  const store = useShellStore();
  const presets = useShell((s) => s.presets);
  const hasPhoto = useShell((s) => s.photo !== null);
  return (
    <>
      <button
        type="button"
        className="ags-tab-action"
        disabled={!hasPhoto}
        onClick={() => store.getState().savePreset(text('shell.presets.default', { n: presets.length + 1 }))}
      >
        <Plus size={13} />
        {text('shell.presets.save')}
      </button>
      {presets.length === 0 ? (
        <p className="ags-dim ags-tab-note">{text('shell.presets.empty')}</p>
      ) : (
        <ul className="ags-list">
          {presets.map((preset, i) => (
            <li key={i}>
              <button type="button" onClick={() => store.getState().applyPreset(i)}>
                {preset.name}
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

export function SideTabs() {
  const tab = useShell((s) => s.sideTab);
  if (!tab) return null;
  return (
    <aside className="ags-side">
      <div className="ags-block-head">
        <span>{text(`shell.rail.${tab}`)}</span>
      </div>
      <div className="ags-side-body">
        {tab === 'history' && <HistoryTab />}
        {tab === 'snapshots' && <SnapshotsTab />}
        {tab === 'presets' && <PresetsTab />}
      </div>
    </aside>
  );
}
