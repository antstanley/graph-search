//! Occurrence counts per aggregate relationship, published with a generation
//! so that relationship queries report them without loading every source
//! occurrence fact.
//!
//! Layout: `GSE1`, then entries of a 32-byte BLAKE3 digest of the edge id and
//! a little-endian `u32` count, strictly ascending by digest. Lookups binary
//! search the committed bytes in place.

use graph_search_types::occurrence::OccurrenceFile;
use std::collections::BTreeMap;
use std::io;

/// The artifact name inside a generation.
pub(crate) const FILE: &str = "edge-occurrences.bin";
const MAGIC: &[u8; 4] = b"GSE1";
const KEY: usize = 32;
const ENTRY: usize = KEY + 4;

/// Encodes the count of every relationship with at least one occurrence.
pub(crate) fn encode(files: &BTreeMap<String, OccurrenceFile>) -> io::Result<Vec<u8>> {
    let mut counts: BTreeMap<[u8; KEY], u32> = BTreeMap::new();
    for record in files.values().flat_map(|facts| &facts.records) {
        let count = counts
            .entry(graph_search_core::hash::digest(
                record.edge_id().as_str().as_bytes(),
            ))
            .or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| io::Error::other("edge occurrence count overflow"))?;
    }
    let mut out = MAGIC.to_vec();
    for (key, count) in counts {
        out.extend_from_slice(&key);
        out.extend_from_slice(&count.to_le_bytes());
    }
    Ok(out)
}

/// A verified, decoded table.
pub(crate) struct EdgeCounts(Vec<u8>);

impl EdgeCounts {
    /// Checks framing and strict key order; the bytes were already verified
    /// against the generation pointer.
    pub(crate) fn decode(bytes: Vec<u8>) -> io::Result<Self> {
        let body = bytes.strip_prefix(MAGIC).unwrap_or_default();
        let (entries, rest) = body.as_chunks::<ENTRY>();
        if !bytes.starts_with(MAGIC)
            || !rest.is_empty()
            || entries
                .windows(2)
                .any(|pair| pair[0][..KEY] >= pair[1][..KEY])
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid edge occurrence table",
            ));
        }
        Ok(Self(bytes))
    }

    fn entries(&self) -> &[[u8; ENTRY]] {
        self.0
            .strip_prefix(MAGIC)
            .unwrap_or_default()
            .as_chunks::<ENTRY>()
            .0
    }

    /// The occurrences recorded for one relationship; `None` when there are none.
    pub(crate) fn get(&self, edge_id: &str) -> Option<usize> {
        let key = graph_search_core::hash::digest(edge_id.as_bytes());
        let entries = self.entries();
        let entry = entries
            .binary_search_by(|entry| entry[..KEY].cmp(&key))
            .ok()
            .and_then(|index| entries.get(index))?;
        let (_, count) = entry.split_last_chunk::<4>()?;
        usize::try_from(u32::from_le_bytes(*count)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::occurrence::{OccurrenceExtent, ReferenceOccurrence, ResolutionClass};
    use graph_search_types::{EdgeKind, NodeId};

    fn record(owner: &str, name: &str, ordinal: u32) -> ReferenceOccurrence {
        ReferenceOccurrence {
            id: String::new(),
            owner: NodeId::new(owner),
            kind: EdgeKind::Calls,
            span: None,
            line: 1,
            extent: OccurrenceExtent::LineOnly,
            raw_name: None,
            name: name.into(),
            ordinal,
            target: None,
            target_name: name.into(),
            resolution: ResolutionClass::Unresolved,
            reason: None,
            scope: None,
            binding: None,
        }
    }

    #[test]
    fn counts_match_the_occurrence_index_and_reject_bad_framing() {
        let facts = OccurrenceFile {
            source_hash: "h".into(),
            version: graph_search_types::limits::OCCURRENCE_VERSION,
            complete: true,
            records: vec![
                record("a", "x", 0),
                record("a", "x", 1),
                record("a", "y", 0),
            ],
        };
        let files = BTreeMap::from([(String::from("a.rs"), facts)]);
        let index = graph_search_core::occurrences::OccurrenceIndex::new(&files);
        let table = EdgeCounts::decode(encode(&files).unwrap()).unwrap();
        for record in &files["a.rs"].records {
            let id = record.edge_id();
            assert_eq!(table.get(id.as_str()), index.count_for_edge(id.as_str()));
        }
        assert_eq!(table.get("absent"), None);
        assert_eq!(table.get(record("a", "x", 0).edge_id().as_str()), Some(2));

        let bytes = encode(&files).unwrap();
        assert!(EdgeCounts::decode(bytes[..bytes.len() - 1].to_vec()).is_err());
        assert!(EdgeCounts::decode(b"GSE0".to_vec()).is_err());
        let mut swapped = bytes[..MAGIC.len()].to_vec();
        swapped.extend_from_slice(&bytes[MAGIC.len() + ENTRY..]);
        swapped.extend_from_slice(&bytes[MAGIC.len()..MAGIC.len() + ENTRY]);
        assert!(EdgeCounts::decode(swapped).is_err(), "keys must ascend");
        assert!(
            EdgeCounts::decode(MAGIC.to_vec())
                .unwrap()
                .get("x")
                .is_none()
        );
    }
}
