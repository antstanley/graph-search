//! Native publication of complete graph generations. Unpublished directories
//! are never opened by readers; CURRENT is the sole visibility boundary.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const CURRENT: &str = "CURRENT";
pub(crate) const GRAPH: &str = "graph.grafeo";
const FORMAT: u32 = 9;
/// One zstd frame of the JSON dependency index.
pub(crate) const DEPENDENCIES: &str = "dependencies.json.zst";
/// Cached generation totals (format 9+, with [`crate::edge_counts::FILE`]).
pub(crate) const SUMMARY: &str = "summary.json";
static SERIAL: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Pointer {
    format: u32,
    id: String,
    files: BTreeMap<String, String>,
}

/// Exact totals of one generation, published with it so that `status` and
/// query contexts never load the graph or its facts to report them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Summary {
    pub(crate) counts: graph_search_types::result::StoreCounts,
    pub(crate) source: graph_search_core::units::SourceCoverage,
}

/// The artifacts CURRENT commits for one generation. Selection verifies the
/// small header artifacts; every other artifact is verified against its
/// committed hash when it is first read, before any of its bytes are used.
pub(crate) struct Committed {
    dir: PathBuf,
    format: u32,
    files: BTreeMap<String, String>,
}

impl Committed {
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) const fn format(&self) -> u32 {
        self.format
    }

    /// The committed bytes of `name`, verified against CURRENT; `None` when the
    /// descriptor does not commit that artifact.
    pub(crate) fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(expected) = self.files.get(name) else {
            return Ok(None);
        };
        let bytes = std::fs::read(self.dir.join(name))?;
        if graph_search_core::hash::content_hash(&bytes) != *expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("generation checksum mismatch: {name}"),
            ));
        }
        Ok(Some(bytes))
    }
}

pub(crate) struct Selected {
    pub(crate) committed: Committed,
    pub(crate) manifest: Option<graph_search_types::manifest::Manifest>,
    pub(crate) summary: Option<Summary>,
    pub(crate) lease: std::fs::File,
}

pub(crate) fn current(root: &Path) -> io::Result<Option<Selected>> {
    read_current(root, |bytes| select(root, bytes))
}

fn read_current(
    root: &Path,
    mut select: impl FnMut(&[u8]) -> io::Result<Selected>,
) -> io::Result<Option<Selected>> {
    for _ in 0..8 {
        let bytes = match std::fs::read(root.join(CURRENT)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match select(&bytes) {
            Ok(selected) => return Ok(Some(selected)),
            Err(error) => {
                // A reader may observe CURRENT immediately before two publications
                // retire that directory. Retry only a demonstrably changed pointer;
                // stable corruption or unsupported file locking remains an error.
                if std::fs::read(root.join(CURRENT))? == bytes {
                    return Err(error);
                }
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "generation changed repeatedly while opening; retry",
    ))
}

/// Admits one generation: the descriptor must be complete, the directory is
/// pinned, and the manifest header and summary are verified and parsed. The
/// graph, facts and records are verified lazily through [`Committed::read`];
/// the lease keeps their paths alive for as long as the store holds it.
fn select(root: &Path, bytes: &[u8]) -> io::Result<Selected> {
    let pointer: Pointer = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if !(1..=FORMAT).contains(&pointer.format) || !valid_id(&pointer.id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid generation pointer",
        ));
    }
    let dir = root.join("generations").join(pointer.id);
    let lease = pin(&dir)?;
    if !pointer.files.contains_key(GRAPH)
        || !pointer.files.contains_key(crate::sidecar::DANGLING_FILE)
        || (pointer.format >= 2 && !pointer.files.contains_key(crate::sidecar::SOURCE_FILE))
        || (pointer.format >= 3 && !pointer.files.contains_key(crate::sidecar::OCCURRENCE_FILE))
        || (pointer.format >= 9) != pointer.files.contains_key(SUMMARY)
        || (pointer.format >= 9) != pointer.files.contains_key(crate::edge_counts::FILE)
        || pointer.files.keys().any(|name| {
            ![
                GRAPH,
                crate::sidecar::DANGLING_FILE,
                crate::sidecar::MANIFEST_FILE,
                crate::sidecar::SOURCE_FILE,
                crate::sidecar::OCCURRENCE_FILE,
                crate::manifest_records::FILE,
                DEPENDENCIES,
                SUMMARY,
                crate::edge_counts::FILE,
            ]
            .contains(&name.as_str())
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete generation descriptor",
        ));
    }
    let has_extractions = pointer.files.contains_key(crate::manifest_records::FILE);
    let has_manifest = pointer.files.contains_key(crate::sidecar::MANIFEST_FILE);
    if (has_extractions && (pointer.format < 6 || !has_manifest))
        || (pointer.format >= 6 && has_manifest && !has_extractions)
    {
        return Err(io::Error::other(
            "incomplete extraction generation descriptor",
        ));
    }
    let has_dependencies = pointer.files.contains_key(DEPENDENCIES);
    if (has_dependencies && (pointer.format < 7 || !has_manifest))
        || (pointer.format >= 7 && has_manifest && !has_dependencies)
    {
        return Err(io::Error::other(
            "incomplete dependency generation descriptor",
        ));
    }
    for name in [
        crate::sidecar::MANIFEST_FILE,
        crate::sidecar::SOURCE_FILE,
        crate::sidecar::OCCURRENCE_FILE,
        crate::manifest_records::FILE,
        DEPENDENCIES,
        SUMMARY,
        crate::edge_counts::FILE,
    ] {
        if !pointer.files.contains_key(name) && dir.join(name).exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("uncommitted artifact in generation: {name}"),
            ));
        }
    }
    let committed = Committed {
        dir,
        format: pointer.format,
        files: pointer.files,
    };
    let manifest = committed
        .read(crate::sidecar::MANIFEST_FILE)?
        .map(|bytes| {
            serde_json::from_slice::<graph_search_types::manifest::Manifest>(&bytes)
                .map_err(io::Error::other)
        })
        .transpose()?
        .as_ref()
        .map(graph_search_types::manifest::Manifest::header);
    let summary = committed
        .read(SUMMARY)?
        .map(|bytes| serde_json::from_slice::<Summary>(&bytes).map_err(io::Error::other))
        .transpose()?;
    Ok(Selected {
        committed,
        manifest,
        summary,
        lease,
    })
}

/// Pin an immutable generation using an existing mandatory sidecar. No new file
/// or write permission is needed by readers. Keep the distinct handle alive for
/// as long as lazy records can be opened from this generation's paths.
pub(crate) fn pin(dir: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::File::open(dir.join(crate::sidecar::DANGLING_FILE))?;
    file.try_lock_shared().map_err(io::Error::from)?;
    Ok(file)
}

fn valid_id(id: &str) -> bool {
    id.starts_with("g-")
        && id.len() > 2
        && id[2..].bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
}

pub(crate) fn allocate(root: &Path) -> io::Result<PathBuf> {
    let parent = root.join("generations");
    std::fs::create_dir_all(&parent)?;
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    for _ in 0..32 {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let dir = parent.join(format!("g-{epoch:x}-{:x}-{serial:x}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot allocate graph generation",
    ))
}

/// Write and sync a temporary file before the visibility-changing rename.
/// The caller must sync the parent directory after a successful rename.
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(tmp, path)
}

pub(crate) fn prepare_pointer(root: &Path, dir: &Path) -> io::Result<()> {
    let id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::other("invalid generation name"))?;
    let mut files = BTreeMap::new();
    for name in [
        GRAPH,
        crate::sidecar::DANGLING_FILE,
        crate::sidecar::MANIFEST_FILE,
        crate::sidecar::SOURCE_FILE,
        crate::sidecar::OCCURRENCE_FILE,
        crate::manifest_records::FILE,
        DEPENDENCIES,
        SUMMARY,
        crate::edge_counts::FILE,
    ] {
        match std::fs::read(dir.join(name)) {
            Ok(bytes) => {
                files.insert(
                    name.to_owned(),
                    graph_search_core::hash::content_hash(&bytes),
                );
            }
            Err(error)
                if [
                    crate::sidecar::MANIFEST_FILE,
                    crate::manifest_records::FILE,
                    DEPENDENCIES,
                ]
                .contains(&name)
                    && error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    // Record hashes come from the synced writer, which verifies reused bytes.
    // Re-reading every blob here repeats that work; readers verify on open.
    let pointer = serde_json::to_vec(&Pointer {
        format: FORMAT,
        id: id.to_owned(),
        files,
    })
    .map_err(io::Error::other)?;
    replace(&root.join(CURRENT), &pointer)
}

pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Called only after publishing under the writer lock. Retain current, previous
/// and every live reader's generation. A later publication retries skipped cleanup.
pub(crate) fn reclaim(root: &Path, current: &Path, previous: &Path) {
    let Ok(entries) = std::fs::read_dir(root.join("generations")) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != current
            && path != previous
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.file_name().to_str().is_some_and(valid_id)
        {
            match std::fs::File::open(path.join(crate::sidecar::DANGLING_FILE)) {
                Ok(file) => {
                    if file.try_lock().is_ok() {
                        // Hold the exclusive lease until all paths are removed.
                        let _ = std::fs::remove_dir_all(&path);
                    }
                }
                // An unfinished generation without the mandatory sidecar cannot
                // have an admitted reader. Never delete on other open/lock errors.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let _ = std::fs::remove_dir_all(&path);
                }
                Err(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_core::ports::GraphStore;

    #[test]
    fn selection_retries_a_retired_pointer_and_pins_before_loading_the_store() {
        let root = tempfile::tempdir().unwrap();
        let mut writer =
            crate::GrafeoStore::open(root.path(), &crate::StoreOptions::default()).unwrap();
        writer
            .publish(graph_search_core::conformance::fixture_batch())
            .unwrap();
        let retired = root
            .path()
            .join("generations")
            .join(writer.generation().unwrap().unwrap());
        let mut selections = 0;
        let selected = read_current(root.path(), |bytes| {
            selections += 1;
            if selections == 1 {
                for _ in 0..2 {
                    writer
                        .publish(graph_search_core::conformance::fixture_batch())
                        .unwrap();
                }
                assert!(!retired.exists());
            }
            select(root.path(), bytes)
        })
        .unwrap()
        .unwrap();
        assert_eq!(selections, 2);
        assert_eq!(
            selected.committed.dir().file_name().unwrap().to_str(),
            writer.generation().unwrap().as_deref()
        );
        for _ in 0..3 {
            writer
                .publish(graph_search_core::conformance::fixture_batch())
                .unwrap();
        }
        assert!(
            selected.committed.dir().exists(),
            "selection already owns a reader lease"
        );
        let retained = selected.committed.dir().to_path_buf();
        drop(selected);
        writer
            .publish(graph_search_core::conformance::fixture_batch())
            .unwrap();
        assert!(!retained.exists());
    }

    #[test]
    fn stable_errors_are_not_retried_and_pointer_churn_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(CURRENT), b"first").unwrap();
        let mut selections = 0u32;
        let error = read_current(root.path(), |_| {
            selections += 1;
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "stable corruption",
            ))
        })
        .err()
        .unwrap();
        assert_eq!(selections, 1);
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let error = read_current(root.path(), |_| {
            selections += 1;
            std::fs::write(root.path().join(CURRENT), selections.to_string()).unwrap();
            Err(io::Error::new(io::ErrorKind::NotFound, "retired"))
        })
        .err()
        .unwrap();
        assert_eq!(selections, 9);
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    }
}
