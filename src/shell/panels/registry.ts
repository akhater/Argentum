// Widgets by setting kind. A tool never names a widget and the panel never
// names a tool: the setting's kind picks the widget. A kind with no widget yet
// (a curve, a colour) is shown as such rather than hiding its tool.

import type { ComponentType } from 'react';

import type { SettingSpec, SettingValue } from '../contract';

export interface WidgetProps {
  spec: SettingSpec;
  /** Already translated. */
  label: string;
  value: SettingValue;
  /** A new value. `live` while a drag is under way: its ticks are one undo step. */
  onChange(value: SettingValue, live?: boolean): void;
  /** The drag that sent `live` changes is over. */
  onDone(): void;
}

export type Widget = ComponentType<WidgetProps>;

const widgets = new Map<string, Widget>();

export function registerWidget(kind: string, widget: Widget) {
  widgets.set(kind, widget);
}

export const widgetFor = (kind: string): Widget | undefined => widgets.get(kind);
