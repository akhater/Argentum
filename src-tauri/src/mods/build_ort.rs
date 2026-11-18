//! The ONNX Runtime library for `build.rs`: checked once, and looked for on
//! this disk before anything is downloaded.
//!
//! Not part of the app: `build.rs` includes this file with `#[path]`. Their
//! build script hashed the 12 MB library every time it ran, and a new git
//! worktree (where the library is gitignored, so missing) downloaded it again. `present` answers "is the right
//! file already here?" before their code goes to the network:
//!
//! - a library this target directory has verified is not hashed again while its
//!   size and modified time are unchanged (a stamp in `OUT_DIR` records them);
//! - a linked worktree with no copy takes the main checkout's, which is then
//!   verified like any other.
//!
//! Anything else returns false and their code downloads it: from our copy
//! first (`mirror`), then from RapidRAW's, where it was first published.
//!
//! `mods/mod.rs` declares this file for tests only, which hold every address
//! in `RUNTIMES` to the rule every download follows.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::mirror::mirror;

/// One target's ONNX Runtime 1.22.0 library: published as `file`, saved in
/// `resources/` as `lib`.
pub struct Runtime {
    pub os: &'static str,
    pub arch: &'static str,
    pub file: &'static str,
    pub lib: &'static str,
    pub sha256: &'static str,
}

impl Runtime {
    /// Where it is downloaded from, in the order tried: our copy, then RapidRAW's.
    pub fn urls(&self) -> [String; 2] {
        [
            format!("{}{}", mirror!("onnxruntime-1.22.0"), self.file),
            format!(
                "https://huggingface.co/CyberTimon/RapidRAW-Models/resolve/main/onnxruntimes-v1.22.0/{}?download=true",
                self.file
            ),
        ]
    }
}

/// Every target the app builds for (RapidRAW's build.rs at 1.6.6, moved here
/// unchanged from `build.rs`).
pub const RUNTIMES: &[Runtime] = &[
    Runtime {
        os: "windows",
        arch: "x86_64",
        file: "onnxruntime-windows-x86_64.dll",
        lib: "onnxruntime.dll",
        sha256: "579b636403983254346a5c1d80bd28f1519cd1e284cd204f8d4ff41f8d711559",
    },
    Runtime {
        os: "windows",
        arch: "aarch64",
        file: "onnxruntime-windows-aarch64.dll",
        lib: "onnxruntime.dll",
        sha256: "79281671a386ed1baab9dbdbb09fe55f99577011472e9526cf9d0b468bb6bcc7",
    },
    Runtime {
        os: "linux",
        arch: "x86_64",
        file: "libonnxruntime-linux-x86_64.so",
        lib: "libonnxruntime.so",
        sha256: "3da6146e14e7b8aaec625dde11d6114c7457c87a5f93d744897da8781e35c673",
    },
    Runtime {
        os: "linux",
        arch: "aarch64",
        file: "libonnxruntime-linux-aarch64.so",
        lib: "libonnxruntime.so",
        sha256: "0afd69a0ae38c5099fd0e8604dda398ac43dee67cd9c6394b5142b19e82528de",
    },
    Runtime {
        os: "macos",
        arch: "x86_64",
        file: "libonnxruntime-macos-x86_64.dylib",
        lib: "libonnxruntime.dylib",
        sha256: "283e595e61cf65df7a6b1d59a1616cbd35c8b6399dd90d799d99b71a3ff83160",
    },
    Runtime {
        os: "macos",
        arch: "aarch64",
        file: "libonnxruntime-macos-aarch64.dylib",
        lib: "libonnxruntime.dylib",
        sha256: "2b885992d3d6fa4130d39ec84a80d7504ff52750027c547bb22c86165f19406a",
    },
];

/// True when `dest` holds the library whose SHA-256 is `expected`.
pub fn present(dest: &Path, expected: &str) -> bool {
    if !dest.exists() && !copy_from_main_checkout(dest) {
        return false;
    }
    let Some(seen) = size_and_time(dest).map(|id| format!("{expected} {id}")) else {
        return false;
    };
    let stamp = env::var_os("OUT_DIR").map(|dir| {
        let name = dest.file_name().unwrap_or_default().to_string_lossy();
        PathBuf::from(dir).join(format!("{name}.verified"))
    });
    if let Some(stamp) = &stamp
        && fs::read_to_string(stamp).is_ok_and(|text| text == seen)
    {
        return true;
    }
    if !sha256(dest).is_ok_and(|hash| hash == expected) {
        return false;
    }
    if let Some(stamp) = &stamp {
        let _ = fs::write(stamp, seen);
    }
    true
}

fn size_and_time(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{} {}", meta.len(), modified.as_nanos()))
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// In a linked worktree, `.git` is a file naming the worktree's git directory,
/// and its `commondir` leads to the main repository's `.git`, whose parent is
/// the main checkout. Copies the same file from there, if it has one.
fn copy_from_main_checkout(dest: &Path) -> bool {
    let Some(repo) = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|manifest| manifest.parent().map(Path::to_path_buf))
    else {
        return false;
    };
    let Some(git_dir) = fs::read_to_string(repo.join(".git")).ok().and_then(|link| {
        link.trim()
            .strip_prefix("gitdir:")
            .map(|p| repo.join(p.trim()))
    }) else {
        return false;
    };
    let Some(main) = fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .and_then(|common| git_dir.join(common.trim()).canonicalize().ok())
        .and_then(|common| common.parent().map(Path::to_path_buf))
    else {
        return false;
    };
    let Ok(relative) = dest.strip_prefix(&repo) else {
        return false;
    };
    let source = main.join(relative);
    if !source.is_file() || fs::copy(&source, dest).is_err() {
        return false;
    }
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    println!("cargo:warning=Copied {name} from the main checkout instead of downloading it.");
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A build never depends on one server: each target's library comes from
    /// our copy, and from RapidRAW's when ours cannot be reached.
    #[test]
    fn every_runtime_comes_from_our_copy_then_rapidraws() {
        for runtime in RUNTIMES {
            let [ours, original] = runtime.urls();
            assert!(
                ours.starts_with(mirror!()) && ours.ends_with(runtime.file),
                "{ours}"
            );
            assert!(
                original.starts_with("https://huggingface.co/CyberTimon/RapidRAW-Models/")
                    && original.contains(runtime.file),
                "{original}"
            );
            assert_eq!(runtime.sha256.len(), 64, "{}", runtime.file);
        }
    }

    #[test]
    fn each_target_is_listed_once() {
        for (i, a) in RUNTIMES.iter().enumerate() {
            for b in &RUNTIMES[i + 1..] {
                assert!(
                    (a.os, a.arch) != (b.os, b.arch),
                    "{}-{} twice",
                    a.os,
                    a.arch
                );
                assert_ne!(a.file, b.file);
            }
        }
    }
}
