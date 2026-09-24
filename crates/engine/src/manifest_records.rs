//! Cold extraction facts share the native pack primitive; manifest.json is the
//! small freshness header. Packed records retain their complete per-file entry
//! so hydration can verify that facts belong to that exact fingerprint.

use crate::source_records::{Index, Layout};
use graph_search_types::manifest::{FileEntry, Manifest};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

pub(crate) const FILE: &str = "extractions.json";
pub(crate) const LAYOUT: Layout = Layout {
    index: FILE,
    directory: "extraction-records",
    pack_bytes: crate::source_records::PACK_BYTES,
};

type Identities = BTreeMap<
    String,
    (
        FileEntry,
        std::sync::Weak<graph_search_types::extraction::Extraction>,
    ),
>;
/// Only initial byte verification and the writer may construct this cache.
/// Its descriptor cannot be replaced independently of the pinned generation.
pub(crate) struct Verified(Index, std::sync::RwLock<Identities>);

impl Verified {
    pub(crate) fn paths(&self) -> BTreeSet<String> {
        self.0.paths().map(str::to_owned).collect()
    }
}

fn identities(manifest: &Manifest) -> Identities {
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

/// The index is verified against CURRENT by the caller and against the
/// manifest here. Record bytes are verified by their own hashes when they are
/// read, so opening the index never reads a pack.
pub(crate) fn prepare_verified(
    bytes: &[u8],
    header: &Manifest,
    _dir: &Path,
) -> io::Result<Verified> {
    let index = prepare(bytes, header)?;
    Ok(Verified(index, std::sync::RwLock::new(BTreeMap::new())))
}

fn prepare(bytes: &[u8], header: &Manifest) -> io::Result<Index> {
    let index = Index::decode_packed(bytes)?;
    if header
        .entries
        .values()
        .any(|entry| entry.extraction.is_some())
        || index.paths().any(|path| !header.entries.contains_key(path))
    {
        return Err(io::Error::other(
            "extraction index does not match manifest header",
        ));
    }
    Ok(index)
}

pub(crate) fn hydrate(dir: &Path, header: &Manifest, index: &Verified) -> io::Result<Manifest> {
    // Opening the index read no pack, so every record hash is checked here.
    let manifest = hydrate_records(header, index.0.load(dir, LAYOUT)?)?;
    *index
        .1
        .write()
        .map_err(|_| io::Error::other("extraction identity lock poisoned"))? =
        identities(&manifest);
    Ok(manifest)
}

/// Read only selected facts from the pinned, verified generation descriptor.
/// Identity additions do not evict other selected reads or retain fact payloads.
pub(crate) fn selected(
    dir: &Path,
    header: &Manifest,
    index: &Verified,
    paths: &BTreeSet<String>,
) -> io::Result<graph_search_core::ports::ExtractionFacts> {
    let facts = validate_records(header, index.0.load_selected_verified(dir, LAYOUT, paths)?)?;
    let mut identities = index
        .1
        .write()
        .map_err(|_| io::Error::other("extraction identity lock poisoned"))?;
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

/// Standalone sidecar compatibility helper. Published stores use the index
/// authenticated and cached during generation selection instead.
pub(crate) fn load(dir: &Path, header: Manifest) -> io::Result<Manifest> {
    match std::fs::read(dir.join(FILE)) {
        Ok(bytes) => hydrate_records(&header, prepare(&bytes, &header)?.load(dir, LAYOUT)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(header),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) fn save(
    dir: &Path,
    manifest: &Manifest,
    previous: &Path,
    old: Option<&Verified>,
) -> io::Result<Verified> {
    save_retaining(dir, manifest, previous, old, &BTreeSet::new())
}

pub(crate) fn save_retaining(
    dir: &Path,
    manifest: &Manifest,
    previous: &Path,
    old: Option<&Verified>,
    retained: &BTreeSet<String>,
) -> io::Result<Verified> {
    let files: BTreeMap<_, _> = manifest
        .entries
        .iter()
        .filter(|(_, entry)| entry.extraction.is_some())
        .map(|(path, entry)| (path.clone(), entry))
        .collect();
    let cached = old
        .map(|index| {
            index
                .1
                .read()
                .map_err(|_| io::Error::other("extraction identity lock poisoned"))
        })
        .transpose()?;
    let index = crate::source_records::save_records_retaining(
        dir,
        LAYOUT,
        &files,
        previous,
        old.map(|v| &v.0),
        retained,
        |path, entry| {
            if cached
                .as_ref()
                .and_then(|cache| cache.get(path))
                .is_some_and(|(header, identity)| {
                    header == &entry.header()
                        && entry
                            .extraction
                            .as_ref()
                            .is_some_and(|facts| facts.matches_identity(identity))
                })
            {
                return Ok(true);
            }
            old.map_or(Ok(false), |index| index.0.matches(path, entry))
        },
    )?;
    // New slices are hashed by the writer; retained references came from Verified.
    Ok(Verified(
        index,
        std::sync::RwLock::new(identities(manifest)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::{
        EdgeKind,
        extraction::{Extraction, ReferenceFact},
    };

    fn persist_fixture(
        dir: &Path,
        manifest: &Manifest,
        previous: &Path,
        old: Option<&Verified>,
    ) -> Verified {
        let index = save(dir, manifest, previous, old).unwrap();
        crate::sidecar::save_manifest(dir, &manifest.header()).unwrap();
        index
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

    #[test]
    fn selected_facts_validate_fingerprints_and_keep_only_weak_identities() {
        let root = tempfile::tempdir().unwrap();
        let manifest = fixture();
        let index = persist_fixture(root.path(), &manifest, Path::new(""), None);
        let header = manifest.header();
        let first = selected(
            root.path(),
            &header,
            &index,
            &BTreeSet::from(["0.rs".into(), "absent".into()]),
        )
        .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(
            first["0.rs"],
            *manifest.entries["0.rs"].extraction.as_ref().unwrap()
        );
        let identity = first["0.rs"].downgrade();
        let second = selected(
            root.path(),
            &header,
            &index,
            &BTreeSet::from(["1.rs".into()]),
        )
        .unwrap();
        assert_eq!(second.len(), 1);
        assert!(first["0.rs"].matches_identity(&index.1.read().unwrap()["0.rs"].1));
        let mut wrong = header.clone();
        wrong.entries.get_mut("0.rs").unwrap().content_hash = "wrong".into();
        assert!(
            selected(
                root.path(),
                &wrong,
                &index,
                &BTreeSet::from(["0.rs".into()])
            )
            .is_err()
        );
        drop(first);
        assert!(
            identity.upgrade().is_none(),
            "selection cache must not retain raw facts"
        );
    }

    #[test]
    fn selection_does_not_decode_unrequested_records_in_the_same_pack() {
        let root = tempfile::tempdir().unwrap();
        let manifest = fixture();
        let records = BTreeMap::from([
            (
                "0.rs".into(),
                serde_json::to_value(&manifest.entries["0.rs"]).unwrap(),
            ),
            ("1.rs".into(), serde_json::json!("not a FileEntry")),
        ]);
        let index = crate::source_records::save_records(
            root.path(),
            LAYOUT,
            &records,
            Path::new(""),
            None,
            |_, _| Ok(false),
        )
        .unwrap();
        let header = manifest.header();
        let bytes = serde_json::to_vec(&index).unwrap();
        let descriptor: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            descriptor["records"]["0.rs"]["pack"],
            descriptor["records"]["1.rs"]["pack"]
        );
        let verified = prepare_verified(&bytes, &header, root.path()).unwrap();
        let selected = selected(
            root.path(),
            &header,
            &verified,
            &BTreeSet::from(["0.rs".into()]),
        )
        .unwrap();
        assert_eq!(selected.len(), 1);
        assert!(hydrate(root.path(), &header, &verified).is_err());
        assert!(
            super::selected(
                root.path(),
                &header,
                &verified,
                &BTreeSet::from(["1.rs".into()])
            )
            .is_err()
        );
    }

    #[test]
    fn header_is_small_and_same_hash_fact_changes_cannot_reuse_stale_records() {
        let old_dir = tempfile::tempdir().unwrap();
        let new_dir = tempfile::tempdir().unwrap();
        let old = fixture();
        let old_index = persist_fixture(old_dir.path(), &old, Path::new(""), None);
        let header_bytes =
            std::fs::read(old_dir.path().join(crate::sidecar::MANIFEST_FILE)).unwrap();
        assert!(header_bytes.len() < 4096);
        let header: Manifest = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header, old.header());
        assert_eq!(hydrate(old_dir.path(), &header, &old_index).unwrap(), old);
        assert_eq!(
            crate::sidecar::load_manifest(old_dir.path()).unwrap(),
            Some(old.clone())
        );
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
        let new_index = persist_fixture(new_dir.path(), &changed, old_dir.path(), Some(&old_index));
        assert_eq!(
            hydrate(new_dir.path(), &changed.header(), &new_index).unwrap(),
            changed
        );
        let shared = std::fs::read_dir(old_dir.path().join(LAYOUT.directory))
            .unwrap()
            .next()
            .unwrap()
            .unwrap();
        let next_pack = new_dir
            .path()
            .join(LAYOUT.directory)
            .join(shared.file_name());
        assert!(
            next_pack.exists(),
            "seven unchanged records keep the old pack live"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let a = shared.metadata().unwrap();
            let b = std::fs::metadata(next_pack).unwrap();
            assert_eq!((a.dev(), a.ino()), (b.dev(), b.ino()));
        }
        drop(old_dir);
        assert_eq!(
            hydrate(new_dir.path(), &changed.header(), &new_index).unwrap(),
            changed
        );
    }

    #[test]
    fn descriptor_is_pinned_and_missing_or_corrupt_packs_and_foreign_facts_fail() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = fixture();
        let index = persist_fixture(dir.path(), &manifest, Path::new(""), None);
        let header = manifest.header();
        let bytes = std::fs::read(dir.path().join(FILE)).unwrap();
        let verified = prepare_verified(&bytes, &header, dir.path()).unwrap();
        assert_eq!(hydrate(dir.path(), &header, &verified).unwrap(), manifest);
        // A record whose committed hash does not match its bytes opens, and
        // fails when it is read: records are verified on use.
        let mut bad_hash: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        bad_hash["records"]["0.rs"]["hash"] = serde_json::json!("0".repeat(64));
        let bad =
            prepare_verified(&serde_json::to_vec(&bad_hash).unwrap(), &header, dir.path()).unwrap();
        assert!(hydrate(dir.path(), &header, &bad).is_err());
        assert!(
            prepare(&bytes, &manifest).is_err(),
            "mixed embedded and packed facts are ambiguous"
        );
        let mut missing_owner = header.clone();
        missing_owner.entries.remove("0.rs");
        assert!(prepare(&bytes, &missing_owner).is_err());
        let mut wrong_hash = header.clone();
        wrong_hash.entries.get_mut("0.rs").unwrap().content_hash = "different".into();
        assert!(hydrate(dir.path(), &wrong_hash, &index).is_err());
        std::fs::write(dir.path().join(FILE), b"invalid index").unwrap();
        assert_eq!(hydrate(dir.path(), &header, &index).unwrap(), manifest);
        assert!(load(dir.path(), header.clone()).is_err());
        let pack = std::fs::read_dir(dir.path().join(LAYOUT.directory))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(&pack, b"corrupt").unwrap();
        assert!(index.0.verify(dir.path(), LAYOUT).is_err());
        assert!(hydrate(dir.path(), &header, &index).is_err());
        std::fs::remove_file(pack).unwrap();
        assert!(index.0.verify(dir.path(), LAYOUT).is_err());
    }

    #[test]
    fn identity_cache_replacement_metadata_and_unique_owner_mutation_are_safe() {
        let old_dir = tempfile::tempdir().unwrap();
        let next_dir = tempfile::tempdir().unwrap();
        let mut original = fixture();
        let index = persist_fixture(old_dir.path(), &original, Path::new(""), None);
        // Sole strong owner: mutable access must detach the writer's weak identity.
        original
            .entries
            .get_mut("0.rs")
            .unwrap()
            .extraction
            .as_mut()
            .unwrap()
            .references
            .clear();
        // A metadata-only edit must not reuse the previous complete FileEntry bytes.
        original.entries.get_mut("1.rs").unwrap().mtime_ns += 1;
        let next = persist_fixture(next_dir.path(), &original, old_dir.path(), Some(&index));
        assert_eq!(
            hydrate(next_dir.path(), &original.header(), &next).unwrap(),
            original
        );

        let first = hydrate(old_dir.path(), &fixture().header(), &index).unwrap();
        let second = hydrate(old_dir.path(), &fixture().header(), &index).unwrap();
        let first_identity = first.entries["0.rs"]
            .extraction
            .as_ref()
            .unwrap()
            .downgrade();
        assert!(
            !second.entries["0.rs"]
                .extraction
                .as_ref()
                .unwrap()
                .matches_identity(&first_identity)
        );
        // Publishing the first hydration after a second hydration replaced the
        // cache safely uses byte comparison, as does independent deserialization.
        let decoded: Manifest =
            serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
        for manifest in [&first, &second, &decoded] {
            let destination = tempfile::tempdir().unwrap();
            let saved = persist_fixture(destination.path(), manifest, old_dir.path(), Some(&index));
            assert_eq!(
                hydrate(destination.path(), &manifest.header(), &saved).unwrap(),
                *manifest
            );
        }
        let identity = second.entries["0.rs"]
            .extraction
            .as_ref()
            .unwrap()
            .downgrade();
        drop(second);
        assert!(
            identity.upgrade().is_none(),
            "cache must not retain extraction payloads"
        );
    }

    #[test]
    fn legacy_maps_migrate_and_removing_cached_facts_does_not_resurrect_them() {
        let old_dir = tempfile::tempdir().unwrap();
        let new_dir = tempfile::tempdir().unwrap();
        let empty_dir = tempfile::tempdir().unwrap();
        let old = fixture();
        crate::sidecar::save_manifest(old_dir.path(), &old).unwrap();
        assert_eq!(
            crate::sidecar::load_manifest(old_dir.path()).unwrap(),
            Some(old.clone())
        );
        let index = persist_fixture(new_dir.path(), &old, old_dir.path(), None);
        assert_eq!(
            crate::sidecar::load_manifest(new_dir.path()).unwrap(),
            Some(old.clone())
        );
        let header = old.header();
        let empty = persist_fixture(empty_dir.path(), &header, new_dir.path(), Some(&index));
        assert_eq!(empty.0.paths().count(), 0);
        assert_eq!(hydrate(empty_dir.path(), &header, &empty).unwrap(), header);
    }
}
