//! A decoded photo, and the key that names it.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use rawler::decoders::RawDecodeParams;
use rawler::rawimage::{RawImage, RawImageData};
use rawler::rawsource::RawSource;

use crate::{Error, Result, develop};

pub struct Source {
    /// Same key, same photo. Every cache key starts from it.
    pub key: u64,
    /// The decoded mosaic, shared with the pipeline rather than copied to it.
    pub raw: Arc<RawImage>,
}

impl Source {
    pub fn open(path: impl AsRef<Path>) -> Result<Source> {
        let path = path.as_ref();
        let bytes =
            std::fs::read(path).map_err(|e| Error::Decode(format!("{}: {e}", path.display())))?;
        Self::decode(&bytes)
    }

    /// Decode with rawler, keyed by the file's bytes.
    pub fn decode(bytes: &[u8]) -> Result<Source> {
        let source = RawSource::new_from_slice(bytes);
        let decoder = rawler::get_decoder(&source).map_err(|e| Error::Decode(e.to_string()))?;
        let raw = decoder
            .raw_image(&source, &RawDecodeParams::default(), false)
            .map_err(|e| Error::Decode(e.to_string()))?;
        let mut key = DefaultHasher::new();
        bytes.hash(&mut key);
        Ok(Self::new(key.finish(), raw))
    }

    /// A photo decoded elsewhere. `key` has to change whenever `raw` does.
    pub fn new(key: u64, raw: RawImage) -> Source {
        Source {
            key,
            raw: Arc::new(raw),
        }
    }

    /// The developed picture's width and height in pixels, without
    /// developing it.
    pub fn size_px(&self) -> (usize, usize) {
        develop::developed_size_px(&self.raw)
    }

    /// The decoded sensor data's size in memory, in bytes.
    pub fn bytes(&self) -> usize {
        match &self.raw.data {
            RawImageData::Integer(v) => std::mem::size_of_val(v.as_slice()),
            RawImageData::Float(v) => std::mem::size_of_val(v.as_slice()),
        }
    }
}
