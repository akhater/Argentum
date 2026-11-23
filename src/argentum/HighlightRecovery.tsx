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
 * WHY FLIPPING IT DECODES THE PHOTO AGAIN
 *
 * It runs while the RAW is decoded: right after demosaic, before white balance,
 * for a new edit (mods/highlight_pull.rs), and on the mosaic for an edit saved
 * before that (mods/highlight_rule.rs). Everything else in this app is applied
 * per frame on the GPU; this cannot be. "Applies to the next photo you open"
 * would not hold either, because recently opened photos are served from
 * memory without decoding again.
 *
 * So the switch decodes the open photo again, to one side, and swaps it in
 * when done — see `mods/redecode.rs` for why it is not `load_image`, which is
 * what four attempts at a live camera-profile switch tripped over. Then it
 * asks for one render of the same edits, through the same path a slider uses.
 */

import { useCallback, useEffect, useState } from 'react';
import Switch from '../components/ui/Switch';
import { ag } from './ag';
import { useAgTranslation } from './locales';
import { redecodeOpenPhoto } from './redecode';

export default function HighlightRecovery() {
  const t = useAgTranslation();
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setOn(await ag('highlight_recovery'));
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
      setBusy(false);
      return;
    }
    try {
      // The setting is saved by now; a failure here leaves the switch where it
      // is and the photo as it was, and the next open picks the setting up.
      await redecodeOpenPhoto();
    } catch (e) {
      console.warn('Re-decoding the open photo failed:', e);
    } finally {
      setBusy(false);
    }
  };

  if (on === null) {
    return null;
  }

  return (
    <div>
      {/*
        The panel's shared Switch, not a hand-rolled one: it matches the panel
        and its knob stays visible on this theme's near-white accent. It already
        carries the label and the tooltip, so the explanation lives on hover
        instead of as a paragraph in a panel this narrow.
      */}
      <Switch checked={on} disabled={busy} label={t('recoveryLabel')} onChange={toggle} tooltip={t('recoveryHelp')} />
    </div>
  );
}
