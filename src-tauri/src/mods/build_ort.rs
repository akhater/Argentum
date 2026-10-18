//! The ONNX Runtime library for `build.rs`: checked once, and looked for on
//! this disk before anything is downloaded.
//!
//! Not part of the app: `build.rs` includes this file with `#[path]`, and
//! `mods/mod.rs` does not declare it. Their build script hashed the 12 MB
//! library every time it ran, and a new git worktree (where the library is
//! gitignored, so missing) downloaded it again. `present` answers "is the right
//! file already here?" before their code goes to the network:
//!
//! - a library this target directory has verified is not hashed again while its
//!   size and modified time are unchanged (a stamp in `OUT_DIR` records them);
//! - a linked worktree with no copy takes the main checkout's, which is then
//!   verified like any other.
//!
//! Anything else returns false and their code downloads as before.

use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

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
