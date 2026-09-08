//! Argentum's own code.
//!
//! Everything in here is ours — upstream RapidRAW has never seen these files,
//! so they can't conflict when we merge its updates. Harvested algorithms get a
//! header naming the source file and upstream commit; the matching reference
//! copy lives in `docs/harvest/`.
//!
//! See `docs/ARCHITECTURE.md` for why the split matters, and
//! `docs/ADDING_A_TOOL.md` for the recipe.

pub mod auto_wb;
pub mod commands;
