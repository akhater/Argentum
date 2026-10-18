//! A rendered view: what part of the photo it shows, where it sits in the
//! view, its sRGB pixels and its histogram, and the block it crosses to the
//! interface as.

use std::sync::OnceLock;

use rayon::prelude::*;

use crate::contract::{FRAME_HEADER_BYTES, HISTOGRAM_BINS, Placement, RenderTicket, View};
use crate::develop::fit_px;
use crate::{Rgb, Size};

/// One rendered view: sRGB, 8 bits per channel, RGBA, row-major,
/// `placement.width_px` x `placement.height_px`.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub ticket: RenderTicket,
    pub placement: Placement,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The block's size in bytes: the header and the pixels.
    pub fn block_bytes(&self) -> usize {
        FRAME_HEADER_BYTES + self.rgba.len()
    }

    /// The frame as it crosses to the interface: `FRAME_HEADER_BYTES` of
    /// ticket, width and height - little-endian u32s - then the RGBA rows.
    pub fn block(&self) -> Vec<u8> {
        let mut out = vec![0; self.block_bytes()];
        self.write_block(&mut out);
        out
    }

    /// The same, written into `out`, which is `block_bytes()` long - a
    /// buffer the interface reads directly, say.
    pub fn write_block(&self, out: &mut [u8]) {
        let (head, pixels) = out.split_at_mut(FRAME_HEADER_BYTES);
        let fields = [
            self.ticket.0,
            self.placement.width_px,
            self.placement.height_px,
        ];
        for (slot, v) in head.as_chunks_mut::<4>().0.iter_mut().zip(fields) {
            slot.copy_from_slice(&v.to_le_bytes());
        }
        pixels.copy_from_slice(&self.rgba);
    }
}

/// How a view of a photo is made: the size the pipeline renders the whole
/// photo at, the part of that render the view shows, and where the frame
/// sits in the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Layout {
    pub size: Size,
    /// Render pixels per device pixel: 1 below 100%, where the render is
    /// made at the zoom, and less past it, where render pixels repeat.
    per_screen: f64,
    /// The top left corner of the part shown, in render pixels.
    x0: f64,
    y0: f64,
    pub placement: Placement,
}

/// How `view` of a photo of `photo_px` is made.
///
/// At or past 100% the pipeline renders at full size and the view is cut out
/// of that; below it, the pipeline renders the whole photo at the zoom, and a
/// pan is a different cut of the same cached render. On an axis where the
/// photo is smaller than the view, the frame is all of it, centred in the
/// view; on an axis where it is larger, the frame fills the view.
pub(crate) fn layout(view: &View, photo_px: (u32, u32)) -> Layout {
    let (w_px, h_px) = (photo_px.0.max(1) as usize, photo_px.1.max(1) as usize);
    let zoom = view.zoom as f64;
    let (size, render_w, render_h) = if zoom >= 1.0 {
        (Size::Full, w_px, h_px)
    } else {
        let max_w = ((w_px as f64 * zoom).ceil() as u32).max(1);
        let max_h = ((h_px as f64 * zoom).ceil() as u32).max(1);
        // What the pipeline's resize will make of that: the same fit.
        let (_, render_w, render_h) = fit_px(w_px, h_px, max_w, max_h);
        (Size::Fit(max_w, max_h), render_w, render_h)
    };
    let per_screen = if zoom > 1.0 { 1.0 / zoom } else { 1.0 };
    let axis = |center: f32, view_px: u32, render_px: usize| {
        let render = render_px as f64;
        let wanted = view_px as f64 * per_screen;
        let span = wanted.min(render);
        let start = (center.clamp(0.0, 1.0) as f64 * render - span / 2.0).clamp(0.0, render - span);
        let frame_px = ((span / per_screen).round() as u32).clamp(1, view_px);
        let offset_px = if wanted >= render {
            (view_px - frame_px) / 2
        } else {
            0
        };
        (start, frame_px, offset_px)
    };
    let (x0, width_px, x_px) = axis(view.center_x, view.width, render_w);
    let (y0, height_px, y_px) = axis(view.center_y, view.height, render_h);
    Layout {
        size,
        per_screen,
        x0,
        y0,
        placement: Placement {
            x_px,
            y_px,
            width_px,
            height_px,
        },
    }
}

/// Where the frame of `view` sits in it, for a photo of
/// `photo_width_px` x `photo_height_px`.
pub fn placement(view: &View, photo_width_px: u32, photo_height_px: u32) -> Placement {
    layout(view, (photo_width_px, photo_height_px)).placement
}

/// The view's part of `rgb`, encoded for the screen.
pub(crate) fn cut(rgb: &Rgb, layout: &Layout, ticket: RenderTicket) -> Frame {
    let Placement {
        width_px,
        height_px,
        ..
    } = layout.placement;
    let per_screen = layout.per_screen;
    let sample = |o: u32, origin: f64, size: usize| {
        ((origin + (o as f64 + 0.5) * per_screen) as usize).min(size - 1)
    };
    let rgba: Vec<u8> = (0..height_px)
        .into_par_iter()
        .flat_map_iter(|oy| {
            let sy = sample(oy, layout.y0, rgb.height);
            (0..width_px).flat_map(move |ox| {
                let p = rgb.pixels[sy * rgb.width + sample(ox, layout.x0, rgb.width)];
                [encode(p[0]), encode(p[1]), encode(p[2]), 255]
            })
        })
        .collect();
    Frame {
        ticket,
        placement: layout.placement,
        rgba,
    }
}

/// Linear light to an 8-bit sRGB value, clipped at white. A plain display
/// encode until display tools take over.
///
/// From a table of 65536 steps rather than a `powf` per channel, which was
/// most of a slider tick: a step is at most a twentieth of an output level,
/// in the shadows where the curve is steepest.
fn encode(v: f32) -> u8 {
    static TABLE: OnceLock<Vec<u8>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        (0..=u16::MAX)
            .map(|i| {
                let v = i as f32 / u16::MAX as f32;
                let s = if v <= 0.003_130_8 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).round() as u8
            })
            .collect()
    });
    table[(v.clamp(0.0, 1.0) * u16::MAX as f32).round() as usize]
}

/// `HISTOGRAM_BINS` bins of the frame's luminance (Rec. 709 weights on the
/// encoded 8-bit values, one bin per level).
pub(crate) fn histogram(frame: &Frame) -> Vec<u32> {
    frame
        .rgba
        .par_chunks_exact(4)
        .fold(
            || vec![0u32; HISTOGRAM_BINS],
            |mut bins, p| {
                let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
                bins[(y.round() as usize).min(HISTOGRAM_BINS - 1)] += 1;
                bins
            },
        )
        .reduce(
            || vec![0u32; HISTOGRAM_BINS],
            |mut a, b| {
                a.iter_mut().zip(b).for_each(|(x, y)| *x += y);
                a
            },
        )
}

/// The whole photo, fitted in `max_w_px` x `max_h_px`, never enlarged.
pub(crate) fn fitted(size_px: (u32, u32), max_w_px: u32, max_h_px: u32) -> View {
    let (scale, w_px, h_px) = fit_px(size_px.0 as usize, size_px.1 as usize, max_w_px, max_h_px);
    View {
        zoom: scale as f32,
        center_x: 0.5,
        center_y: 0.5,
        width: w_px as u32,
        height: h_px as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: usize, h: usize) -> Rgb {
        Rgb::new(
            w,
            h,
            (0..w * h)
                .map(|i| {
                    let v = (i % w) as f32 / w as f32;
                    [v, v, v]
                })
                .collect(),
        )
    }

    fn view(zoom: f32, center: (f32, f32), size: (u32, u32)) -> View {
        View {
            zoom,
            center_x: center.0,
            center_y: center.1,
            width: size.0,
            height: size.1,
        }
    }

    #[test]
    fn below_100_percent_the_view_is_a_cut_of_the_render() {
        // A 800 x 600 photo at 50% is a 400 x 300 render.
        let rgb = picture(400, 300);
        let layout = layout(&view(0.5, (0.5, 0.5), (100, 50)), (800, 600));
        assert_eq!(layout.size, Size::Fit(400, 300));
        let f = cut(&rgb, &layout, RenderTicket(1));
        assert_eq!(
            f.placement,
            Placement {
                x_px: 0,
                y_px: 0,
                width_px: 100,
                height_px: 50
            }
        );
        // The middle of the render, pixel for pixel.
        assert_eq!(f.rgba[0], encode(150.0 / 400.0));
    }

    #[test]
    fn past_100_percent_pixels_repeat() {
        let rgb = picture(400, 300);
        let layout = layout(&view(4.0, (0.0, 0.0), (40, 40)), (400, 300));
        let f = cut(&rgb, &layout, RenderTicket(1));
        assert_eq!((f.placement.width_px, f.placement.height_px), (40, 40));
        // Held at the corner, ten render pixels across, each four wide.
        assert_eq!(f.rgba[0], f.rgba[3 * 4]);
        assert_ne!(f.rgba[0], f.rgba[4 * 4]);
    }

    #[test]
    fn a_photo_smaller_than_the_view_is_centred_in_it() {
        let f = cut(
            &picture(40, 30),
            &layout(&view(1.0, (0.5, 0.5), (800, 600)), (40, 30)),
            RenderTicket(1),
        );
        assert_eq!(
            f.placement,
            Placement {
                x_px: 380,
                y_px: 285,
                width_px: 40,
                height_px: 30
            }
        );
        assert_eq!(histogram(&f).iter().sum::<u32>(), 1200);
    }

    #[test]
    fn fitted_the_photo_is_letterboxed_on_one_axis_only() {
        // A 3:2 photo fitted in a 4:3 view: full width, bands above and below.
        let p = placement(&view(1600.0 / 6000.0, (0.5, 0.5), (1600, 1200)), 6000, 4000);
        assert_eq!(
            p,
            Placement {
                x_px: 0,
                y_px: 66,
                width_px: 1600,
                height_px: 1067
            }
        );
    }

    #[test]
    fn the_block_is_the_header_then_the_pixels() {
        let frame = Frame {
            ticket: RenderTicket(7),
            placement: Placement {
                x_px: 3,
                y_px: 4,
                width_px: 2,
                height_px: 1,
            },
            rgba: vec![255, 0, 0, 255, 0, 128, 255, 255],
        };
        let block = frame.block();
        assert_eq!(block.len(), FRAME_HEADER_BYTES + 8);
        assert_eq!(&block[..4], &7u32.to_le_bytes());
        assert_eq!(&block[4..8], &2u32.to_le_bytes());
        assert_eq!(&block[8..12], &1u32.to_le_bytes());
        assert_eq!(&block[12..], &frame.rgba[..]);
    }

    #[test]
    fn the_encode_is_srgb() {
        assert_eq!(encode(0.0), 0);
        assert_eq!(encode(1.0), 255);
        assert_eq!(encode(4.0), 255);
        assert_eq!(encode(0.18), 118);
        // The table against the formula, across the range.
        for i in 0..=200_000 {
            let v = i as f32 / 200_000.0;
            let s = if v <= 0.003_130_8 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            let exact = s * 255.0;
            assert!(
                (encode(v) as f32 - exact).abs() <= 0.55,
                "{v}: {} vs {exact}",
                encode(v)
            );
        }
    }
}
