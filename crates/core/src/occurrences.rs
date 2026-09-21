//! Source-owned occurrences and generation-local lookup structures.
use graph_search_types::occurrence::{
    OccurrenceExtent, OccurrenceFile, ReferenceOccurrence, ResolutionClass,
};
use graph_search_types::{Node, NodeId, WriteBatch};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// Identity encodes only source facts. Target/reason changes do not rename evidence.
#[must_use]
pub fn identity(path: &str, source_hash: &str, record: &ReferenceOccurrence) -> String {
    let mut key = String::from("reference-occurrence-v1:");
    for value in [
        path,
        source_hash,
        record.owner.as_str(),
        record.kind.as_str(),
        &record.name,
        record.raw_name.as_deref().unwrap_or_default(),
    ] {
        key.push_str(&value.len().to_string());
        key.push(':');
        key.push_str(value);
    }
    let _ = write!(
        key,
        "|{}|{}|{}",
        record.raw_name.is_some(),
        record.ordinal,
        record.line
    );
    if let Some(span) = record.span {
        let _ = write!(
            key,
            "|span:{}:{}:{}:{}",
            span.start_line, span.end_line, span.start_byte, span.end_byte
        );
    } else {
        key.push_str("|no-span");
    }
    format!("occ:{}", crate::hash::content_hash(key.as_bytes()))
}

/// Validates file/hash/owner/range identities before mutation and on reopen.
/// Target bindings are validated separately against the final graph.
/// # Errors
/// On malformed, foreign or inconsistent occurrence facts.
pub fn validate<'a>(
    file: &Node,
    facts: &OccurrenceFile,
    lookup: impl Fn(&NodeId) -> Option<&'a Node>,
) -> crate::Result<()> {
    let mut ids = BTreeSet::new();
    let invalid = file.content_hash.as_deref() != Some(facts.source_hash.as_str())
        || facts.version != graph_search_types::limits::OCCURRENCE_VERSION
        || facts.records.len() > graph_search_types::limits::MAX_EDGES_PER_FILE
        || facts.records.iter().any(|record| {
            record.id != identity(&file.path, &facts.source_hash, record)
                || !ids.insert(record.id.as_str())
                || record.line == 0
                || (record.target.is_some() != (record.resolution != ResolutionClass::Unresolved))
                || record.span.is_some_and(|span| {
                    span.start_line != record.line
                        || span.start_line == 0
                        || span.end_line < span.start_line
                        || span.end_byte < span.start_byte
                        || file
                            .bytes
                            .is_some_and(|bytes| u64::from(span.end_byte) > bytes)
                })
                || (record.span.is_none() != (record.extent == OccurrenceExtent::LineOnly))
                || lookup(&record.owner).is_none_or(|owner| {
                    owner.path != file.path
                        || (!owner.is_file()
                            && record.span.is_some_and(|span| {
                                owner.span.is_none_or(|parent| {
                                    parent.start_byte > span.start_byte
                                        || span.end_byte > parent.end_byte
                                })
                            }))
                })
        });
    if invalid {
        return Err(crate::Error::Store(format!(
            "invalid source occurrences for {}",
            file.path
        )));
    }
    Ok(())
}

/// Replace only facts owned by changed source files. A deleted target invalidates
/// its binding, not the unchanged source occurrence or its source-derived identity.
pub fn apply(
    files: &mut BTreeMap<String, OccurrenceFile>,
    batch: &WriteBatch,
    exists: impl Fn(&NodeId) -> bool,
) {
    for path in &batch.removed_files {
        files.remove(path);
    }
    for upsert in &batch.upserts {
        files.remove(&upsert.file.path);
        if let Some(facts) = &upsert.occurrences {
            files.insert(upsert.file.path.clone(), facts.clone());
        }
    }
    for facts in files.values_mut() {
        for record in &mut facts.records {
            if record.target.as_ref().is_some_and(|id| !exists(id)) {
                record.target = None;
                record.target_name.clone_from(&record.name);
                record.resolution = ResolutionClass::Unresolved;
                record.reason = Some("target_removed".into());
            }
        }
    }
}

/// Compact lookup positions; occurrence source records remain owned by file facts.
#[derive(Default)]
pub struct OccurrenceIndex {
    extracted_files: usize,
    files: Vec<String>,
    targets: BTreeMap<NodeId, Vec<(usize, usize)>>,
    owners: BTreeMap<NodeId, Vec<(usize, usize)>>,
    names: BTreeMap<String, Vec<(usize, usize)>>,
    edges: BTreeMap<String, Vec<(usize, usize)>>,
}
impl OccurrenceIndex {
    /// Builds indexes in deterministic path/source-order without cloning records.
    #[must_use]
    pub fn new(files: &BTreeMap<String, OccurrenceFile>) -> Self {
        let mut index = Self::default();
        for (path, facts) in files {
            index.extracted_files = index
                .extracted_files
                .saturating_add(usize::from(facts.complete));
            let file = index.files.len();
            index.files.push(path.clone());
            for (record, occurrence) in facts.records.iter().enumerate() {
                let position = (file, record);
                index
                    .owners
                    .entry(occurrence.owner.clone())
                    .or_default()
                    .push(position);
                if let Some(target) = &occurrence.target {
                    index
                        .targets
                        .entry(target.clone())
                        .or_default()
                        .push(position);
                }
                index
                    .names
                    .entry(
                        occurrence
                            .raw_name
                            .as_ref()
                            .unwrap_or(&occurrence.name)
                            .clone(),
                    )
                    .or_default()
                    .push(position);
                index
                    .edges
                    .entry(occurrence.edge_id().to_string())
                    .or_default()
                    .push(position);
            }
        }
        index
    }
    /// Known occurrences attached to one aggregate relationship; absent is unknown/zero.
    #[must_use]
    pub fn count_for_edge(&self, id: &str) -> Option<usize> {
        self.edges.get(id).map(Vec::len)
    }

    pub(crate) fn edge_positions(&self, id: &str) -> &[(usize, usize)] {
        self.edges.get(id).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn coverage(&self) -> (usize, usize) {
        (self.files.len(), self.extracted_files)
    }

    pub(crate) fn positions(
        &self,
        by: graph_search_types::occurrence::OccurrenceBy,
        key: &str,
    ) -> &[(usize, usize)] {
        use graph_search_types::occurrence::OccurrenceBy;
        match by {
            OccurrenceBy::Target => self.targets.get(&NodeId::new(key)),
            OccurrenceBy::Owner => self.owners.get(&NodeId::new(key)),
            OccurrenceBy::Name => self.names.get(key),
        }
        .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn record<'a>(
        &'a self,
        files: &'a BTreeMap<String, OccurrenceFile>,
        position: (usize, usize),
    ) -> Option<(&'a str, &'a OccurrenceFile, &'a ReferenceOccurrence)> {
        let path = self.files.get(position.0)?;
        let file = files.get(path)?;
        Some((path, file, file.records.get(position.1)?))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{EdgeKind, Span};

    fn record() -> ReferenceOccurrence {
        ReferenceOccurrence {
            id: String::new(),
            owner: NodeId::file("src/a.rs"),
            kind: EdgeKind::Calls,
            span: Some(Span::new(2, 2, 4, 7)),
            line: 2,
            extent: OccurrenceExtent::Expression,
            raw_name: Some("b".into()),
            name: "b".into(),
            ordinal: 0,
            target: Some(NodeId::file("src/b.rs")),
            target_name: "b".into(),
            resolution: ResolutionClass::UniqueName,
            reason: None,
            scope: None,
            binding: None,
        }
    }

    #[test]
    fn identity_preserves_source_evidence_across_rebinding() {
        let mut reference = record();
        let original = identity("src/a.rs", "ha", &reference);
        reference.target = None;
        reference.target_name = "different display name".into();
        reference.resolution = ResolutionClass::Unresolved;
        reference.reason = Some("target_removed".into());
        assert_eq!(identity("src/a.rs", "ha", &reference), original);
        reference.ordinal += 1;
        assert_ne!(identity("src/a.rs", "ha", &reference), original);
        reference.ordinal = 0;
        reference.span.as_mut().unwrap().start_byte += 1;
        assert_ne!(identity("src/a.rs", "ha", &reference), original);
        assert_ne!(identity("src/a.rs", "hb", &record()), original);
    }

    #[test]
    fn target_deletion_keeps_each_source_occurrence_and_reindexes_the_binding() {
        let mut first = record();
        first.id = identity("src/a.rs", "ha", &first);
        let mut second = first.clone();
        second.ordinal = 1;
        second.span = Some(Span::new(2, 2, 7, 10));
        second.id = identity("src/a.rs", "ha", &second);
        let ids = [first.id.clone(), second.id.clone()];
        let edge = first.edge_id().to_string();
        let mut files = BTreeMap::from([(
            "src/a.rs".into(),
            OccurrenceFile {
                source_hash: "ha".into(),
                version: graph_search_types::limits::OCCURRENCE_VERSION,
                complete: true,
                records: vec![first, second],
            },
        )]);
        assert_eq!(OccurrenceIndex::new(&files).count_for_edge(&edge), Some(2));
        apply(&mut files, &WriteBatch::default(), |_| false);
        let records = &files["src/a.rs"].records;
        for (reference, id) in records.iter().zip(ids) {
            assert_eq!(reference.id, id);
            assert!(reference.target.is_none());
            assert_eq!(reference.reason.as_deref(), Some("target_removed"));
            assert_eq!(identity("src/a.rs", "ha", reference), id);
        }
        let index = OccurrenceIndex::new(&files);
        assert_eq!(index.count_for_edge(&edge), None);
        assert_eq!(
            index.count_for_edge(&records[0].edge_id().to_string()),
            Some(2)
        );
    }

    #[test]
    fn validation_rejects_foreign_owners_duplicate_facts_and_out_of_file_ranges() {
        let batch = crate::conformance::fixture_batch();
        let file = &batch.upserts[0].file;
        let mut reference = record();
        reference.id = identity(&file.path, "ha", &reference);
        let valid = OccurrenceFile {
            source_hash: "ha".into(),
            version: graph_search_types::limits::OCCURRENCE_VERSION,
            complete: true,
            records: vec![reference],
        };
        let lookup = |id: &NodeId| (id == &file.id).then_some(file);
        assert!(validate(file, &valid, lookup).is_ok());
        let mut duplicate = valid.clone();
        duplicate.records.push(duplicate.records[0].clone());
        assert!(validate(file, &duplicate, lookup).is_err());
        for foreign in [false, true] {
            let mut invalid = valid.clone();
            let reference = &mut invalid.records[0];
            if foreign {
                reference.owner = NodeId::file("foreign.rs");
            } else {
                reference.span.as_mut().unwrap().end_byte = 11;
            }
            reference.id = identity(&file.path, "ha", reference);
            assert!(validate(file, &invalid, lookup).is_err());
        }
    }
}
