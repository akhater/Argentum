/**
 * The GPS map in the Metadata panel. Ours.
 *
 * The inherited panel loaded an OpenStreetMap frame as soon as a photo with GPS
 * was shown, which told a third party where the photo was taken without the
 * user asking. Now the panel shows a "Show map" button; the frame (and the link
 * that opens the place in the browser) appears after a click, for that photo
 * only. Moving to another photo shows the button again. Nothing is remembered.
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import Button from '../components/ui/Button';

/** The map was asked for on this very photo. */
export const isMapShown = (askedFor: string | null, photoKey: string) => askedFor === photoKey;

interface GpsMapViewProps {
  lat: number;
  lon: number;
  shown: boolean;
  onShow: () => void;
}

export function GpsMapView({ lat, lon, shown, onShow }: GpsMapViewProps) {
  const { t } = useTranslation();

  if (!shown) {
    return (
      <Button className="bg-surface border border-border-color" onClick={onShow}>
        {t('editor.metadata.gps.showMap')}
      </Button>
    );
  }

  return (
    <div className="relative rounded-md overflow-hidden shadow-sm">
      <iframe
        className="pointer-events-none"
        frameBorder="0"
        height="180"
        loading="lazy"
        marginHeight={0}
        marginWidth={0}
        scrolling="no"
        src={`https://www.openstreetmap.org/export/embed.html?bbox=${lon - 0.01}%2C${lat - 0.01}%2C${lon + 0.01}%2C${
          lat + 0.01
        }&layer=mapnik&marker=${lat}%2C${lon}`}
        width="100%"
      ></iframe>
      <a
        className="absolute inset-0 cursor-pointer hover:bg-black/10 transition-colors"
        href={`https://www.openstreetmap.org/?mlat=${lat}&mlon=${lon}#map=15/${lat}/${lon}`}
        rel="noopener noreferrer"
        target="_blank"
        data-tooltip={t('editor.metadata.gps.clickToOpenTooltip')}
      ></a>
    </div>
  );
}

/** `photoKey` is the photo's path: a different path is a different photo, so the map is hidden again. */
export default function GpsMap({ photoKey, lat, lon }: { photoKey: string; lat: number; lon: number }) {
  const [askedFor, setAskedFor] = useState<string | null>(null);
  return <GpsMapView lat={lat} lon={lon} shown={isMapShown(askedFor, photoKey)} onShow={() => setAskedFor(photoKey)} />;
}
