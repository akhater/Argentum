//! RawTherapee's camera profiles at one commit: which exist, the other names
//! of their bodies, and how to check a download is the file published.
//!
//! The table (`rt_profiles.tsv`) is written by `scripts/rawtherapee-profiles.mjs`
//! from RawTherapee's `rtdata/dcpprofiles` and its `camera_model_aliases.json`,
//! and `npm run check:sources` warns when RawTherapee changes the folder after
//! the commit it names. Knowing the list in the app means "Find one" no longer
//! asks GitHub's API at click time, which allows 60 calls an hour per network
//! and so failed for everyone behind one address once they ran out.

use std::sync::OnceLock;

const TABLE: &str = include_str!("rt_profiles.tsv");

/// One published profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    /// The file name, as published and as saved in the library.
    pub file: &'static str,
    pub size: u64,
    /// The file's git blob hash: SHA-1 of `blob <size>\0` and its bytes.
    pub blob: &'static str,
    /// Other names of the same body (RawTherapee's aliases), without the ones
    /// that have a profile of their own.
    pub other_names: Vec<&'static str>,
}

impl Published {
    /// The name the profile is published under: its file name without `.dcp`.
    pub fn body(&self) -> &'static str {
        self.file.strip_suffix(".dcp").unwrap_or(self.file)
    }
}

/// The RawTherapee commit the table was made from.
pub fn commit() -> &'static str {
    static COMMIT: OnceLock<&'static str> = OnceLock::new();
    COMMIT.get_or_init(|| {
        TABLE
            .lines()
            .find_map(|l| l.strip_prefix("# Source: RawTherapee rtdata/dcpprofiles @ "))
            .and_then(|rest| rest.get(..40))
            .unwrap_or_default()
    })
}

/// Every profile the table lists.
pub fn all() -> &'static [Published] {
    static ALL: OnceLock<Vec<Published>> = OnceLock::new();
    ALL.get_or_init(|| {
        TABLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .filter_map(|l| {
                let mut fields = l.split('\t');
                let file = fields.next()?;
                let size = fields.next()?.parse().ok()?;
                let blob = fields.next()?;
                let other_names = fields
                    .next()
                    .unwrap_or("")
                    .split(';')
                    .filter(|n| !n.is_empty())
                    .collect();
                Some(Published {
                    file,
                    size,
                    blob,
                    other_names,
                })
            })
            .collect()
    })
}

/// The other names RawTherapee gives the body `profile_camera` names: those
/// whose photos take its profile (a Kiss X7 the EOS 100D's).
pub fn other_names_of(profile_camera: &str) -> impl Iterator<Item = &'static str> + '_ {
    all()
        .iter()
        .filter(move |p| super::profiles::same_model(p.body(), profile_camera))
        .flat_map(|p| p.other_names.iter().copied())
}

/// Where the file is published, at the table's commit.
pub fn url(file: &str) -> String {
    let encoded: String = file
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!(
        "https://raw.githubusercontent.com/RawTherapee/RawTherapee/{}/rtdata/dcpprofiles/{encoded}",
        commit()
    )
}

/// The git blob hash of `bytes`: SHA-1 of `blob <len>\0` followed by them.
pub fn blob_hash(bytes: &[u8]) -> String {
    let mut message = format!("blob {}\0", bytes.len()).into_bytes();
    message.extend_from_slice(bytes);
    sha1(&message).iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-1 (FIPS 180-4), here because it is what git names files by and the only
/// hash the published listing carries; no crate in the build offers it on
/// every platform. It checks a download against the table, nothing more.
fn sha1(message: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&((message.len() as u64) * 8).to_be_bytes());
    for block in padded.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (hi, v) in h.iter_mut().zip([a, b, c, d, e]) {
            *hi = hi.wrapping_add(v);
        }
    }
    let mut out = [0u8; 20];
    for (chunk, v) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *chunk = v.to_be_bytes();
    }
    out
}

/// Whether `bytes` are the file the table lists for `published`.
pub fn is_published(published: &Published, bytes: &[u8]) -> bool {
    bytes.len() as u64 == published.size && blob_hash(bytes) == published.blob
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_lists_every_profile_at_one_commit() {
        assert_eq!(commit().len(), 40);
        assert!(commit().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(all().len(), 161);
        assert!(
            all()
                .iter()
                .all(|p| p.file.ends_with(".dcp") && p.size > 0 && p.blob.len() == 40)
        );
    }

    #[test]
    fn other_names_come_with_their_body() {
        let names: Vec<_> = other_names_of("Canon EOS 100D").collect();
        assert_eq!(names, ["Canon EOS Kiss X7", "Canon EOS REBEL SL1"]);
        // RawTherapee names the D800 under the D800E, but the D800 has its own.
        assert_eq!(other_names_of("NIKON D800E").count(), 0);
        assert_eq!(other_names_of("Canon EOS 5D Mark II").count(), 0);
    }

    /// FIPS 180-4's own examples, the second spanning two blocks.
    #[test]
    fn sha1_gives_the_standard_s_answers() {
        let hex = |m: &[u8]| {
            sha1(m)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(&[b'a'; 1000]),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
    }

    /// The hash git gives a file, as `git hash-object` prints it.
    #[test]
    fn the_blob_hash_is_git_s() {
        assert_eq!(blob_hash(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(
            blob_hash(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }

    #[test]
    fn only_the_published_bytes_pass() {
        let p = Published {
            file: "x.dcp",
            size: 6,
            blob: "ce013625030ba8dba906f756967f9e9ca394464a",
            other_names: vec![],
        };
        assert!(is_published(&p, b"hello\n"));
        assert!(!is_published(&p, b"hellO\n"));
        assert!(!is_published(&p, b"hello"));
    }

    #[test]
    fn the_url_is_the_pinned_commit_s() {
        let url = url("Canon EOS 5D Mark II.dcp");
        assert_eq!(
            url,
            format!(
                "https://raw.githubusercontent.com/RawTherapee/RawTherapee/{}/rtdata/dcpprofiles/Canon%20EOS%205D%20Mark%20II.dcp",
                commit()
            )
        );
    }
}
