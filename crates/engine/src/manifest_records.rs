//! Cold extraction facts, one record per file in their own pack family
//! (format 15, `research/16-proportional-sync.md` phase 1i); `manifest.json`
//! is the small freshness header. A record is the file's complete manifest
//! entry, so hydration verifies that facts belong to that exact fingerprint.
//! The family's index is posting tables (see [`crate::record_tables`]), so a
//! publish writes only the records that changed.

use crate::record_codec::EncodeRecord;
use crate::record_tables::{Delta, Family, Records};
use crate::source_records::Layout;
use graph_search_types::manifest::{FileEntry, Manifest};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    sync::RwLock,
};

pub(crate) const LAYOUT: Layout = Layout {
    directory: "extraction-records",
    pack_bytes: crate::source_records::PACK_BYTES,
};

/// Every file's extraction record, by path.
pub(crate) const RECORDS: Family = Family {
    layout: LAYOUT,
    paths: "extraction_paths",
    packs: "extraction_packs",
    small: "extraction_small",
};

/// The facts a handle has written or read, by path: the header they belong to
/// and a weak identity, so an unchanged value is recognized without encoding
/// it and without keeping it alive.
pub(crate) type Identities = BTreeMap<
    String,
    (
        FileEntry,
        std::sync::Weak<graph_search_types::extraction::Extraction>,
    ),
>;

/// The identities of every value `manifest` carries.
pub(crate) fn identities(manifest: &Manifest) -> Identities {
    manifest
        .entries
        .iter()
        .filter_map(|(path, entry)| {
            entry
                .extraction
                .as_ref()
                .map(|facts| (path.clone(), (entry.header(), facts.downgrade())))
        })
        .collect()
}

fn poisoned() -> io::Error {
    io::Error::other("extraction identity lock poisoned")
}

/// `header` with every file's facts, each verified against its entry.
pub(crate) fn hydrate(
    objects: &Path,
    header: &Manifest,
    records: Option<&Records<'_>>,
    cache: &RwLock<Identities>,
) -> io::Result<Manifest> {
    let loaded = match records {
        Some(records) => records.load::<FileEntry>(objects)?,
        None => BTreeMap::new(),
    };
    let manifest = hydrate_records(header, loaded)?;
    *cache.write().map_err(|_| poisoned())? = identities(&manifest);
    Ok(manifest)
}

/// Only the facts of `paths`, each verified against its entry. Identity
/// additions do not evict other selected reads or retain fact payloads.
pub(crate) fn selected(
    objects: &Path,
    header: &Manifest,
    records: Option<&Records<'_>>,
    cache: &RwLock<Identities>,
    paths: &BTreeSet<String>,
) -> io::Result<graph_search_core::ports::ExtractionFacts> {
    let Some(records) = records else {
        return Ok(graph_search_core::ports::ExtractionFacts::new());
    };
    let facts = validate_records(header, records.load_selected::<FileEntry>(objects, paths)?)?;
    let mut identities = cache.write().map_err(|_| poisoned())?;
    for (path, facts) in &facts {
        let entry = header
            .entries
            .get(path)
            .ok_or_else(|| io::Error::other("extraction record without manifest owner"))?;
        identities.insert(path.clone(), (entry.clone(), facts.downgrade()));
    }
    Ok(facts)
}

fn validate_records(
    header: &Manifest,
    records: BTreeMap<String, FileEntry>,
) -> io::Result<graph_search_core::ports::ExtractionFacts> {
    records
        .into_iter()
        .map(|(path, record)| validate_record(header, &path, record).map(|facts| (path, facts)))
        .collect()
}

fn validate_record(
    header: &Manifest,
    path: &str,
    mut record: FileEntry,
) -> io::Result<graph_search_types::extraction::SharedExtraction> {
    let extraction = record
        .extraction
        .take()
        .ok_or_else(|| io::Error::other("empty extraction record"))?;
    let entry = header
        .entries
        .get(path)
        .ok_or_else(|| io::Error::other("extraction record without manifest owner"))?;
    if entry != &record {
        return Err(io::Error::other("extraction record fingerprint mismatch"));
    }
    Ok(extraction)
}

fn hydrate_records(
    header: &Manifest,
    records: BTreeMap<String, FileEntry>,
) -> io::Result<Manifest> {
    if header
        .entries
        .values()
        .any(|entry| entry.extraction.is_some())
    {
        return Err(io::Error::other("a header carries no extraction values"));
    }
    let mut manifest = header.clone();
    for (path, record) in records {
        let facts = validate_record(header, &path, record)?;
        let entry = manifest
            .entries
            .get_mut(&path)
            .ok_or_else(|| io::Error::other("extraction record without manifest owner"))?;
        entry.extraction = Some(facts);
    }
    Ok(manifest)
}

/// One publish's change to the extraction records, and the paths that have
/// one afterwards.
pub(crate) struct Update {
    pub(crate) delta: Delta,
    pub(crate) paths: BTreeSet<String>,
}

/// Writes the records of `manifest` that changed, keeps the unchanged ones
/// and those of `retained` (whose values the publish did not load), and drops
/// every other path in `old_paths`. A value is unchanged when it is the one
/// this handle last wrote or read for the same header, or when its encoding
/// hashes to the stored record's.
pub(crate) fn update(
    objects: &Path,
    old: Option<&Records<'_>>,
    old_paths: &BTreeSet<String>,
    manifest: &Manifest,
    cache: &RwLock<Identities>,
    retained: &BTreeSet<String>,
) -> io::Result<Update> {
    let identities = cache.read().map_err(|_| poisoned())?;
    let mut writes: BTreeMap<String, &FileEntry> = BTreeMap::new();
    let mut paths: BTreeSet<String> = retained.clone();
    for (path, entry) in &manifest.entries {
        let Some(facts) = entry.extraction.as_ref() else {
            continue;
        };
        paths.insert(path.clone());
        if !old_paths.contains(path) {
            writes.insert(path.clone(), entry);
            continue;
        }
        let known = identities.get(path).is_some_and(|(header, identity)| {
            header == &entry.header() && facts.matches_identity(identity)
        });
        let unchanged = known
            || match old.map(|old| old.get(path)).transpose()?.flatten() {
                Some(stored) => {
                    let mut bytes = Vec::new();
                    (&entry).encode_record(&mut bytes)?;
                    graph_search_core::hash::content_hash(&bytes) == stored.hash
                }
                None => false,
            };
        if !unchanged {
            writes.insert(path.clone(), entry);
        }
    }
    drop(identities);
    let dropped: BTreeSet<String> = old_paths.difference(&paths).cloned().collect();
    let delta = crate::record_tables::update(objects, RECORDS, old, &writes, &dropped)?;
    Ok(Update { delta, paths })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{Table, TableRef, Tables};
    use graph_search_types::{
        EdgeKind,
        extraction::{Extraction, ReferenceFact},
    };

    /// A standalone family: its tables' references and the path set.
    #[derive(Default)]
    struct Store {
        refs: Tables,
        paths: BTreeSet<String>,
        cache: RwLock<Identities>,
    }

    impl Store {
        fn tables(&self, objects: &Path) -> BTreeMap<String, Table> {
            self.refs
                .tables
                .iter()
                .map(|(name, reference)| (name.clone(), Table::open(objects, reference).unwrap()))
                .collect()
        }

        /// Publishes `manifest` over this store's records.
        fn publish(&mut self, objects: &Path, manifest: &Manifest, retained: &BTreeSet<String>) {
            let tables = self.tables(objects);
            let old = Records::open(RECORDS, &tables);
            let update = update(
                objects,
                old.as_ref(),
                &self.paths,
                manifest,
                &self.cache,
                retained,
            )
            .unwrap();
            let previous = (!self.refs.tables.is_empty()).then_some(objects);
            for table in RECORDS.tables() {
                let reference = crate::segment::publish(
                    objects,
                    table,
                    previous,
                    self.refs.tables.get(table).unwrap_or(&TableRef::default()),
                    &update.delta.owners[table],
                    update.delta.rows[table].clone(),
                )
                .unwrap();
                self.refs.tables.insert(table.to_owned(), reference);
            }
            self.paths = update.paths;
            *self.cache.write().unwrap() = identities(manifest);
        }

        fn hydrate(&self, objects: &Path, header: &Manifest) -> io::Result<Manifest> {
            let tables = self.tables(objects);
            hydrate(
                objects,
                header,
                Records::open(RECORDS, &tables).as_ref(),
                &self.cache,
            )
        }

        fn selected(
            &self,
            objects: &Path,
            header: &Manifest,
            paths: &[&str],
        ) -> io::Result<graph_search_core::ports::ExtractionFacts> {
            let tables = self.tables(objects);
            selected(
                objects,
                header,
                Records::open(RECORDS, &tables).as_ref(),
                &self.cache,
                &paths.iter().map(|path| (*path).to_owned()).collect(),
            )
        }
    }

    fn fixture() -> Manifest {
        let mut manifest = Manifest::new(5, 1);
        for n in 0..8 {
            manifest.entries.insert(
                format!("{n}.rs"),
                FileEntry {
                    size: 42,
                    mtime_ns: 7,
                    content_hash: format!("hash-{n}"),
                    parser_version: 5,
                    schema_version: 1,
                    quarantine: None,
                    extraction: Some(
                        Extraction {
                            references: vec![ReferenceFact::file_level(
                                EdgeKind::Calls,
                                format!("{n}{}", "x".repeat(8192)),
                                1,
                            )],
                            ..Extraction::default()
                        }
                        .into(),
                    ),
                },
            );
        }
        manifest
    }

    fn packs(objects: &Path) -> BTreeSet<std::ffi::OsString> {
        std::fs::read_dir(objects.join(LAYOUT.directory))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }

    #[test]
    fn selected_facts_validate_fingerprints_and_keep_only_weak_identities() {
        let root = tempfile::tempdir().unwrap();
        let manifest = fixture();
        let mut store = Store::default();
        store.publish(root.path(), &manifest, &BTreeSet::new());
        let header = manifest.header();
        let first = store
            .selected(root.path(), &header, &["0.rs", "absent"])
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(
            first["0.rs"],
            *manifest.entries["0.rs"].extraction.as_ref().unwrap()
        );
        let identity = first["0.rs"].downgrade();
        assert!(first["0.rs"].matches_identity(&store.cache.read().unwrap()["0.rs"].1));
        let mut wrong = header.clone();
        wrong.entries.get_mut("0.rs").unwrap().content_hash = "wrong".into();
        assert!(store.selected(root.path(), &wrong, &["0.rs"]).is_err());
        drop(manifest);
        drop(first);
        assert!(
            identity.upgrade().is_none(),
            "selection cache must not retain raw facts"
        );
    }

    #[test]
    fn changed_facts_and_metadata_are_rewritten_and_unchanged_records_stay_packed() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::default();
        let old = fixture();
        store.publish(root.path(), &old, &BTreeSet::new());
        assert_eq!(store.hydrate(root.path(), &old.header()).unwrap(), old);
        let before = packs(root.path());
        // Same content hash, different facts: the record must not be reused.
        let mut changed = old.clone();
        changed
            .entries
            .get_mut("0.rs")
            .unwrap()
            .extraction
            .as_mut()
            .unwrap()
            .references
            .clear();
        // A metadata-only edit changes the complete record too.
        changed.entries.get_mut("1.rs").unwrap().mtime_ns += 1;
        // A publisher that did not hydrate compares encodings, not identities.
        let decoded: Manifest =
            serde_json::from_slice(&serde_json::to_vec(&changed).unwrap()).unwrap();
        store.cache.write().unwrap().clear();
        store.publish(root.path(), &decoded, &BTreeSet::new());
        assert_eq!(
            store.hydrate(root.path(), &changed.header()).unwrap(),
            changed
        );
        assert!(
            before.is_subset(&packs(root.path())),
            "six unchanged records keep the old pack live"
        );
    }

    #[test]
    fn retained_records_survive_and_removed_facts_do_not_resurrect() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::default();
        let full = fixture();
        store.publish(root.path(), &full, &BTreeSet::new());
        // A header-only publish that retains every record but 0.rs's.
        let mut header = full.header();
        let retained: BTreeSet<String> = header
            .entries
            .keys()
            .filter(|path| *path != "0.rs")
            .cloned()
            .collect();
        header.entries.remove("0.rs");
        store.publish(root.path(), &header, &retained);
        let mut expected = full.clone();
        expected.entries.remove("0.rs");
        assert_eq!(store.hydrate(root.path(), &header).unwrap(), expected);
        // Without retention, absent values drop their records.
        store.publish(root.path(), &header, &BTreeSet::new());
        assert!(store.paths.is_empty());
        assert_eq!(store.hydrate(root.path(), &header).unwrap(), header);
    }

    #[test]
    fn corrupt_packs_and_foreign_facts_fail_when_read() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::default();
        let manifest = fixture();
        store.publish(root.path(), &manifest, &BTreeSet::new());
        let header = manifest.header();
        let mut missing_owner = header.clone();
        missing_owner.entries.remove("0.rs");
        assert!(store.hydrate(root.path(), &missing_owner).is_err());
        assert!(
            store.hydrate(root.path(), &manifest).is_err(),
            "mixed embedded and packed facts are ambiguous"
        );
        for pack in packs(root.path()) {
            std::fs::write(root.path().join(LAYOUT.directory).join(pack), b"corrupt").unwrap();
        }
        assert!(store.hydrate(root.path(), &header).is_err());
        assert!(store.selected(root.path(), &header, &["0.rs"]).is_err());
    }
}
