//! What flows from one tool to the next.

use std::sync::Arc;

use rawler::rawimage::{RawImage, RawImageData};

/// A picture at some point in the pipeline.
#[derive(Debug, Clone)]
pub enum Image {
    /// Sensor data before demosaic, as rawler decodes it: a Bayer or X-Trans
    /// mosaic (`cpp` 1), or three colours per pixel (sRAW, linear DNG). What
    /// `Stage::Raw` tools receive. Shared, so the photo's own mosaic is never
    /// copied to be passed along.
    Raw(Arc<RawImage>),
    /// Demosaiced RGB, linear light. What `Stage::SceneLinear` tools receive.
    Rgb(Rgb),
}

/// Three f32 channels per pixel, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Rgb {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
    /// The width this picture has at full resolution. A preview is smaller,
    /// and a tool whose radius is in sensor pixels scales it by `scale()`.
    pub full_width: usize,
}

impl Rgb {
    /// A picture at full resolution.
    pub fn new(width: usize, height: usize, pixels: Vec<[f32; 3]>) -> Self {
        assert_eq!(pixels.len(), width * height, "one pixel per position");
        Self {
            width,
            height,
            pixels,
            full_width: width,
        }
    }

    /// Render pixels per full-resolution pixel: 1 at full size.
    pub fn scale(&self) -> f32 {
        if self.full_width == 0 {
            1.0
        } else {
            self.width as f32 / self.full_width as f32
        }
    }

    /// The same picture with other pixels: size and scale carried over.
    pub fn with_pixels(&self, pixels: Vec<[f32; 3]>) -> Self {
        assert_eq!(pixels.len(), self.pixels.len(), "same size");
        Self { pixels, ..*self }
    }
}

impl Image {
    /// Roughly how much memory the pixels take, for the cache's budget.
    pub fn bytes(&self) -> usize {
        match self {
            Image::Raw(raw) => match &raw.data {
                RawImageData::Integer(v) => v.len() * 2,
                RawImageData::Float(v) => v.len() * 4,
            },
            Image::Rgb(rgb) => rgb.pixels.len() * 12,
        }
    }

    pub fn as_raw(&self) -> Option<&RawImage> {
        match self {
            Image::Raw(raw) => Some(raw.as_ref()),
            Image::Rgb(_) => None,
        }
    }

    pub fn as_rgb(&self) -> Option<&Rgb> {
        match self {
            Image::Rgb(rgb) => Some(rgb),
            Image::Raw(_) => None,
        }
    }

    /// What kind of image this is, for error messages.
    pub fn describe(&self) -> &'static str {
        match self {
            Image::Raw(_) => "a raw (pre-demosaic) image",
            Image::Rgb(_) => "an RGB image",
        }
    }
}
