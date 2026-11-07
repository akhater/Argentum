//! A HEIC's pixels decoded by macOS's ImageIO, whose HEVC decoder is the
//! system's, so Argentum ships none there.
//!
//! ImageIO decodes the file's primary image, grid and all. It is drawn into a
//! bitmap of the colour space the header says (its ICC profile, one built
//! from its CICP, or sRGB) at the depth the header says, so the pixels are
//! what [`Header`] promises whatever colour ImageIO itself makes of the file:
//! CoreGraphics converts them when the two differ. A profile is used only
//! when moxcms, which converts from it afterwards, finds it an RGB one, and
//! CoreGraphics reads it as RGB too; otherwise the pixels are drawn as sRGB.
//!
//! The image ImageIO makes is not turned (the caller turns it by the header's
//! `irot` and `imir`); whether it is cropped by the `clap` or not, the caller
//! crops what it was not. It all runs in an autorelease pool, so nothing the
//! frameworks autorelease outlives the call on a long-lived worker thread.
//!
//! ImageIO decodes lazily, when the image is drawn, and a decode that fails
//! then draws nothing: the photo would come out black, not as an error. So
//! the image is decoded when it is made, and drawn small onto black and onto
//! white first: an image drawn shows the same on both, one not drawn shows
//! the background, and is an error.
//!
//! Adapted from RAWmakase v0.4.12 (`704ab207`),
//! `crates/rawmakase-decode/src/raster/heic/imageio.rs`, without its HDR
//! colour spaces (Argentum refuses HDR HEIF) and alpha.
use super::{Header, HeicError, Pixels};
use moxcms::{ColorProfile, DataColorSpace};
use objc2::rc::autoreleasepool;
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFRetained, CFString, CGPoint, CGRect, CGSize, kCFBooleanTrue,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGColorSpaceModel, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGInterpolationQuality, kCGColorSpaceSRGB,
};
use objc2_image_io::{CGImageSource, CGImageSourceStatus, kCGImageSourceShouldCacheImmediately};

fn failed() -> HeicError {
    HeicError::NoDecoder("macOS could not decode this HEIC photo".into())
}

pub(super) fn pixels(header: &Header, bytes: &[u8]) -> Result<Pixels, HeicError> {
    autoreleasepool(|_| decode(header, bytes))
}

/// The colour space the header says: its profile, and sRGB for none, or one
/// that moxcms or CoreGraphics can't read as RGB.
fn space(header: &Header) -> Option<CFRetained<CGColorSpace>> {
    // SAFETY: a constant CoreGraphics defines.
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
    header
        .icc
        .as_ref()
        .filter(|icc| {
            ColorProfile::new_from_slice(icc)
                .is_ok_and(|profile| profile.color_space == DataColorSpace::Rgb)
        })
        .and_then(|icc| {
            // SAFETY: the data is a CFData, as the function takes.
            unsafe { CGColorSpace::with_icc_data(Some(&CFData::from_bytes(icc))) }
        })
        .filter(|space| CGColorSpace::model(Some(space)) == CGColorSpaceModel::RGB)
        .or(srgb)
}

/// Whether `image` draws: drawn 16 × 16 onto black and onto white, an image
/// comes out the same on both. The edge pixels, which smoothing may blend
/// with the background, are not compared.
fn drawn(image: &CGImage) -> Result<bool, HeicError> {
    const SIDE: usize = 16;
    // SAFETY: a constant CoreGraphics defines.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or_else(failed)?;
    let draw = |background: u8| -> Result<Vec<u8>, HeicError> {
        let mut rgbx = vec![background; SIDE * SIDE * 4];
        // SAFETY: `rgbx` holds SIDE rows of SIDE * 4 bytes, outlives the
        // context, and nothing else uses it while the context draws.
        let context = unsafe {
            CGBitmapContextCreate(
                rgbx.as_mut_ptr().cast(),
                SIDE,
                SIDE,
                8,
                SIDE * 4,
                Some(&space),
                CGImageAlphaInfo::NoneSkipLast.0,
            )
        }
        .ok_or_else(failed)?;
        let side = SIDE as f64;
        CGContext::draw_image(
            Some(&context),
            CGRect::new(CGPoint::new(0., 0.), CGSize::new(side, side)),
            Some(image),
        );
        drop(context);
        Ok(rgbx)
    };
    let (black, white) = (draw(0)?, draw(255)?);
    let inner = |pixels: &[u8]| -> Vec<[u8; 3]> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                (1..SIDE - 1).contains(&(i % SIDE)) && (1..SIDE - 1).contains(&(i / SIDE))
            })
            .map(|(_, p)| [p[0], p[1], p[2]])
            .collect()
    };
    Ok(inner(&black) == inner(&white))
}

fn decode(header: &Header, bytes: &[u8]) -> Result<Pixels, HeicError> {
    // SAFETY: no options are passed.
    let source =
        unsafe { CGImageSource::with_data(&CFData::from_bytes(bytes), None) }.ok_or_else(failed)?;
    // SAFETY: the source is a live CGImageSource.
    let index = unsafe { source.primary_image_index() };
    // SAFETY: the index is the source's own.
    let status = unsafe { source.status_at_index(index) };
    if status != CGImageSourceStatus::StatusComplete {
        return Err(failed());
    }
    // Decoded now, so a decode that fails can fail here.
    // SAFETY: a constant ImageIO defines, and CoreFoundation's true.
    let (key, yes) = unsafe { (kCGImageSourceShouldCacheImmediately, kCFBooleanTrue) };
    let yes: &CFBoolean = yes.ok_or_else(failed)?;
    let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[yes]);
    // SAFETY: the index is the source's own; the options are a dictionary of
    // an ImageIO key to a CFBoolean, as it takes.
    let image =
        unsafe { source.image_at_index(index, Some(options.as_ref())) }.ok_or_else(failed)?;
    if !drawn(&image)? {
        return Err(failed());
    }
    let (width, height) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    let size =
        |n: usize| u32::try_from(n).map_err(|_| HeicError::Corrupt("the picture is too large"));
    if !header.expects(size(width)?, size(height)?) {
        return Err(HeicError::Corrupt("the picture is not the size it says"));
    }
    let space = space(header).ok_or_else(failed)?;
    // RGB with an unused fourth sample, as CoreGraphics draws no packed RGB:
    // 8-bit, or 16-bit in the Mac's own (little-endian) order.
    let (bits, order) = if header.deep {
        (16, CGImageByteOrderInfo::Order16Little)
    } else {
        (8, CGImageByteOrderInfo::OrderDefault)
    };
    let sample = bits / 8;
    let row = width * 4 * sample;
    let mut rgbx = vec![0u8; row * height];
    // SAFETY: `rgbx` holds `height` rows of `row` bytes, outlives the
    // context, and nothing else uses it while the context draws.
    let context = unsafe {
        CGBitmapContextCreate(
            rgbx.as_mut_ptr().cast(),
            width,
            height,
            bits,
            row,
            Some(&space),
            CGImageAlphaInfo::NoneSkipLast.0 | order.0,
        )
    }
    .ok_or_else(failed)?;
    CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::None);
    CGContext::draw_image(
        Some(&context),
        CGRect::new(
            CGPoint::new(0., 0.),
            CGSize::new(width as f64, height as f64),
        ),
        Some(&image),
    );
    drop(context);
    let rgb = rgbx
        .chunks_exact(4 * sample)
        .flat_map(|pixel| &pixel[..3 * sample])
        .copied()
        .collect();
    Ok(Pixels {
        width: size(width)?,
        height: size(height)?,
        rgb,
    })
}
