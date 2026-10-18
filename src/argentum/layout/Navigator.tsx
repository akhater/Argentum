/**
 * The Navigator: the whole photo, the part on screen outlined while zoomed in,
 * and the zoom levels. Ours.
 *
 * Today's editor has no navigator, so this is the one new control in the
 * RAWmakase arrangement; it is RAWmakase's (src/app/navigator.rs: Fit and fixed
 * levels over the photo, a click moves the view there) drawn with today's
 * parts. It keeps no zoom of its own. The picture is the photo's thumbnail,
 * which today's library keeps current with its edits; the levels call today's
 * handleZoomChange, the same as the zoom control under the filmstrip; the
 * outline and the clicks go through the editor's own transform handle
 * (transformWrapperRef), the one its wheel and pinch zoom move.
 *
 * Levels stop at 200%, as today's zoom does (useEditorActions clamps there),
 * where RAWmakase goes to 400%.
 */

import { type PointerEvent, type RefObject, useEffect, useState } from 'react';
import clsx from 'clsx';
import CollapsibleSection from '../../components/ui/CollapsibleSection';
import { useEditorStore } from '../../store/useEditorStore';
import { useProcessStore } from '../../store/useProcessStore';
import { useAgTranslation } from '../locales';
import { useEditorLayout } from './editorLayout';

/** Device pixels per pixel of the photo at each level; 0 stands for Fit. */
const LEVELS: readonly number[] = [0, 0.5, 1, 2];

/** The editor's transform, as its handle reports it: pixels of offset and a scale where 1 is Fit. */
interface Transform {
  positionX: number;
  positionY: number;
  scale: number;
}

/** The part of the photo in view, as fractions of its width and height. */
interface Part {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where the fitted photo sits in the editor's frame at scale 1, in CSS pixels. */
function fittedPhoto(frame: { width: number; height: number }, aspect: number) {
  const width = Math.min(frame.width, frame.height * aspect);
  const height = width / aspect;
  return { x: (frame.width - width) / 2, y: (frame.height - height) / 2, width, height };
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

/** What the editor's transform handle offers (Editor.tsx, its useImperativeHandle). */
export interface TransformHandle {
  setTransform(x: number, y: number, scale: number, time?: number): void;
  instance: {
    wrapperComponent: HTMLElement | null;
    contentComponent: HTMLElement | null;
    readonly transformState: Transform;
  };
}

interface NavigatorProps {
  transformWrapperRef: RefObject<TransformHandle | null>;
  onZoomChange: (zoom: number, fitToWindow?: boolean) => void;
}

export default function Navigator({ transformWrapperRef, onZoomChange }: NavigatorProps) {
  const agT = useAgTranslation();
  const open = useEditorLayout((s) => s.panels.navigatorOpen);
  const changePanels = useEditorLayout((s) => s.changePanels);
  const path = useEditorStore((s) => s.selectedImage?.path);
  const preview = useEditorStore((s) => s.finalPreviewUrl);
  const base = useEditorStore((s) => s.baseRenderSize);
  const displaySize = useEditorStore((s) => s.displaySize);
  const originalSize = useEditorStore((s) => s.originalSize);
  const scale = useEditorStore((s) => s.zoom);
  const thumbnail = useProcessStore((s) => (path ? s.thumbnails[path] : undefined));
  const [transform, setTransform] = useState<Transform | null>(null);

  // The editor moves its photo by writing a CSS transform straight onto it, on
  // every step of a pan or zoom, so that write is what is watched: no polling,
  // and nothing in the editor changes for the outline to follow it.
  useEffect(() => {
    const instance = transformWrapperRef.current?.instance;
    const content = instance?.contentComponent;
    if (!instance || !content || !open) return;
    const read = () => {
      const now: Transform = instance.transformState;
      setTransform((was) =>
        was && was.positionX === now.positionX && was.positionY === now.positionY && was.scale === now.scale
          ? was
          : { ...now },
      );
    };
    read();
    const observer = new MutationObserver(read);
    observer.observe(content, { attributes: true, attributeFilter: ['style'] });
    return () => observer.disconnect();
  }, [transformWrapperRef, path, open]);

  const frame = (): { width: number; height: number } | null => {
    const wrapper = transformWrapperRef.current?.instance.wrapperComponent;
    return wrapper && wrapper.clientWidth > 0 ? { width: wrapper.clientWidth, height: wrapper.clientHeight } : null;
  };
  const aspect = base.width > 0 && base.height > 0 ? base.width / base.height : 0;

  const inView = ((): Part | null => {
    const box = frame();
    if (!transform || transform.scale <= 1.01 || !box || !aspect) return null;
    const photo = fittedPhoto(box, aspect);
    const s = transform.scale;
    const left = clamp01((-transform.positionX / s - photo.x) / photo.width);
    const top = clamp01((-transform.positionY / s - photo.y) / photo.height);
    const right = clamp01((-transform.positionX / s + box.width / s - photo.x) / photo.width);
    const bottom = clamp01((-transform.positionY / s + box.height / s - photo.y) / photo.height);
    return { left, top, width: right - left, height: bottom - top };
  })();

  // Today's zoom readout, as the control under the filmstrip works it out:
  // device pixels on screen per pixel of the photo.
  const dpr = window.devicePixelRatio || 1;
  const percent = originalSize.width > 0 && displaySize.width > 0 ? (displaySize.width * dpr) / originalSize.width : 0;
  const isOn = (zoom: number) => (zoom === 0 ? scale <= 1.001 : scale > 1.001 && Math.abs(percent - zoom) < 0.01);

  /** Centres the view on a point of the photo, given as fractions; zooms to 100% there first from Fit. */
  const showPoint = (fx: number, fy: number) => {
    const handle = transformWrapperRef.current;
    const box = frame();
    if (!handle || !box || !aspect) return;
    let s = handle.instance.transformState.scale;
    if (s <= 1.01) {
      // Today's 100%, worked out by today's zoom, then placed here at once so
      // the editor finds itself already at the zoom it was asked for.
      onZoomChange(1);
      s = useEditorStore.getState().zoom;
    }
    const photo = fittedPhoto(box, aspect);
    handle.setTransform(
      box.width / 2 - (photo.x + fx * photo.width) * s,
      box.height / 2 - (photo.y + fy * photo.height) * s,
      s,
    );
  };

  const pointAt = (e: PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    showPoint(clamp01((e.clientX - rect.left) / rect.width), clamp01((e.clientY - rect.top) / rect.height));
  };

  const src = thumbnail || preview;

  return (
    <CollapsibleSection
      canToggleVisibility={false}
      isContentVisible
      isOpen={open}
      onToggle={() => changePanels({ navigatorOpen: !open })}
      title={agT('layoutNavigator')}
    >
      <div className="flex flex-col gap-2">
        <div className="flex gap-1">
          {LEVELS.map((zoom) => (
            <button
              key={zoom}
              className={clsx(
                'flex-1 px-1 py-1 rounded-md text-xs transition-colors',
                isOn(zoom) ? 'bg-accent text-button-text' : 'bg-bg-primary text-text-secondary hover:text-text-primary',
              )}
              onClick={() => (zoom === 0 ? onZoomChange(0, true) : onZoomChange(zoom))}
            >
              {zoom === 0 ? agT('layoutFit') : `${zoom * 100}%`}
            </button>
          ))}
        </div>
        <div className="flex items-center justify-center h-24">
          {src && (
            <div
              className="relative max-w-full max-h-full cursor-crosshair touch-none"
              onPointerDown={(e) => {
                e.currentTarget.setPointerCapture(e.pointerId);
                pointAt(e);
              }}
              onPointerMove={(e) => {
                if (e.currentTarget.hasPointerCapture(e.pointerId)) pointAt(e);
              }}
            >
              <img src={src} className="block max-w-full max-h-24 rounded-sm select-none" draggable={false} alt="" />
              {inView && (
                <div
                  className="absolute border border-white shadow-[0_0_0_1px_rgba(0,0,0,0.6)] pointer-events-none"
                  style={{
                    left: `${inView.left * 100}%`,
                    top: `${inView.top * 100}%`,
                    width: `${inView.width * 100}%`,
                    height: `${inView.height * 100}%`,
                  }}
                />
              )}
            </div>
          )}
        </div>
      </div>
    </CollapsibleSection>
  );
}
