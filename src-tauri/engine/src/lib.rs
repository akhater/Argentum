//! Argentum's engine.
//!
//! Tools from darktable, RawTherapee, RapidRAW, RAWmakase and Argentum, each
//! one file in `src/tools/` and one line in `registry.rs`, run in order by a
//! pipeline that keeps every tool's result. `contract.rs` holds every type
//! the interface sees, and `engine.rs` the calls it makes. See README.md for
//! the contract and how to add a tool.
//!
//! A library of its own: it builds and tests from its folder without
//! building the app. The app uses it through `src-tauri/src/mods/engine_bridge.rs`.

pub mod contract;
mod develop;
pub mod engine;
mod error;
mod gpu;
mod image;
pub mod pipeline;
pub mod registry;
pub mod tool;

pub use contract::{Origin, Section, Stage, sections};
pub use engine::{Engine, Frame};
pub use error::{Error, Result};
pub use gpu::{Gpu, GpuSlot};
pub use image::{Image, Rgb};
pub use pipeline::{Pipeline, Size, Source, Step};
pub use registry::{Registry, standard, tools};
pub use tool::{Context, Equals, Field, Kind, Settings, Tool};

/// RawImage and friends, as the tools receive them.
pub use rawler;
