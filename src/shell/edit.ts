// The edit document and its patches, as plain functions. Nothing here knows a
// tool: every rule comes from the descriptors.

import { EDIT_VERSION, type EditDocument, type EditPatch, type SettingSpec, type SettingValue } from './contract';
import type { ToolDescriptor } from './contract';

export const emptyDocument = (): EditDocument => ({ version: EDIT_VERSION, tools: {} });

const specOf = (tools: ToolDescriptor[], toolId: string, key: string) =>
  tools.find((t) => t.id === toolId)?.settings.find((s) => s.key === key);

/** The value shown for a setting: the edit's, or the default when the edit has none. */
export const valueOf = (doc: EditDocument, toolId: string, spec: SettingSpec): SettingValue =>
  doc.tools[toolId]?.settings[spec.key] ?? spec.default;

/** `doc` with `patch` applied, `null` meaning the default. The document is never mutated. */
export function applyPatch(doc: EditDocument, patch: EditPatch, tools: ToolDescriptor[]): EditDocument {
  const next: EditDocument = { version: doc.version, tools: { ...doc.tools } };
  for (const [toolId, values] of Object.entries(patch)) {
    const before = next.tools[toolId];
    const settings = { ...before?.settings };
    for (const [key, value] of Object.entries(values)) {
      const v = value ?? specOf(tools, toolId, key)?.default;
      if (v === undefined) delete settings[key];
      else settings[key] = v;
    }
    const v = before?.v ?? tools.find((t) => t.id === toolId)?.version ?? 1;
    next.tools[toolId] = { v, settings };
  }
  return next;
}

/** The patch that undoes `patch` on `doc`: every value it touches, as it was. */
export function inverseOf(doc: EditDocument, patch: EditPatch, tools: ToolDescriptor[]): EditPatch {
  const inverse: EditPatch = {};
  for (const [toolId, values] of Object.entries(patch)) {
    inverse[toolId] = {};
    for (const key of Object.keys(values)) {
      inverse[toolId][key] = doc.tools[toolId]?.settings[key] ?? specOf(tools, toolId, key)?.default ?? null;
    }
  }
  return inverse;
}

/** `later` applied after `earlier`, as one patch. */
export function composePatches(earlier: EditPatch, later: EditPatch): EditPatch {
  const out: EditPatch = {};
  for (const p of [earlier, later]) {
    for (const [toolId, values] of Object.entries(p)) out[toolId] = { ...out[toolId], ...values };
  }
  return out;
}

/** The patch that takes `from` to `to`, over the settings the descriptors name. */
export function diffDocuments(from: EditDocument, to: EditDocument, tools: ToolDescriptor[]): EditPatch {
  const patch: EditPatch = {};
  for (const tool of tools) {
    for (const spec of tool.settings) {
      const b = valueOf(to, tool.id, spec);
      if (valueOf(from, tool.id, spec) !== b) (patch[tool.id] ??= {})[spec.key] = b;
    }
  }
  return patch;
}

/** Every value that differs from its default, as a patch: what a preset keeps. */
export const changesFromDefaults = (doc: EditDocument, tools: ToolDescriptor[]): EditPatch =>
  diffDocuments(emptyDocument(), doc, tools);

/** Every setting of `tools` that is not at its default, put back to it. */
export function resetPatch(doc: EditDocument, tools: ToolDescriptor[]): EditPatch {
  const patch: EditPatch = {};
  for (const tool of tools) {
    for (const spec of tool.settings) {
      if (valueOf(doc, tool.id, spec) !== spec.default) (patch[tool.id] ??= {})[spec.key] = null;
    }
  }
  return patch;
}

export const isEmptyPatch = (patch: EditPatch): boolean =>
  Object.values(patch).every((values) => Object.keys(values).length === 0);
