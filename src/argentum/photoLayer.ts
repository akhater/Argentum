/**
 * The layer the photo's overlay svg sits in, followed every frame. Ours.
 *
 * Mask guides are drawn into it so they move with their pan and zoom without
 * any geometry of ours; see `photoBox.ts` for why the svg is the photo.
 */

import { useEffect, useState } from 'react';
import { findPhotoSvg } from './photoBox';

/** The overlay svg's place and size, and how many screen pixels one of its pixels is. */
export interface PhotoLayout {
  svg: SVGSVGElement;
  left: string;
  top: string;
  width: number;
  height: number;
  zoom: number;
}

export function svgSize(svg: SVGSVGElement) {
  return { width: parseFloat(svg.style.width) || 0, height: parseFloat(svg.style.height) || 0 };
}

/** The photo's layer while `active`. Their canvas remounts it, and pan and zoom change its size on screen. */
export function usePhotoLayer(active: boolean): PhotoLayout | null {
  const [layout, setLayout] = useState<PhotoLayout | null>(null);
  useEffect(() => {
    if (!active) {
      setLayout(null);
      return;
    }
    let raf = 0;
    const tick = () => {
      const svg = findPhotoSvg();
      if (svg) {
        const { width, height } = svgSize(svg);
        const rect = svg.getBoundingClientRect();
        const zoom = width > 0 ? rect.width / width : 1;
        const { left, top } = svg.style;
        setLayout((was) =>
          was &&
          was.svg === svg &&
          was.left === left &&
          was.top === top &&
          was.width === width &&
          was.height === height &&
          Math.abs(was.zoom - zoom) < 1e-4
            ? was
            : { svg, left, top, width, height, zoom },
        );
      } else {
        setLayout(null);
      }
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(raf);
  }, [active]);
  return layout;
}
