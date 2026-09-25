//! Immutable, content-addressed record packs: the writer every pack family
//! shares (see [`crate::record_tables`] for their indexes).
//!
//! A pack is one zstd frame of concatenated records, named by the hash of its
//! file bytes. Each record is independently hashed, so a reader verifies
//! exactly the records it decodes.

use crate::record_codec::{DecodeRecord, EncodeRecord};
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, io, path::Path};

pub(crate) const SOURCE_LAYOUT: Layout = Layout {
    directory: "source-records",
    pack_bytes: PACK_BYTES,
};

/// Where a family's packs live, and how large they grow.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub(crate) directory: &'static str,
    /// Target uncompressed pack size. A selective read inflates one whole pack,
    /// so facts read one file at a time use small packs.
    pub(crate) pack_bytes: usize,
}
/// Default pack size for facts that are read in bulk.
pub(crate) const PACK_BYTES: usize = 8 * 1024 * 1024;

impl Layout {
    /// Packs below this size are combined when more than one survives.
    pub(crate) const fn small_pack_bytes(self) -> u64 {
        (self.pack_bytes >> 3) as u64
    }
}

/// Where the writer put one record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Packed {
    pub(crate) pack: String,
    pub(crate) hash: String,
    pub(crate) offset: u64,
    pub(crate) len: u64,
}

/// Record encoding: each fact type's native [`EncodeRecord`] encoding.
pub(crate) const NATIVE_FORMAT: u32 = 3;

pub(crate) fn decode<T: DecodeRecord + DeserializeOwned>(
    format: u32,
    bytes: &[u8],
) -> io::Result<T> {
    if format >= NATIVE_FORMAT {
        T::decode_record(bytes)
    } else {
        serde_json::from_slice(bytes).map_err(io::Error::other)
    }
}

pub(crate) fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

struct Pending {
    offset: u64,
    len: u64,
    paths: Vec<String>,
}

/// Packs records into new packs of about `limit` bytes. Identical records
/// written together share one range.
pub(crate) struct Writer<'a> {
    directory: &'a Path,
    /// Inflated sizes of the packs written.
    pub(crate) sizes: BTreeMap<String, u64>,
    pub(crate) records: BTreeMap<String, Packed>,
    known: BTreeMap<String, Packed>,
    bytes: Vec<u8>,
    pending: BTreeMap<String, Pending>,
    limit: usize,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(directory: &'a Path, limit: usize) -> Self {
        Self {
            directory,
            sizes: BTreeMap::new(),
            records: BTreeMap::new(),
            known: BTreeMap::new(),
            bytes: Vec::new(),
            pending: BTreeMap::new(),
            limit,
        }
    }

    pub(crate) fn add<T: EncodeRecord>(&mut self, path: &str, record: &T) -> io::Result<()> {
        // Encode directly into the pack buffer. Only a boundary-crossing
        // record needs a separate allocation while the preceding pack is flushed.
        let start = self.bytes.len();
        record.encode_record(&mut self.bytes)?;
        self.finish_record(path, start)
    }

    pub(crate) fn add_encoded(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let start = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        self.finish_record(path, start)
    }

    fn finish_record(&mut self, path: &str, start: usize) -> io::Result<()> {
        let hash = graph_search_core::hash::content_hash(&self.bytes[start..]);
        if let Some(reference) = self.known.get(&hash).cloned() {
            self.bytes.truncate(start);
            self.records.insert(path.to_owned(), reference);
            return Ok(());
        }
        if let Some(pending) = self.pending.get_mut(&hash) {
            self.bytes.truncate(start);
            pending.paths.push(path.to_owned());
            return Ok(());
        }
        let offset = if start > 0 && self.bytes.len() > self.limit {
            let record_bytes = self.bytes.split_off(start);
            self.flush()?;
            self.bytes = record_bytes;
            0
        } else {
            start
        };
        self.pending.insert(
            hash,
            Pending {
                offset: offset as u64,
                len: self.bytes.len().saturating_sub(offset) as u64,
                paths: vec![path.to_owned()],
            },
        );
        if self.bytes.len() >= self.limit {
            self.flush()?;
        }
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> io::Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let file = crate::compress::deflate(&self.bytes)?;
        let pack = graph_search_core::hash::content_hash(&file);
        self.sizes.insert(pack.clone(), self.bytes.len() as u64);
        // Content-named: a pack already present in a shared directory is the
        // same pack, committed by an earlier generation.
        let target = self.directory.join(&pack);
        if !target.exists() {
            crate::generation::replace(&target, &file)?;
        }
        for (hash, pending) in std::mem::take(&mut self.pending) {
            let reference = Packed {
                pack: pack.clone(),
                hash: hash.clone(),
                offset: pending.offset,
                len: pending.len,
            };
            self.known.insert(hash, reference.clone());
            for path in pending.paths {
                self.records.insert(path, reference.clone());
            }
        }
        self.bytes.clear();
        Ok(())
    }
}

#[allow(clippy::integer_division)] // Round the allowed dead quarter down, retaining at least 75% live.
pub(crate) fn minimum_live(bytes: usize) -> usize {
    bytes.saturating_sub(bytes / 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::source::SourceFileUnits;

    fn record(hash: &str) -> SourceFileUnits {
        SourceFileUnits {
            source_hash: hash.into(),
            version: 1,
            ..SourceFileUnits::default()
        }
    }

    #[test]
    fn pack_rollover_dedup_and_oversized_records_preserve_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        let mut writer = Writer::new(root.path(), 150);
        let files = BTreeMap::from([
            ("a".to_owned(), record("a")),
            ("a-copy".into(), record("a")),
            ("b".into(), record("b")),
            ("c".into(), record(&"x".repeat(200))),
            ("duplicate".into(), record("a")),
        ]);
        for (path, record) in &files {
            writer.add(path, record).unwrap();
        }
        writer.flush().unwrap();
        assert!(writer.bytes.is_empty());
        assert_eq!(writer.sizes.len(), 2);
        assert_eq!(writer.records["a"], writer.records["duplicate"]);
        assert_eq!(writer.records["a"], writer.records["a-copy"]);
        for (path, packed) in &writer.records {
            let file = std::fs::read(root.path().join(&packed.pack)).unwrap();
            assert_eq!(graph_search_core::hash::content_hash(&file), packed.pack);
            let bytes = crate::compress::inflate_pack(&file).unwrap();
            let start = usize::try_from(packed.offset).unwrap();
            let end = start + usize::try_from(packed.len).unwrap();
            let slice = &bytes[start..end];
            assert_eq!(graph_search_core::hash::content_hash(slice), packed.hash);
            let decoded: SourceFileUnits = decode(NATIVE_FORMAT, slice).unwrap();
            assert_eq!(decoded, files[path]);
        }
    }
}
