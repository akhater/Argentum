// How descriptors become panels: grouped by section, in pipeline order. This
// is the whole of it, and it knows no tool by name.

import type { SettingSpec, SettingValue, ToolDescriptor } from '../contract';
import { text } from '../i18n';

export interface PanelSection {
  id: string;
  tools: ToolDescriptor[];
}

/** Sections in the order of their first tool; tools by `order` within each. */
export function buildSections(tools: ToolDescriptor[]): PanelSection[] {
  const sorted = [...tools].sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  const sections = new Map<string, ToolDescriptor[]>();
  for (const tool of sorted) {
    if (!sections.has(tool.section)) sections.set(tool.section, []);
    sections.get(tool.section)!.push(tool);
  }
  return [...sections].map(([id, list]) => ({ id, tools: list }));
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
