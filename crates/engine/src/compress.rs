//! zstd framing for committed generation bytes.
//!
//! Record packs and the large JSON sidecars are written once and read on every
//! open, so a fast codec with a good ratio on text beats smaller-but-slower ones:
//! level 3 inflates at close to memory speed on both x86 and ARM.
//! Fingerprints always cover the committed (compressed) bytes, so corruption is
//! detected before any inflation.

use std::io;

/// Compression level for packs and sidecars (see `research/results/storage-formats`).
const LEVEL: i32 = 3;
/// Rejects corrupt pack frame headers before allocation. Packs target 8 MiB;
/// only a single oversized record gets a larger pack of its own.
const MAX_PACK_RAW: u64 = 1 << 30;

/// One zstd frame, including its content size.
pub(crate) fn deflate(bytes: &[u8]) -> io::Result<Vec<u8>> {
    zstd::bulk::compress(bytes, LEVEL)
}

/// Inflates a record pack into one allocation sized from its frame header,
/// bounded by [`MAX_PACK_RAW`] and checked against the declared size.
pub(crate) fn inflate_pack(file: &[u8]) -> io::Result<Vec<u8>> {
    let size = frame_size(file)?;
    if size > MAX_PACK_RAW {
        return Err(io::Error::other("pack frame exceeds size limit"));
    }
    let size = usize::try_from(size).map_err(io::Error::other)?;
    let bytes = zstd::bulk::decompress(file, size)?;
    if bytes.len() != size {
        return Err(io::Error::other("pack frame size mismatch"));
    }
    Ok(bytes)
}

/// Inflated size from a frame header alone (the first bytes of the frame).
pub(crate) fn frame_size(header: &[u8]) -> io::Result<u64> {
    zstd::zstd_safe::get_frame_content_size(header)
        .map_err(|_| io::Error::other("invalid zstd frame"))?
        .ok_or_else(|| io::Error::other("zstd frame without content size"))
}

/// Inflates a sidecar by streaming, so memory tracks actual content rather
/// than a size claimed by the header.
pub(crate) fn inflate_sidecar(file: &[u8]) -> io::Result<Vec<u8>> {
    zstd::stream::decode_all(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_reject_corruption() {
        let text = b"{\"occurrences\":[1,2,3]}".repeat(100);
        let frame = deflate(&text).unwrap();
        assert!(frame.len() < text.len());
        assert_eq!(frame_size(&frame).unwrap(), text.len() as u64);
        assert_eq!(inflate_pack(&frame).unwrap(), text);
        assert_eq!(inflate_sidecar(&frame).unwrap(), text);
        assert!(inflate_pack(b"not a frame").is_err());
        assert!(inflate_sidecar(b"not a frame").is_err());
        assert!(inflate_pack(&frame[..frame.len() - 1]).is_err());
        assert!(inflate_sidecar(&frame[..frame.len() - 1]).is_err());
    }
}
