// The GPS map loads only when asked (argentum/GpsMap.tsx): until then nothing is requested from a map host.
import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { ReactElement } from 'react';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const { GpsMapView, isMapShown } = await import('./GpsMap');

const spot = { lat: 48.8584, lon: 2.2945 };
const render = (shown: boolean, onShow = () => {}) =>
  renderToStaticMarkup(<GpsMapView lat={spot.lat} lon={spot.lon} shown={shown} onShow={onShow} />);

describe('the GPS map', () => {
  it('requests nothing from a map host before the click', () => {
    const html = render(false);
    expect(html).not.toMatch(/openstreetmap|<iframe|https?:\/\//);
    expect(html).toContain('editor.metadata.gps.showMap');
  });

  it('shows the map after the click', () => {
    const html = render(true);
    expect(html).toContain('<iframe');
    expect(html).toContain('openstreetmap.org/export/embed.html');
    expect(html).toContain('marker=48.8584%2C2.2945');
    expect(html).toContain('href="https://www.openstreetmap.org/?mlat=48.8584&amp;mlon=2.2945');
  });

  it('asks to show the map when the button is clicked', () => {
    const onShow = vi.fn();
    const tree = GpsMapView({ lat: spot.lat, lon: spot.lon, shown: false, onShow }) as ReactElement<{
      onClick: () => void;
    }>;
    tree.props.onClick();
    expect(onShow).toHaveBeenCalledTimes(1);
  });

  it('is shown for the photo it was asked on, and not for the next one', () => {
    expect(isMapShown('/photos/a.CR3', '/photos/a.CR3')).toBe(true);
    expect(isMapShown('/photos/a.CR3', '/photos/b.CR3')).toBe(false);
    expect(isMapShown(null, '/photos/a.CR3')).toBe(false);
  });
});
