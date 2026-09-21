//! The files beside the graph store: the manifest and the dangling-reference
//! sidecar (`SPEC.md` §5.2, §6.3).
//!
//! Dangling references are kept, named, and counted — but an LPG edge needs a
//! target node, so they live in a JSON-lines sidecar the snapshot merges into
//! its reads. The manifest is one JSON document, written atomically
//! (temporary file, then rename) and only ever *after* a successful apply.

use graph_search_types::NodeId;
use graph_search_types::kind::EdgeKind;
use graph_search_types::manifest::Manifest;
use graph_search_types::node::Edge;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::Path;

/// The manifest file name inside the store directory.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The dangling-reference sidecar file name.
pub const DANGLING_FILE: &str = "dangling.jsonl";

/// One sidecar record.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DanglingRecord {
    from: String,
    kind: String,
    to_name: String,
    path: Option<String>,
    line: Option<u32>,
}

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
    crate::manifest_records::load(store_dir, manifest).map(Some)
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

/// Loads every dangling reference.
///
/// # Errors
/// When the sidecar exists but cannot be parsed.
pub fn load_dangling(store_dir: &Path) -> std::io::Result<Vec<Edge>> {
    let path = store_dir.join(DANGLING_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut edges = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let record: DanglingRecord = serde_json::from_str(line).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        let Some(kind) = EdgeKind::parse(&record.kind) else {
            continue;
        };
        let from = NodeId::new(record.from);
        edges.push(Edge::dangling(
            &from,
            kind,
            &record.to_name,
            record.path.as_deref(),
            record.line,
        ));
    }
    Ok(edges)
}

/// Rewrites the sidecar with `edges`, keeping the files in `keep_paths` and
/// dropping everything else (the replaced files' dangles are gone).
///
/// # Errors
/// When the store directory is unwritable.
pub fn save_dangling(
    store_dir: &Path,
    edges: &[Edge],
    keep_paths: &std::collections::BTreeSet<String>,
) -> std::io::Result<()> {
    let kept: Vec<_> = edges
        .iter()
        .filter(|edge| {
            edge.path
                .as_ref()
                .is_some_and(|path| keep_paths.contains(path))
        })
        .cloned()
        .collect();
    prepare_dangling(store_dir, &kept)?;
    crate::generation::sync_dir(store_dir)
}

/// Write the complete prepared edge set, including pathless references.
/// The generation owner already applied source ownership filtering and must sync
/// the directory before publishing CURRENT.
pub(crate) fn prepare_dangling(store_dir: &Path, edges: &[Edge]) -> std::io::Result<()> {
    std::fs::create_dir_all(store_dir)?;
    let target = store_dir.join(DANGLING_FILE);
    let tmp = store_dir.join(format!("{DANGLING_FILE}.tmp"));
    let file = std::fs::File::create(&tmp)?;
    let mut writer = std::io::BufWriter::new(file);
    for edge in edges {
        let record = DanglingRecord {
            from: edge.from.to_string(),
            kind: edge.kind.as_str().to_owned(),
            to_name: edge.to_name.clone(),
            path: edge.path.clone(),
            line: edge.line,
        };
        serde_json::to_writer(&mut writer, &record).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    std::fs::rename(&tmp, &target)
}

/// Source retrieval facts published and checksummed with the graph generation.
pub const SOURCE_FILE: &str = "source-units.json";
/// Source-owned references, stored separately from aggregate adjacency.
pub const OCCURRENCE_FILE: &str = "occurrences.json";

/// Reads source occurrences; missing legacy artifacts have unknown occurrence coverage.
/// # Errors
/// On unreadable or malformed data.
pub fn load_occurrences(
    dir: &Path,
) -> std::io::Result<
    std::collections::BTreeMap<String, graph_search_types::occurrence::OccurrenceFile>,
> {
    let bytes = match std::fs::read(dir.join(OCCURRENCE_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(std::collections::BTreeMap::new());
        }
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// Publishes and syncs occurrences inside an unpublished generation.
/// # Errors
/// On serialization, write or sync failure.
pub fn save_occurrences(
    dir: &Path,
    files: &std::collections::BTreeMap<String, graph_search_types::occurrence::OccurrenceFile>,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(files).map_err(std::io::Error::other)?;
    crate::generation::replace(&dir.join(OCCURRENCE_FILE), &bytes)
}

/// Reads native source facts. Missing legacy sidecars have no source coverage.
/// # Errors
/// On unreadable or malformed data.
pub fn load_sources(
    dir: &Path,
) -> std::io::Result<std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>
{
    crate::source_records::load(dir)
}

/// Writes native source facts inside an unpublished generation.
/// # Errors
/// On serialization, write or sync failure.
pub fn save_sources(
    dir: &Path,
    sources: &std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(sources).map_err(std::io::Error::other)?;
    crate::generation::replace(&dir.join(SOURCE_FILE), &bytes)
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

    #[test]
    fn dangling_references_round_trip_and_filter() {
        let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let from = NodeId::symbol(
            "src/a.rs",
            graph_search_types::kind::NodeKind::Function,
            "f",
            None,
        );
        let keep = Edge::dangling(&from, EdgeKind::Calls, "ghost", Some("src/a.rs"), Some(3));
        let drop = Edge::dangling(&from, EdgeKind::Calls, "gone", Some("src/old.rs"), Some(1));
        let mut all = vec![keep.clone(), drop];
        let keep_paths = std::collections::BTreeSet::from([String::from("src/a.rs")]);
        save_dangling(tmp.path(), &all, &keep_paths).unwrap_or_else(|e| panic!("save: {e}"));
        all = load_dangling(tmp.path()).unwrap_or_else(|e| panic!("load: {e}"));
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].to_name, "ghost");
        assert_eq!(all[0].line, Some(3));
    }
}
