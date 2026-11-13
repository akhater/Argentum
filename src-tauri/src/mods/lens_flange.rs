//! Whether a lens of one mount can be adapted to a body of another. Ours.
//!
//! Lensfun lists which mounts fit a body directly (`<compat>`), not which can
//! be adapted. An adapter is a tube between the lens's mount and the body's: it
//! can only exist when the lens's flange focal distance (mount to sensor) is
//! longer than the body's by at least the thinnest adapter that holds both
//! mounts. So a Canon EF lens goes on a Sony E body (44 mm over 18 mm) and a
//! Nikon F lens on a Canon EF one (46.5 over 44), but an RF lens never on an
//! EF body, nor a Micro Four Thirds lens on Sony E (1.25 mm, thinner than any
//! adapter). An EF-S lens on an EF-only body has no room either (the same
//! 44 mm), as Lensfun's `docs/mounts.txt` says of EF-S: "Not usable on cameras
//! with crop 1.0 and on EF-only cameras".
//!
//! Source of the distances: each maker's specification, as listed in
//! Wikipedia's "Flange focal distance" table (read 2026-11-13). None of the
//! projects Argentum harvests from (RAWmakase, darktable, Lensfun, ExifTool)
//! records them. Keys are Lensfun's mount names; a mount not listed here is
//! never taken as adaptable.

/// The thinnest adapter between two mounts, in millimetres: the M42 to Canon EF
/// ring (45.46 − 44.00 = 1.46 mm) is the thinnest common one. Micro Four Thirds
/// to Sony E (1.25 mm) and Pentax K to Sony A (0.96 mm) have none.
pub const MIN_ADAPTER_MM: f32 = 1.4;

/// Lensfun's mount names and their flange focal distances, in millimetres.
const FLANGE_MM: &[(&str, f32)] = &[
    ("4/3 System", 38.67),
    ("Canon EF", 44.0),
    ("Canon EF-M", 18.0),
    ("Canon EF-S", 44.0),
    ("Canon FD", 42.0),
    ("Canon FL", 42.0),
    ("Canon RF", 20.0),
    ("Contax/Yashica", 45.5),
    ("DKL", 44.7),
    ("Exakta", 44.7),
    ("Fujifilm G", 26.7),
    ("Fujifilm X", 17.7),
    ("Hasselblad 500", 74.9),
    ("Hasselblad H", 61.63),
    ("Hasselblad X", 18.14),
    ("Kiev 88", 82.1),
    ("Leica L", 20.0),
    ("Leica M", 27.8),
    ("M39", 28.8),
    ("M39/1", 45.2),
    ("M42", 45.46),
    ("Mamiya 645", 63.3),
    ("Micro 4/3", 19.25),
    ("Micro 4/3 System", 19.25),
    ("Minolta AF", 44.5),
    ("Minolta M", 43.5),
    ("Minolta MC", 43.5),
    ("Minolta MD", 43.5),
    ("Nikon CX", 17.0),
    ("Nikon F", 46.5),
    ("Nikon F AF", 46.5),
    ("Nikon F AI", 46.5),
    ("Nikon F AI-S", 46.5),
    ("Nikon Z", 16.0),
    ("Olympus OM", 46.0),
    ("Pentak K", 45.46),
    ("Pentax 645AF2", 70.87),
    ("Pentax K", 45.46),
    ("Pentax KA", 45.46),
    ("Pentax KAF", 45.46),
    ("Pentax KAF2", 45.46),
    ("Pentax KAF3", 45.46),
    ("Pentax KAF4", 45.46),
    ("Pentax Q", 9.2),
    ("Samsung NX", 25.5),
    ("Samsung NX mini", 6.95),
    ("Sigma SA", 44.0),
    ("Sony Alpha", 44.5),
    ("Sony E", 18.0),
    ("T2", 55.0),
];

/// The flange focal distance of the Lensfun mount `mount`, in millimetres.
pub fn flange_mm(mount: &str) -> Option<f32> {
    let mount = mount.trim();
    FLANGE_MM
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(mount))
        .map(|(_, mm)| *mm)
}

/// Whether a lens of `lens_mount` can go on a body of `body_mount` through an
/// adapter: its flange is longer by at least [`MIN_ADAPTER_MM`].
pub fn adaptable(lens_mount: &str, body_mount: &str) -> bool {
    match (flange_mm(lens_mount), flange_mm(body_mount)) {
        (Some(lens), Some(body)) => lens - body >= MIN_ADAPTER_MM,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The adapters that exist and those that cannot.
    #[test]
    fn adapters_exist_only_where_there_is_room() {
        for (lens, body) in [
            ("Canon EF", "Sony E"),
            ("Pentax KAF2", "Fujifilm X"),
            ("Nikon F AF", "Nikon Z"),
            ("Nikon F AF", "Sony Alpha"),
            ("Nikon F AF", "Canon EF"),
            ("M42", "Canon EF"),
            ("Sony Alpha", "Sony E"),
        ] {
            assert!(adaptable(lens, body), "{lens} on {body}");
        }
        for (lens, body) in [
            ("Canon RF", "Canon EF"),
            ("Canon EF-S", "Canon EF"),
            ("Canon EF", "Sony Alpha"),
            ("Pentax KAF", "Sony Alpha"),
            ("Micro 4/3 System", "Sony E"),
            ("Canon EF", "Unknown mount"),
        ] {
            assert!(!adaptable(lens, body), "{lens} on {body}");
        }
    }

    /// Every interchangeable-lens mount the bundled database's bodies have, and
    /// every such mount a dozen lenses or more have, has a distance. Not "Rollei
    /// 6x6", which names two Rollei systems of different flanges (the SL66's and
    /// the 6000 series'), nor "Generic".
    #[test]
    fn the_database_s_mounts_have_a_distance() {
        let db = crate::lens_correction::read_lensfun_dir(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/lensfun_db"
        )));
        let mut lens_mounts = std::collections::HashMap::<String, usize>::new();
        for lens in &db.lenses {
            for m in &lens.mount {
                *lens_mounts.entry(m.trim().to_string()).or_default() += 1;
            }
        }
        let body_mounts = db
            .cameras
            .iter()
            .map(|c| c.mount.trim().to_string())
            .filter(|m| m.chars().next().is_some_and(|c| !c.is_lowercase()));
        let missing: Vec<String> = body_mounts
            .chain(
                lens_mounts
                    .into_iter()
                    .filter(|(_, n)| *n >= 12)
                    .map(|(m, _)| m),
            )
            .filter(|m| m.chars().next().is_some_and(|c| !c.is_lowercase()))
            .filter(|m| !["Generic", "Rollei 6x6"].contains(&m.as_str()) && flange_mm(m).is_none())
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }
}
