// The tool panels, built from descriptors alone: the engine's sections, a tool
// block per descriptor, a row per setting with the widget of its kind. There is
// no code here for any particular tool.

import { ChevronDown, ChevronRight, RotateCcw } from 'lucide-react';
import { useMemo, useState } from 'react';

import type { SettingSpec, ToolDescriptor } from '../contract';
import { valueOf } from '../edit';
import { text, toolLabel } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import { WIDGETS } from '../widgets';
import { advancedSettings, buildSections, isEnabled, mainSettings, type PanelSection } from './model';

function SettingRow({ tool, spec }: { tool: ToolDescriptor; spec: SettingSpec }) {
  const store = useShellStore();
  const value = useShell((s) => valueOf(s.doc, tool.id, spec));
  const enabled = useShell((s) => isEnabled(s.doc, tool, spec));
  const label = text(spec.label);
  const Widget = WIDGETS[spec.kind];
  return (
    <div data-setting={spec.key} data-kind={spec.kind}>
      <Widget
        spec={spec}
        label={label}
        value={value}
        disabled={!enabled}
        onChange={(v, live) => store.getState().setSetting(tool.id, spec.key, v, { gesture: live })}
        onDone={() => store.getState().endGesture()}
      />
    </div>
  );
}

function ToolPanel({ tool }: { tool: ToolDescriptor }) {
  const store = useShellStore();
  const [more, setMore] = useState(false);
  const changed = useShell((s) => tool.settings.some((spec) => valueOf(s.doc, tool.id, spec) !== spec.default));
  const advanced = advancedSettings(tool);
  const name = toolLabel(tool);
  return (
    <div className="ags-tool" data-tool={tool.id}>
      <div className="ags-tool-head">
        <span className="ags-tool-name">{name}</span>
        <span className="ags-origin" title={text(`origins.${tool.origin}`)}>
          {tool.origin}
        </span>
        <button
          type="button"
          className="ags-icon-button"
          disabled={!changed}
          title={text('shell.resetNamed', { name })}
          onClick={() => store.getState().resetTool(tool.id)}
        >
          <RotateCcw size={12} />
        </button>
      </div>
      {mainSettings(tool).map((spec) => (
        <SettingRow key={spec.key} tool={tool} spec={spec} />
      ))}
      {advanced.length > 0 && (
        <>
          <button type="button" className="ags-more" onClick={() => setMore(!more)}>
            {more ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            {text(more ? 'shell.less' : 'shell.more')}
          </button>
          {more && advanced.map((spec) => <SettingRow key={spec.key} tool={tool} spec={spec} />)}
        </>
      )}
    </div>
  );
}

function Section({ section }: { section: PanelSection }) {
  const store = useShellStore();
  const collapsed = useShell((s) => s.collapsed[section.id] === true);
  const title = text(section.label);
  return (
    <section className="ags-section" data-section={section.id}>
      <div className="ags-section-head">
        <button type="button" className="ags-section-toggle" onClick={() => store.getState().toggleSection(section.id)}>
          <span className="ags-section-title">{title}</span>
        </button>
        <button
          type="button"
          className="ags-icon-button"
          title={text('shell.resetNamed', { name: title })}
          onClick={() => store.getState().resetSection(section.id)}
        >
          <RotateCcw size={12} />
        </button>
        <button type="button" className="ags-icon-button" onClick={() => store.getState().toggleSection(section.id)}>
          {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </button>
      </div>
      {!collapsed && (
        <div className="ags-section-body">
          {section.tools.map((tool) => (
            <ToolPanel key={tool.id} tool={tool} />
          ))}
        </div>
      )}
    </section>
  );
}

export function PanelStack() {
  const listed = useShell((s) => s.sections);
  const tools = useShell((s) => s.tools);
  const sections = useMemo(() => buildSections(listed, tools), [listed, tools]);
  return (
    <div className="ags-stack">
      {sections.map((section) => (
        <Section key={section.id} section={section} />
      ))}
    </div>
  );
}
