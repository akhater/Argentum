/**
 * Where the photo is on screen. Ours.
 *
 * Shared by the RGB readout and the object brush, which both need to turn a
 * cursor position into a place on the photo and cannot ask `ImageCanvas` for
 * it without a hook in their file.
 */

/**
 * The overlay `<svg>` the editor lays over the photo.
 *
 * This used to hunt for the largest `<img>` in the page. That was never sound:
 * with the GPU renderer on there is no `<img>` for the photo at all — it is a
 * native surface composited behind the webview — so what it actually latched
 * onto was the cached `_medium.jpg` thumbnail, which is in the tree only
 * sometimes. Clear the thumbnail cache and the readout goes silent, with no
 * error to explain why.
 *
 * What is always present is the overlay `<svg>` the editor lays over the photo
 * for masks and crop handles. `ImageCanvas` sizes it in pixels to the drawn
 * image, and it sits inside the pan/zoom transform, so its bounding box *is*
 * the photo at the current zoom and pan. Every other svg in the tree is sized
 * in percentages or not at all, which is what tells them apart.
 */
export function findPhotoSvg(): SVGSVGElement | null {
  let best: SVGSVGElement | null = null;
  let bestArea = 0;
  for (const el of Array.from(document.querySelectorAll('svg'))) {
    const { position, width, height } = el.style;
    if (position !== 'absolute' || !width.endsWith('px') || !height.endsWith('px')) {
      continue;
    }
    const rect = el.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      continue;
    }
    const area = rect.width * rect.height;
    if (area > bestArea) {
      best = el;
      bestArea = area;
    }
  }
  return best;
}

/** The photo's box on screen. Its rect is the drawn image exactly, so there is no letterboxing to undo. */
export function findPhotoBox(): DOMRect | null {
  return findPhotoSvg()?.getBoundingClientRect() ?? null;
}
