/**
 * The highlight recovery switch. Ours.
 *
 * WHY IT IS A SWITCH AND NOT A SLIDER
 *
 * Nowhere gives this an amount. Lightroom has no control at all — it happens as
 * part of reading the file. darktable and RawTherapee let you pick a *method*
 * and turn it off, because there is nothing continuous to dial: a channel is
 * either being rebuilt from the ones that survived, or it is being left where
 * the sensor gave up.
 *
 * WHY IT SAYS "NEXT PHOTO"
 *
 * It runs while the RAW is decoded, before demosaic, which is the only place
 * the mosaic still exists. Everything else in this app is applied per frame on
 * the GPU and changes as you drag. This cannot be, so it does not pretend: the
 * label says when it applies, and switching photos is what applies it.
 *
 * Camera profiles were built to pretend, four times, and each attempt broke
 * something else. Saying what it does is cheaper and truer than hiding it.
 */

import { useCallback, useEffect, useState } from 'react';
import Switch from '../components/ui/Switch';
import { ag } from './ag';
import { useAgTranslation } from './locales';

export default function HighlightRecovery() {
  const t = useAgTranslation();
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setOn(await ag<boolean>('highlight_recovery'));
    } catch {
      setOn(null);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const toggle = async (next: boolean) => {
    if (on === null || busy) {
      return;
    }
    setBusy(true);
    // Shown immediately, because the write is to a one-line file and a switch
    // that lags a click feels broken.
    setOn(next);
    try {
      await ag('set_highlight_recovery', { on: next });
    } catch {
      setOn(!next);
    } finally {
      setBusy(false);
    }
  };

  if (on === null) {
    return null;
  }

  return (
    <div className="p-2 bg-bg-tertiary rounded-md">
      {/*
        Their Switch, not one of ours. The first version was hand-rolled, and
        besides looking like a stranger in the panel its knob was white on an
        accent that is near-white in this theme — so turning it on made the
        knob vanish. Theirs already carries the label and the tooltip, so the
        explanation lives on hover instead of as a paragraph in a panel this
        narrow.
      */}
      <Switch
        checked={on}
        disabled={busy}
        label={t('recoveryLabel')}
        onChange={toggle}
        tooltip={t('recoveryHelp')}
      />
    </div>
  );
}
