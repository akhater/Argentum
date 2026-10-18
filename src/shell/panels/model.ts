// How descriptors become panels: sections in the order the engine gives them,
// tools by their order within each. This is the whole of it, and it knows no
// tool and no section by name.

import type { EditDocument, SectionDescriptor, SettingSpec, SettingValue, ToolDescriptor } from '../contract';
import { text } from '../i18n';

export interface PanelSection {
  id: string;
  /** i18n key. */
  label: string;
  tools: ToolDescriptor[];
}

/**
 * The sections that have tools, in the engine's order, each with its tools by
 * `order`. A tool naming a section the engine did not list still shows, after
 * the listed ones, so no tool can go missing.
 */
export function buildSections(sections: SectionDescriptor[], tools: ToolDescriptor[]): PanelSection[] {
  const sorted = [...tools].sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  const listed: PanelSection[] = sections.map((s) => ({ id: s.id, label: s.label, tools: [] }));
  const out = [...listed];
  for (const tool of sorted) {
    let section = out.find((s) => s.id === tool.section);
    if (!section) out.push((section = { id: tool.section, label: tool.section, tools: [] }));
    section.tools.push(tool);
  }
  return out.filter((s) => s.tools.length > 0);
}

/** Whether a setting means anything now: its `enabledWhen` holds, or it has none. */
export function isEnabled(doc: EditDocument, tool: ToolDescriptor, spec: SettingSpec): boolean {
  const when = spec.enabledWhen;
  if (!when) return true;
  const value = doc.tools[tool.id]?.settings[when.key] ?? tool.settings.find((s) => s.key === when.key)?.default;
  return value === when.equals;
}

const isLog = (spec: SettingSpec, min: number, max: number) => spec.scale === 'log' && min > 0 && max > min;

/** Where a value sits along its slider, 0..1: evenly, or by ratio for a log setting. */
export function railPosition(spec: SettingSpec, value: number): number {
  const min = spec.min ?? 0;
  const max = spec.max ?? 1;
  if (max <= min) return 0;
  const v = Math.min(max, Math.max(min, value));
  if (isLog(spec, min, max)) return Math.log(v / min) / Math.log(max / min);
  return (v - min) / (max - min);
}

/** The value at a point along its slider, 0..1: the inverse of railPosition. */
export function railValue(spec: SettingSpec, position: number): number {
  const min = spec.min ?? 0;
  const max = spec.max ?? 1;
  const p = Math.min(1, Math.max(0, position));
  if (isLog(spec, min, max)) return min * Math.pow(max / min, p);
  return min + p * (max - min);
}

export const mainSettings = (tool: ToolDescriptor) => tool.settings.filter((s) => !s.advanced);
export const advancedSettings = (tool: ToolDescriptor) => tool.settings.filter((s) => s.advanced);

/** Decimals a number is shown with: from its step, or two. */
export function decimalsOf(spec: SettingSpec): number {
  if (spec.kind === 'int') return 0;
  if (spec.step === undefined) return 2;
  const s = String(spec.step);
  return s.includes('.') ? s.split('.')[1].length : 0;
}

/** A value as the panel and the history show it. */
export function formatValue(spec: SettingSpec, value: SettingValue): string {
  if (typeof value === 'boolean') return text(value ? 'shell.on' : 'shell.off');
  if (typeof value === 'string') {
    const choice = spec.choices?.find((c) => c.value === value);
    return choice ? text(choice.label) : value;
  }
  const n = value.toFixed(decimalsOf(spec));
  const signed = spec.min !== undefined && spec.min < 0 && value > 0 ? `+${n}` : n;
  return spec.unit ? `${signed} ${spec.unit}` : signed;
}
