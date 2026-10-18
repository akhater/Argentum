// The widget for each setting kind, one entry per kind the contract has. A
// tool never names a widget and the panel never names a tool: the setting's
// kind picks the widget. A kind added to the contract without a widget here
// is a compile error.

import type { ComponentType } from 'react';

import type { SettingKind, SettingSpec, SettingValue } from '../contract';
import { Choice } from './Choice';
import { Slider } from './Slider';
import { Toggle } from './Toggle';

export interface WidgetProps {
  spec: SettingSpec;
  /** Already translated. */
  label: string;
  value: SettingValue;
  /** The setting's `enabledWhen` does not hold: shown, but not editable. */
  disabled?: boolean;
  /** A new value. `live` while a drag is under way: its ticks are one undo step. */
  onChange(value: SettingValue, live?: boolean): void;
  /** The drag that sent `live` changes is over. */
  onDone(): void;
}

export const WIDGETS: Readonly<Record<SettingKind, ComponentType<WidgetProps>>> = {
  float: Slider,
  int: Slider,
  bool: Toggle,
  choice: Choice,
};
