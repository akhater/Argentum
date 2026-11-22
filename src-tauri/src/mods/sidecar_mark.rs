//! A rule mark at the top of a photo's own sidecar, read. Ours.
//!
//! A rule that changes a photo's decode (`highlight_rule.rs`,
//! `demosaic_rule.rs`) keeps every edit on the rule it was made with: a
//! photo's own sidecar carries the newest rule's mark from its first write,
//! and one written before that rule carries none. This reads the mark; each
//! rule says what its words mean.

use std::path::Path;

/// What a photo's own sidecar says under one rule's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    /// No sidecar: the photo has no edit to keep.
    NoSidecar,
    /// A sidecar, and no word under the key: one saved before the rule, or
    /// one that does not read, which was still an edit.
    Absent,
    /// The word under the key.
    Word(String),
}

/// The mark under `key` in the sidecar at `sidecar`.
pub fn read(sidecar: &Path, key: &str) -> Mark {
    let text = match std::fs::read_to_string(sidecar) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Mark::NoSidecar,
        Err(_) => return Mark::Absent,
    };
    // Most sidecars predate the marks, and some hold a long History: the key
    // is looked for before anything is parsed.
    if !text.contains(key) {
        return Mark::Absent;
    }
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(mut top)) => match top.remove(key) {
            Some(serde_json::Value::String(word)) => Mark::Word(word),
            _ => Mark::Absent,
        },
        _ => Mark::Absent,
    }
}

/// The photo's own sidecar for `photo` (a file, or a virtual copy's path):
/// the decode is the file's, shared by its virtual copies.
pub fn own_sidecar(photo: &str) -> std::path::PathBuf {
    let (source, _) = crate::file_management::split_virtual_path(photo);
    crate::file_management::sidecar_for(Path::new(source), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_case() {
        let dir = std::env::temp_dir().join("argentum-sidecar-mark");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sidecar = dir.join("a.agdata");
        assert_eq!(read(&sidecar, "agX"), Mark::NoSidecar);
        std::fs::write(&sidecar, r#"{"version":1}"#).unwrap();
        assert_eq!(read(&sidecar, "agX"), Mark::Absent);
        std::fs::write(&sidecar, r#"{"version":1,"agX":"new"}"#).unwrap();
        assert_eq!(read(&sidecar, "agX"), Mark::Word("new".into()));
        std::fs::write(&sidecar, r#"{"version":1,"agX":3}"#).unwrap();
        assert_eq!(read(&sidecar, "agX"), Mark::Absent);
        std::fs::write(&sidecar, "{ agX not json").unwrap();
        assert_eq!(read(&sidecar, "agX"), Mark::Absent);
    }
}
