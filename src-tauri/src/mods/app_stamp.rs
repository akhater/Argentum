//! The application's name as it is written into the files it makes: the
//! EXIF Software tag of an export, the toolkit name of an XMP packet, the
//! User-Agent of a request. Defined once here.

pub const APP_NAME: &str = "Argentum";

/// Opening tag of an XMP packet's root element, carrying the toolkit name.
/// Both XMP writers (export packet, sidecar skeleton) start their root with it.
pub fn xmpmeta_open() -> String {
    format!(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="{APP_NAME}">"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_argentum() {
        assert_eq!(APP_NAME, "Argentum");
    }

    #[test]
    fn the_xmp_root_differs_from_the_old_one_only_in_the_toolkit_name() {
        let old = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="RapidRAW">"#;
        assert_eq!(xmpmeta_open(), old.replace("RapidRAW", "Argentum"));
    }
}
