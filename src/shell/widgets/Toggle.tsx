import type { WidgetProps } from '.';

/** A bool setting: its label and a switch. */
export function Toggle({ label, value, disabled, onChange, onDone }: WidgetProps) {
  const on = value === true;
  return (
    <div className={disabled ? 'ags-row ags-toggle is-disabled' : 'ags-row ags-toggle'}>
      <span className="ags-row-label" title={label}>
        {label}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={label}
        disabled={disabled}
        className={on ? 'ags-switch is-on' : 'ags-switch'}
        onClick={() => {
          onChange(!on);
          onDone();
        }}
      >
        <span className="ags-switch-knob" />
      </button>
    </div>
  );
}
