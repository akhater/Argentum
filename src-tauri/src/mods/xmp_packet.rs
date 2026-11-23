//! Finds the XMP packet embedded in an image file's bytes
//! (`<?xpacket begin=...?> ... <?xpacket end=...?>`). Cameras such as Sony and
//! phone exports keep the rating and label there instead of in a `.xmp` sidecar.

use std::io::Read;

/// Bytes read per step. Matches the chunk the caller reads from the file.
const CHUNK_SIZE: usize = 64 * 1024;
/// Largest packet accepted. Real packets are a few KB to a few hundred KB; 16 MB
/// is far above that and still bounds what one file can make us hold in memory.
const MAX_XMP_PACKET_SIZE: usize = 16 * 1024 * 1024;
const XMP_START: &[u8] = b"<?xpacket begin=";
const XMP_END: &[u8] = b"<?xpacket end=";
const XMP_CLOSE: &[u8] = b"?>";

/// Position of the first `needle` in `haystack`. Each byte of the haystack is
/// visited once; the needle is compared only where its first byte matches.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    let last_start = haystack.len().checked_sub(needle.len())?;
    let mut at = 0;
    while at <= last_start {
        let hit = at + haystack[at..=last_start].iter().position(|&b| b == first)?;
        if haystack[hit + 1..].starts_with(rest) {
            return Some(hit);
        }
        at = hit + 1;
    }
    None
}

/// Reads `reader` in chunks and returns the first complete embedded XMP packet.
///
/// Linear in the bytes read: each byte is searched for a marker once, with a
/// small overlap kept across chunk boundaries, so a marker split between two
/// chunks is still found. Nothing already searched is searched again.
pub fn find_in_reader<R: Read>(mut reader: R) -> Option<String> {
    let mut chunk = vec![0u8; CHUNK_SIZE];
    // Before the start marker: the unmatched tail. From the start marker on: the packet.
    let mut buf: Vec<u8> = Vec::new();
    let mut in_packet = false;
    // Next index in `buf` where the marker being searched for could begin.
    let mut from = 0;
    // Where the end marker begins, once it has been found.
    let mut end_at: Option<usize> = None;

    loop {
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..read]);

        if !in_packet {
            match find_bytes(&buf, XMP_START) {
                Some(start) => {
                    buf.drain(..start);
                    in_packet = true;
                    from = 0;
                }
                None => {
                    let keep = XMP_START.len() - 1;
                    if buf.len() > keep {
                        buf.drain(..buf.len() - keep);
                    }
                    continue;
                }
            }
        }

        if end_at.is_none() {
            match find_bytes(&buf[from..], XMP_END) {
                Some(i) => {
                    end_at = Some(from + i);
                    from += i + XMP_END.len();
                }
                None => from = buf.len().saturating_sub(XMP_END.len() - 1),
            }
        }
        if end_at.is_some() {
            if let Some(i) = find_bytes(&buf[from..], XMP_CLOSE) {
                buf.truncate(from + i + XMP_CLOSE.len());
                return String::from_utf8(buf).ok();
            }
            from = buf.len().saturating_sub(XMP_CLOSE.len() - 1);
        }

        if buf.len() > MAX_XMP_PACKET_SIZE {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_management::extract_xmp_rating;
    use std::time::{Duration, Instant};

    #[test]
    fn reads_rating_from_embedded_xmp_packet() {
        let mut buf = vec![0u8; 200_000];
        buf.extend_from_slice(
            b"<?xpacket begin='\xef\xbb\xbf' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
              <x:xmpmeta xmlns:x='adobe:ns:meta/'>\n\
              <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
              <rdf:Description rdf:about='' xmlns:xmp='http://ns.adobe.com/xap/1.0/'>\n\
              <xmp:Rating>5</xmp:Rating>\n</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n\
              <?xpacket end='w'?>",
        );
        buf.extend_from_slice(&vec![0xffu8; 50_000]);

        let packet = find_in_reader(&buf[..]).expect("packet found");
        assert!(packet.contains("<xmp:Rating>5</xmp:Rating>"));
        assert_eq!(extract_xmp_rating(&packet), Some(5));
    }

    #[test]
    fn returns_none_when_no_packet() {
        assert!(find_in_reader(&vec![0x11u8; 400_000][..]).is_none());
    }

    #[test]
    fn a_packet_straddling_a_chunk_boundary_is_found() {
        let packet = b"<?xpacket begin=''?><xmp:Rating>4</xmp:Rating><?xpacket end='w'?>";
        for offset in [CHUNK_SIZE - 5, CHUNK_SIZE - 1, CHUNK_SIZE, 0] {
            let mut buf = vec![0u8; offset];
            buf.extend_from_slice(packet);
            buf.extend_from_slice(&[0u8; 100]);
            let found = find_in_reader(&buf[..]).expect("packet found");
            assert_eq!(found.as_bytes(), &packet[..], "offset {offset}");
        }
    }

    #[test]
    fn a_packet_at_the_very_end_of_the_file_is_found() {
        let packet = b"<?xpacket begin=''?><xmp:Rating>2</xmp:Rating><?xpacket end='w'?>";
        let mut buf = vec![0u8; 300_000];
        buf.extend_from_slice(packet);
        assert_eq!(
            find_in_reader(&buf[..]).as_deref().map(str::as_bytes),
            Some(&packet[..])
        );
    }

    #[test]
    fn the_first_of_two_packets_wins() {
        let first = b"<?xpacket begin=''?><xmp:Rating>1</xmp:Rating><?xpacket end='w'?>";
        let second = b"<?xpacket begin=''?><xmp:Rating>5</xmp:Rating><?xpacket end='w'?>";
        let mut buf = first.to_vec();
        buf.extend_from_slice(&[0u8; 1000]);
        buf.extend_from_slice(second);
        assert_eq!(
            find_in_reader(&buf[..]).as_deref().map(str::as_bytes),
            Some(&first[..])
        );
    }

    #[test]
    fn a_start_without_an_end_is_none() {
        let mut buf = b"<?xpacket begin=''?>".to_vec();
        buf.extend_from_slice(&[b'x'; 1000]);
        assert!(find_in_reader(&buf[..]).is_none());
    }

    /// A start marker followed by the whole 16 MB packet cap of near-matches of
    /// the end marker and no real end. Every chunk used to rescan the whole
    /// packet so far.
    #[test]
    fn a_long_unterminated_packet_is_scanned_in_linear_time() {
        let mut buf = b"<?xpacket begin=".to_vec();
        while buf.len() < MAX_XMP_PACKET_SIZE + CHUNK_SIZE {
            buf.extend_from_slice(b"<?xpacket end");
        }
        let started = Instant::now();
        let found = find_in_reader(&buf[..]);
        let elapsed = started.elapsed();
        eprintln!("scan of 16 MB unterminated packet: {elapsed:?}");
        assert!(found.is_none());
        assert!(
            elapsed < Duration::from_secs(1),
            "scan took {elapsed:?} on a 16 MB file"
        );
    }

    #[test]
    fn find_bytes_matches_the_naive_search() {
        let hay = b"aa<?xpacket begin=<?xpacket begi<?xpacket begin=x";
        assert_eq!(find_bytes(hay, XMP_START), Some(2));
        assert_eq!(find_bytes(hay, b"x"), Some(4));
        assert_eq!(find_bytes(hay, b"x"), hay.iter().position(|&b| b == b'x'));
        assert_eq!(find_bytes(hay, b"zz"), None);
        assert_eq!(find_bytes(b"ab", b"abc"), None);
        assert_eq!(find_bytes(hay, b""), None);
    }
}
