//! A rendered view: what part of the photo it shows, where it sits in the
//! view, its sRGB pixels and its histogram, and the block it crosses to the
//! interface as. The pixels themselves come from the pixel pass
//! (`src/pixels/`), which reads the columns and rows picked here.

use rayon::prelude::*;

use crate::Size;
use crate::contract::{FRAME_HEADER_BYTES, HISTOGRAM_BINS, Placement, RenderTicket, View};
use crate::develop::fit_px;

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

impl Layout {
    /// For each column of the frame, the column of a render `render_width`
    /// pixels wide it shows: the cut, and past 100% each render pixel
    /// repeated. The pixel pass reads these.
    pub(crate) fn columns(&self, render_width: usize) -> Vec<u32> {
        picks(
            self.placement.width_px,
            self.x0,
            self.per_screen,
            render_width,
        )
    }

    /// The same for each row.
    pub(crate) fn rows(&self, render_height: usize) -> Vec<u32> {
        picks(
            self.placement.height_px,
            self.y0,
            self.per_screen,
            render_height,
        )
    }
}

/// The render pixel under the middle of each of `frame_px` frame pixels.
fn picks(frame_px: u32, origin: f64, per_screen: f64, render_px: usize) -> Vec<u32> {
    (0..frame_px)
        .map(|o| ((origin + (o as f64 + 0.5) * per_screen) as usize).min(render_px - 1) as u32)
        .collect()
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
                let y = crate::colour::luma([p[0], p[1], p[2]].map(f32::from));
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
        let layout = layout(&view(0.5, (0.5, 0.5), (100, 50)), (800, 600));
        assert_eq!(layout.size, Size::Fit(400, 300));
        assert_eq!(
            layout.placement,
            Placement {
                x_px: 0,
                y_px: 0,
                width_px: 100,
                height_px: 50
            }
        );
        // The middle of the render, pixel for pixel.
        assert_eq!(layout.columns(400), (150..250).collect::<Vec<u32>>());
        assert_eq!(layout.rows(300), (125..175).collect::<Vec<u32>>());
    }

    #[test]
    fn past_100_percent_pixels_repeat() {
        let layout = layout(&view(4.0, (0.0, 0.0), (40, 40)), (400, 300));
        assert_eq!(
            (layout.placement.width_px, layout.placement.height_px),
            (40, 40)
        );
        // Held at the corner, ten render pixels across, each four wide.
        let columns = layout.columns(400);
        assert_eq!(columns[..8], [0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(columns[39], 9);
    }

    #[test]
    fn a_photo_smaller_than_the_view_is_centred_in_it() {
        let layout = layout(&view(1.0, (0.5, 0.5), (800, 600)), (40, 30));
        assert_eq!(
            layout.placement,
            Placement {
                x_px: 380,
                y_px: 285,
                width_px: 40,
                height_px: 30
            }
        );
        assert_eq!(layout.columns(40), (0..40).collect::<Vec<u32>>());
    }

    #[test]
    fn the_histogram_counts_every_pixel_by_its_luminance() {
        let f = Frame {
            ticket: RenderTicket(1),
            placement: Placement {
                x_px: 0,
                y_px: 0,
                width_px: 3,
                height_px: 1,
            },
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255, 118, 118, 118, 255],
        };
        let bins = histogram(&f);
        assert_eq!(bins.iter().sum::<u32>(), 3);
        assert_eq!((bins[0], bins[118], bins[255]), (1, 1, 1));
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
}
