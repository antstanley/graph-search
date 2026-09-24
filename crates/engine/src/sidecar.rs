//! The small files of a generation: the manifest header (`SPEC.md` §6.3) and
//! the source-record index. The manifest is one JSON document, written
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
    let manifest = serde_json::from_str(&text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    // A generation keeps its packs in the store's object directory; a bare
    // directory keeps them beside its index.
    let packs = if store_dir
        .join(crate::manifest_records::LAYOUT.directory)
        .is_dir()
    {
        store_dir.to_path_buf()
    } else {
        objects_of(store_dir)
    };
    crate::manifest_records::load(store_dir, &packs, manifest).map(Some)
}

/// Writes the manifest atomically.
///
/// # Errors
/// When the store directory is unwritable.
pub fn save_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    prepare_manifest(store_dir, manifest)?;
    crate::generation::sync_dir(store_dir)
}

/// Sync the file; the unpublished-generation owner must sync its directory
/// after all artifact renames and before publishing CURRENT.
pub(crate) fn prepare_manifest(store_dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    std::fs::create_dir_all(store_dir)?;
    let target = store_dir.join(MANIFEST_FILE);
    let tmp = store_dir.join(format!("{MANIFEST_FILE}.tmp"));
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&tmp, &target)
}

/// Source retrieval facts published and checksummed with the graph generation.
pub const SOURCE_FILE: &str = "source-units.json";
/// The store-level object directory of a generation directory
/// (`<store>/generations/<id>`).
fn objects_of(generation: &Path) -> std::path::PathBuf {
    generation.parent().and_then(Path::parent).map_or_else(
        || generation.to_path_buf(),
        |store| store.join(crate::generation::OBJECTS),
    )
}

/// Reads the native source facts a generation directory commits.
/// # Errors
/// On unreadable or malformed data.
pub fn load_sources(
    generation: &Path,
) -> std::io::Result<std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>
{
    let bytes = std::fs::read(generation.join(SOURCE_FILE))?;
    crate::source_records::Index::decode_packed(&bytes)?.load(
        &objects_of(generation),
        crate::source_records::SOURCE_LAYOUT,
    )
}

/// Replaces the native source facts a generation directory commits: new packs
/// in the store's object directory and a fresh index. The caller commits the
/// index artifact.
/// # Errors
/// On serialization, write or sync failure.
pub fn save_sources(
    generation: &Path,
    sources: &std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>,
) -> std::io::Result<()> {
    let objects = objects_of(generation);
    let index = crate::source_records::save_packs(
        &objects,
        crate::source_records::SOURCE_LAYOUT,
        sources,
        &objects,
        None,
        &std::collections::BTreeSet::new(),
        |_, _| Ok(false),
    )?;
    crate::source_records::write_index(generation, crate::source_records::SOURCE_LAYOUT, &index)
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
