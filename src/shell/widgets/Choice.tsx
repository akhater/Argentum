import { text } from '../i18n';
import type { WidgetProps } from '../panels/registry';

/** A choice setting: its label and a menu of the choices. */
export function Choice({ spec, label, value, onChange, onDone }: WidgetProps) {
  return (
    <div className="ags-row ags-choice">
      <span className="ags-row-label" title={label}>
        {label}
      </span>
      <select
        className="ags-select"
        aria-label={label}
        value={String(value)}
        onChange={(e) => {
          onChange(e.target.value);
          onDone();
        }}
      >
        {spec.choices?.map((c) => (
          <option key={c.value} value={c.value}>
            {text(c.label)}
          </option>
        ))}
      </select>
    </div>
  );
}
