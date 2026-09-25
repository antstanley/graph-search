//! The small files of a generation: the manifest header (`SPEC.md` §6.3), and
//! access to a generation's source records. The manifest is one JSON document, written
//! atomically (temporary file, then rename) and only ever *after* a successful
//! apply.

use graph_search_types::manifest::Manifest;
use std::io::Write as _;
use std::path::Path;

/// The manifest file name inside the store directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Reads the manifest, when one is committed.
///
/// # Errors
/// When the file exists but cannot be parsed.
pub fn load_manifest(store_dir: &Path) -> std::io::Result<Option<Manifest>> {
    let path = store_dir.join(MANIFEST_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    // A header's extraction values live in the generation's record family.
    if manifest
        .entries
        .values()
        .any(|entry| entry.extraction.is_some())
        || !store_dir.join(crate::generation::TABLES).exists()
    {
        return Ok(Some(manifest));
    }
    let objects = objects_of(store_dir);
    let tables = open_tables(store_dir, &objects)?;
    crate::manifest_records::hydrate(
        &objects,
        &manifest,
        crate::record_tables::Records::open(crate::manifest_records::RECORDS, &tables).as_ref(),
        &std::sync::RwLock::default(),
    )
    .map(Some)
}

/// Writes the manifest atomically.
///
/// # Errors
/// When the store directory is unwritable.
pub fn save_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    prepare_manifest(store_dir, manifest)?;
    crate::generation::sync_dir(store_dir)?;
    crate::durable::barrier(store_dir)
}

/// Sync the file; the unpublished-generation owner must sync its directory
/// after all artifact renames and before publishing CURRENT.
pub(crate) fn prepare_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    std::fs::create_dir_all(store_dir)?;
    let target = store_dir.join(MANIFEST_FILE);
    let tmp = store_dir.join(format!("{MANIFEST_FILE}.tmp"));
    let text = serde_json::to_string(manifest)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    crate::durable::flush(&file)?;
    std::fs::rename(&tmp, &target)
}

/// The name source-record layouts carry; since format 14 the source index is
/// a set of posting tables in `tables.json`, not an artifact of its own.
pub const SOURCE_FILE: &str = "source-units.json";
/// The store-level object directory of a generation directory
/// (`<store>/generations/<id>`).
fn objects_of(generation: &Path) -> std::path::PathBuf {
    generation.parent().and_then(Path::parent).map_or_else(
        || generation.to_path_buf(),
        |store| store.join(crate::generation::OBJECTS),
    )
}

/// The table references a generation directory commits.
fn table_refs(generation: &Path) -> std::io::Result<crate::segment::Tables> {
    serde_json::from_slice(&std::fs::read(generation.join(crate::generation::TABLES))?)
        .map_err(std::io::Error::other)
}

/// A generation directory's opened tables.
fn open_tables(
    generation: &Path,
    objects: &Path,
) -> std::io::Result<std::collections::BTreeMap<String, crate::segment::Table>> {
    table_refs(generation)?
        .tables
        .iter()
        .map(|(name, reference)| {
            crate::segment::Table::open(objects, reference).map(|table| (name.clone(), table))
        })
        .collect()
}

/// Reads the native source facts a generation directory commits.
/// # Errors
/// On unreadable or malformed data.
pub fn load_sources(
    generation: &Path,
) -> std::io::Result<std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>
{
    let objects = objects_of(generation);
    let tables = open_tables(generation, &objects)?;
    match crate::record_tables::Records::open(crate::store::SOURCE_RECORDS, &tables) {
        Some(records) => records.load(&objects),
        None => Ok(std::collections::BTreeMap::new()),
    }
}

/// Replaces the native source facts a generation directory commits: new packs
/// and tables in the store's object directory and a rewritten `tables.json`.
/// The caller commits that artifact.
/// # Errors
/// On serialization, write or sync failure.
pub fn save_sources(
    generation: &Path,
    sources: &std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>,
) -> std::io::Result<()> {
    let objects = objects_of(generation);
    let family = crate::store::SOURCE_RECORDS;
    let delta = crate::record_tables::update(
        &objects,
        family,
        None,
        sources,
        &std::collections::BTreeSet::new(),
    )?;
    let mut refs = table_refs(generation)?;
    for table in family.tables() {
        let reference = crate::segment::publish(
            &objects,
            table,
            None,
            &crate::segment::TableRef::default(),
            delta
                .owners
                .get(table)
                .unwrap_or(&std::collections::BTreeSet::new()),
            delta.rows.get(table).cloned().unwrap_or_default(),
        )?;
        refs.tables.insert(table.to_owned(), reference);
    }
    crate::generation::replace(
        &generation.join(crate::generation::TABLES),
        &serde_json::to_vec(&refs).map_err(std::io::Error::other)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_round_trips_atomically() {
        let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let mut manifest = Manifest::new(1, 1);
        manifest.indexed_at_ms = 7;
        save_manifest(tmp.path(), &manifest).unwrap_or_else(|e| panic!("save: {e}"));
        let loaded = load_manifest(tmp.path()).unwrap_or_else(|e| panic!("load: {e}"));
        assert_eq!(loaded, Some(manifest));
    }
}
