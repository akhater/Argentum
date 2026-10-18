// A Lightroom-style slider row: label, rail, number. The layout and behaviour
// follow RAWmakase's `slider_styled` (src/app/widgets.rs, MIT): an 83 px label
// column, a 52 px number, double-click to reset, and a fill that starts at zero
// for a setting that starts at zero and at its default otherwise, with a tick.
// A `log` setting moves by ratio, so a multiplier's 1 sits mid-rail.

import { useRef, useState, type KeyboardEvent, type PointerEvent } from 'react';

import { decimalsOf, formatValue, railPosition, railValue } from '../panels/model';
import type { WidgetProps } from '.';

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function Slider({ spec, label, value, disabled, onChange, onDone }: WidgetProps) {
  const rail = useRef<HTMLDivElement>(null);
  const [typing, setTyping] = useState<string | null>(null);
  const min = spec.min ?? 0;
  const max = spec.max ?? 1;
  const n = typeof value === 'number' ? value : Number(value);
  const fallback = typeof spec.default === 'number' ? spec.default : min;
  const step = spec.step ?? (spec.kind === 'int' ? 1 : (max - min) / 100);

  const snap = (v: number) => {
    const stepped = Math.round((v - min) / step) * step + min;
    const out = spec.kind === 'int' ? Math.round(stepped) : Number(stepped.toFixed(decimalsOf(spec) + 2));
    return clamp(out, min, max);
  };
  const at = (clientX: number) => {
    const r = rail.current!.getBoundingClientRect();
    return snap(railValue(spec, (clientX - r.left) / Math.max(1, r.width)));
  };
  const share = (v: number) => railPosition(spec, v);

  // RAWmakase's fill_origin: from zero if the range starts there, else from the default.
  const origin = min === 0 ? 0 : fallback;
  const [a, b] = [share(origin), share(n)].sort((x, y) => x - y);

  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || disabled) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    onChange(at(e.clientX), true);
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    if (e.currentTarget.hasPointerCapture(e.pointerId)) onChange(at(e.clientX), true);
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
    e.currentTarget.releasePointerCapture(e.pointerId);
    onDone();
  };
  const reset = () => {
    if (disabled) return;
    onChange(fallback);
    onDone();
  };
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    const delta =
      e.key === 'ArrowRight' || e.key === 'ArrowUp' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowDown' ? -1 : 0;
    if (!delta || disabled) return;
    e.preventDefault();
    const times = delta * (e.shiftKey ? 10 : 1);
    // Along the rail for a log setting, where a fixed step means less at the top than the bottom.
    onChange(snap(spec.scale === 'log' ? railValue(spec, share(n) + times / 100) : n + times * step));
    onDone();
  };
  const typed = () => {
    if (typing === null) return;
    const parsed = Number.parseFloat(typing.replace(',', '.'));
    setTyping(null);
    if (Number.isFinite(parsed)) {
      onChange(snap(parsed));
      onDone();
    }
  };

  return (
    <div className={disabled ? 'ags-row ags-slider is-disabled' : 'ags-row ags-slider'}>
      <label className="ags-row-label" title={label} onDoubleClick={reset}>
        {label}
      </label>
      <div
        ref={rail}
        className="ags-rail"
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-disabled={disabled}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={n}
        aria-valuetext={formatValue(spec, n)}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onDoubleClick={reset}
        onKeyDown={key}
      >
        <div className="ags-rail-track" />
        <div className="ags-rail-fill" style={{ left: `${a * 100}%`, width: `${(b - a) * 100}%` }} />
        {origin !== min && <div className="ags-rail-tick" style={{ left: `${share(origin) * 100}%` }} />}
        <div className="ags-rail-thumb" style={{ left: `${share(n) * 100}%` }} />
      </div>
      <input
        className="ags-number"
        aria-label={label}
        disabled={disabled}
        value={typing ?? n.toFixed(decimalsOf(spec))}
        onChange={(e) => setTyping(e.target.value)}
        onFocus={(e) => e.target.select()}
        onBlur={typed}
        onKeyDown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
          if (e.key === 'Escape') {
            setTyping(null);
            e.currentTarget.blur();
          }
        }}
      />
      <span className="ags-unit">{spec.unit ?? ''}</span>
    </div>
  );
}
