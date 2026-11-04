/**
 * The Grain row of the Provider looks section: grain over the look on the photo,
 * when its provider offers it. Ours.
 *
 * Its own switch, off until turned on, and an Amount slider while it is on. It is
 * drawn over the LUT, so it never makes or remakes a look's file
 * (src-tauri/src/mods/look_grain.rs). The Effects panel's Grain is another tool.
 */

import clsx from 'clsx';
import Slider from '../../components/ui/Slider';
import type { GrainAmount, LookGrain } from '../ag.generated';
import { useAgTranslation } from '../locales';

/** Past this many steps the slider shows no tick under each one. */
const MAX_TICKS = 8;

interface Props {
  label: string;
  amount: GrainAmount;
  grain: LookGrain;
  /** With the section off, or no look on the photo. */
  disabled: boolean;
  onChange: (grain: LookGrain) => void;
}

export default function GrainRow({ label, amount, grain, disabled, onChange }: Props) {
  const t = useAgTranslation();
  const steps = Math.round((amount.max - amount.min) / amount.step);
  const ticks = steps <= MAX_TICKS ? Array.from({ length: steps + 1 }, (_, i) => amount.min + i * amount.step) : [];
  return (
    <div className={clsx('agm-provider-grain', disabled && 'agm-look-dim')}>
      <div className="agm-provider-grain-top">
        <span className="min-w-0 truncate">{label}</span>
        <button
          aria-checked={grain.on}
          aria-label={label}
          className="agm-switch"
          disabled={disabled}
          onClick={() => onChange({ ...grain, on: !grain.on })}
          role="switch"
        >
          <span>{t(grain.on ? 'looksProviderOn' : 'looksProviderOff')}</span>
          <i />
        </button>
      </div>
      {grain.on && (
        <div className="agm-provider-grain-amount">
          <Slider
            defaultValue={amount.default}
            disabled={disabled}
            label={t('looksProviderGrainAmount')}
            max={amount.max}
            min={amount.min}
            onChange={(e) => onChange({ ...grain, amount: Number(e.target.value) })}
            step={amount.step}
            value={grain.amount}
          />
          {ticks.length > 0 && (
            <div className="agm-provider-grain-ticks">
              <div>
                {ticks.map((tick) => (
                  <span key={tick}>{tick}</span>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
