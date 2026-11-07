/**
 * The resolution the editor renders at once the zoom settles. Ours.
 *
 * Their editor only ever raised it (src/hooks/useImageProcessing.ts, from
 * RapidRAW). After a zoom to 100% and back, fit-to-window kept showing the
 * full-size frame, sampled down by the display one pixel in three or four,
 * with sharpening sized for 100%: on an ISO 8000 RAW about three times the
 * noise of a fit render, until another photo was opened. So it comes back
 * down as well, to what a fresh open of the photo renders at that zoom.
 *
 * `shown` is the long side of the frame on screen (`currentResRef`): 0 before
 * the first render of a photo, Infinity while it loads. `wanted` is what the
 * zoom asks for (calculateTargetRes). Returns the long side to render, or null
 * to keep the frame shown.
 */
export const zoomRenderResolution = (wanted: number, shown: number): number | null =>
  wanted !== shown && Number.isFinite(shown) ? wanted : null;
