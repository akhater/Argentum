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
 * It runs while the RAW is decoded, before demosaic, which is the only place
 * the mosaic still exists. Everything else in this app is applied per frame on
 * the GPU; this cannot be. It used to say "applies to the next photo you open"
 * and did not even manage that, because recently opened photos are served
 * from memory without decoding again.
 *
 * So the switch decodes the open photo again, to one side, and swaps it in
 * when done — see `mods/redecode.rs` for why it is not `load_image`, which is
 * what four attempts at a live camera-profile switch tripped over. Then it
 * asks for one render of the same edits, through the same path a slider uses.
 */

import { useCallback, useEffect, useState } from 'react';
import Switch from '../components/ui/Switch';
import { useEditorStore } from '../store/useEditorStore';
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
      setBusy(false);
      return;
    }
    try {
      // The setting is saved by now; a failure here leaves the switch where it
      // is and the photo as it was, and the next open picks the setting up.
      if (await ag<boolean>('redecode_open_photo')) {
        // A new adjustments object with the same values: the render effect
        // fires on identity, exactly as after a slider, and nothing changes.
        useEditorStore.getState().setEditor((s) => ({ adjustments: { ...s.adjustments } }));
      }
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
