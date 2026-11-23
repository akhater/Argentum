//! Our own copy of every file Argentum downloads. Ours.
//!
//! Each file Argentum fetches, at build time or in the app, has one address on
//! someone else's server: when that server goes, the build or the feature goes
//! with it. So each is also kept, byte for byte and under its
//! published name, as an asset of a GitHub release of `akhater/argentum-models`
//! (its release notes name every file's source and licence). That copy is
//! fetched first and the original address is the fallback; both are checked
//! against the same pinned SHA-256, so neither can hand over anything else.
//!
//! `build.rs` includes this file with `#[path]` too, so the address is written
//! here once for the build and the app.
//!
//! Its test keeps `download-addresses.tsv` (beside `Cargo.toml`): every address
//! the build and the app download from, which `npm run check:downloads` asks
//! each week whether it still answers.

/// Where our copies are kept: `mirror!()` is every release's parent,
/// `mirror!(release)` one release's folder, `mirror!(release, file)` one file.
macro_rules! mirror {
    () => {
        "https://github.com/akhater/argentum-models/releases/download/"
    };
    ($release:literal $(, $file:literal)?) => {
        concat!(mirror!(), $release, "/" $(, $file)?)
    };
}
pub(crate) use mirror;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{build_ort, gpu_runtime, model_catalog};

    /// Every address Argentum downloads from, for `npm run check:downloads`
    /// (scripts/check-downloads.mjs), beside `Cargo.toml`.
    const LIST: &str = "download-addresses.tsv";

    /// `name<TAB>address` for every address, each file's in the order tried:
    /// the build's ONNX Runtime, the DirectML package, every catalogued model.
    fn every_address() -> Vec<String> {
        let mut lines = Vec::new();
        for runtime in build_ort::RUNTIMES {
            for url in runtime.urls() {
                lines.push(format!("{}\t{url}", runtime.file));
            }
        }
        let files = std::iter::once(gpu_runtime::PACKAGE).chain(
            model_catalog::CATALOGUE
                .iter()
                .flat_map(|m| m.stored.files()),
        );
        for file in files {
            for url in std::iter::once(file.url).chain(file.fallback) {
                let line = format!("{}\t{url}", file.name);
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
        }
        lines
    }

    /// The check asks the addresses the build and the app use, never a copy
    /// kept by hand: the committed list is the one these lists make. When it
    /// differs it is rewritten here, and this fails until that is committed.
    #[test]
    fn the_address_list_is_current() {
        let made = format!(
            "# Every address Argentum downloads from, written by a test in src/mods/mirror.rs.\n{}\n",
            every_address().join("\n")
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(LIST);
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if committed != made {
            std::fs::write(&path, made).expect("write the address list");
            panic!("{LIST} was out of date and has been rewritten: commit it");
        }
    }
}
