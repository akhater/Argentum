//! A HEIC's pixels decoded by Windows Imaging Component, whose HEIF decoder
//! and HEVC decoder are Microsoft's, so Argentum ships none there.
//!
//! Both come from the Microsoft Store, as the HEIF Image Extensions and the
//! HEVC Video Extensions, and a PC may lack either: then WIC can't decode the
//! file, and the error says what to install.
//!
//! WIC's frame is converted to 8- or 16-bit RGB, the depth the header says,
//! and read as it is: in the colour space of the file, unturned; the caller
//! crops, turns and converts it.
//!
//! Adapted from RAWmakase v0.4.12 (`704ab207`),
//! `crates/rawmakase-decode/src/raster/heic/wic.rs`: read from memory here,
//! through a WIC stream, rather than from an open file.
use super::{Header, HeicError, Pixels};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat24bppRGB, GUID_WICPixelFormat48bppRGB,
    IWICImagingFactory, WICConvertBitmapSource, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

/// What to install, said when WIC can't decode a HEIC.
const INSTALL: &str = "HEIC photos need Microsoft's HEIF Image Extensions and HEVC Video \
     Extensions, from the Microsoft Store";

/// COM on this thread for as long as it lives: initialized here unless the
/// thread already was, in another mode, which serves as well.
struct Com(bool);

impl Com {
    fn init() -> Self {
        // SAFETY: no reserved pointer; balanced by `drop` when it succeeds.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx in `init`.
            unsafe { CoUninitialize() }
        }
    }
}

pub(super) fn pixels(header: &Header, bytes: &[u8]) -> Result<Pixels, HeicError> {
    // Declared first, so dropped after every COM object below.
    let _com = Com::init();
    let missing = |e: windows::core::Error| {
        HeicError::NoDecoder(format!(
            "Windows could not decode this HEIC photo ({}). {INSTALL}.",
            e.message().trim_end_matches(['.', '\r', '\n'])
        ))
    };
    // SAFETY: COM is initialized on this thread; the factory is an
    // in-process server.
    let factory: IWICImagingFactory =
        unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
            .map_err(missing)?;
    // SAFETY: a live factory.
    let stream = unsafe { factory.CreateStream() }.map_err(missing)?;
    // SAFETY: `bytes` outlive the stream and every object made from it, all
    // dropped at the end of this function; WIC only reads them.
    unsafe { stream.InitializeFromMemory(bytes) }.map_err(missing)?;
    // SAFETY: a live stream over `bytes`; no vendor is preferred.
    let decoder = unsafe {
        factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
    }
    .map_err(missing)?;
    // SAFETY: frame 0 is the primary image.
    let frame = unsafe { decoder.GetFrame(0) }.map_err(missing)?;
    let (format, pixel) = if header.deep {
        (GUID_WICPixelFormat48bppRGB, 6)
    } else {
        (GUID_WICPixelFormat24bppRGB, 3)
    };
    // SAFETY: the format is a WIC pixel format GUID that outlives the call.
    let rgb_source = unsafe { WICConvertBitmapSource(&format, &frame) }.map_err(missing)?;
    let (mut width, mut height) = (0, 0);
    // SAFETY: both pointers are to live locals.
    unsafe { rgb_source.GetSize(&mut width, &mut height) }.map_err(missing)?;
    if !header.expects(width, height) {
        return Err(HeicError::Corrupt("the picture is not the size it says"));
    }
    let stride = width
        .checked_mul(pixel)
        .ok_or(HeicError::Corrupt("the picture is too large"))?;
    let mut rgb = vec![0u8; stride as usize * height as usize];
    // SAFETY: a null rectangle copies the whole image, `height` rows of
    // `stride` bytes, which `rgb` holds.
    unsafe { rgb_source.CopyPixels(std::ptr::null(), stride, &mut rgb) }.map_err(missing)?;
    Ok(Pixels { width, height, rgb })
}
