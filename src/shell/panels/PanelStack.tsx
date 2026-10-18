// The tool panels, built from descriptors alone: a section per descriptor
// `section`, a tool block per descriptor, a row per setting with the widget its
// kind is registered to. There is no code here for any particular tool.

import { ChevronDown, ChevronRight, RotateCcw } from 'lucide-react';
import { useMemo, useState } from 'react';

import type { SettingSpec, ToolDescriptor } from '../contract';
import { valueOf } from '../edit';
import { text } from '../i18n';
import { useShell, useShellStore } from '../ShellContext';
import '../widgets';
import { advancedSettings, buildSections, mainSettings, type PanelSection } from './model';
import { widgetFor } from './registry';

function SettingRow({ tool, spec }: { tool: ToolDescriptor; spec: SettingSpec }) {
  const store = useShellStore();
  const value = useShell((s) => valueOf(s.doc, tool.id, spec));
  const label = text(spec.label);
  const Widget = widgetFor(spec.kind);
  if (!Widget) {
    return (
      <div className="ags-row ags-unsupported" data-setting={spec.key}>
        <span className="ags-row-label">{label}</span>
        <span className="ags-dim">{text('shell.unsupported', { kind: spec.kind })}</span>
      </div>
    );
  }
  return (
    <div data-setting={spec.key} data-kind={spec.kind}>
      <Widget
        spec={spec}
        label={label}
        value={value}
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
  const name = text(tool.label);
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
  const title = text(`sections.${section.id}`);
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
  const tools = useShell((s) => s.tools);
  const sections = useMemo(() => buildSections(tools), [tools]);
  return (
    <div className="ags-stack">
      {sections.map((section) => (
        <Section key={section.id} section={section} />
      ))}
    </div>
  );
}
