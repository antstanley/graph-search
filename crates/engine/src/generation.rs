//! Native publication of graph generations (format 10). Unpublished
//! directories are never opened by readers; CURRENT is the sole visibility
//! boundary. A generation commits small index artifacts by hash; the packs and
//! posting segments they reference are committed transitively by their own
//! content hashes and verified when read.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const CURRENT: &str = "CURRENT";
/// Format 10: per-file shards and posting tables replace the Grafeo graph file
/// and the whole-workspace sidecars (`research/16-proportional-sync.md`).
/// Format 11: packs and segments live in one store-level object directory
/// that every generation references, instead of being linked into each.
/// Format 12: dependency records live in shards and their reverse maps in
/// posting tables, replacing the whole-workspace dependency index artifact.
/// Format 13: shards also index symbols by name, qualified name and
/// structure, so a sync reads only the symbols its references name.
/// Format 14: the shard, source and dependency pack indexes are posting
/// tables (`record_tables`), so a publish writes nothing per unchanged file.
const FORMAT: u32 = 14;
/// The store-level directory of content-addressed packs and segments.
pub(crate) const OBJECTS: &str = "objects";
/// Every object a generation references, relative to [`OBJECTS`]: what the
/// collector keeps alive for as long as the generation exists.
pub(crate) const OBJECT_LIST: &str = "objects.json";
/// An empty file every generation holds: readers pin it with a shared lock and
/// reclamation takes it exclusively.
pub(crate) const LEASE: &str = "lease";
/// The posting tables' segment lists ([`crate::segment::Tables`]).
pub(crate) const TABLES: &str = "tables.json";
/// Cached generation totals.
pub(crate) const SUMMARY: &str = "summary.json";
/// Artifacts every generation commits.
const REQUIRED: [&str; 2] = [SUMMARY, TABLES];
/// The name the dependency-record layout carries; since format 14 its index
/// is a set of posting tables, not an artifact.
pub(crate) const DEPENDENCY_RECORDS: &str = "dependencies.json";
/// Artifacts a generation may commit.
const OPTIONAL: [&str; 2] = [crate::sidecar::MANIFEST_FILE, crate::manifest_records::FILE];
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
    /// Whether every file has a dependency record, so repair can use them.
    #[serde(default)]
    pub(crate) dependencies: bool,
}

/// The artifacts CURRENT commits for one generation. Selection verifies the
/// small header artifacts; every other artifact is verified against its
/// committed hash when it is first read, before any of its bytes are used.
pub(crate) struct Committed {
    dir: PathBuf,
    files: BTreeMap<String, String>,
}

impl Committed {
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
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

/// The published generation, or `None` when nothing is published or the
/// published generation predates the current format (it is rebuilt, never
/// migrated).
pub(crate) fn current(root: &Path) -> io::Result<Option<Selected>> {
    match read_current(root, |bytes| select(root, bytes)) {
        Err(error) if error.kind() == io::ErrorKind::Unsupported => Ok(None),
        other => other,
    }
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
    if !valid_id(&pointer.id) || pointer.format > FORMAT || pointer.format == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid generation pointer",
        ));
    }
    if pointer.format < FORMAT {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "generation predates the current format and must be rebuilt",
        ));
    }
    let dir = root.join("generations").join(pointer.id);
    let lease = pin(&dir)?;
    if REQUIRED
        .iter()
        .any(|name| !pointer.files.contains_key(*name))
        || pointer
            .files
            .keys()
            .any(|name| !REQUIRED.contains(&name.as_str()) && !OPTIONAL.contains(&name.as_str()))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "incomplete generation descriptor",
        ));
    }
    let has_extractions = pointer.files.contains_key(crate::manifest_records::FILE);
    let has_manifest = pointer.files.contains_key(crate::sidecar::MANIFEST_FILE);
    if has_extractions != has_manifest {
        return Err(io::Error::other(
            "incomplete extraction generation descriptor",
        ));
    }
    for name in OPTIONAL {
        if !pointer.files.contains_key(name) && dir.join(name).exists() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("uncommitted artifact in generation: {name}"),
            ));
        }
    }
    let committed = Committed {
        dir,
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

/// Pin an immutable generation by its lease file. No new file or write
/// permission is needed by readers. Keep the handle alive for as long as lazy
/// records can be opened from this generation's paths.
pub(crate) fn pin(dir: &Path) -> io::Result<std::fs::File> {
    let file = std::fs::File::open(dir.join(LEASE))?;
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

/// Write and flush a temporary file before the visibility-changing rename.
/// The caller must flush the parent directory after a successful rename; the
/// publication barrier makes both durable (see [`crate::durable`]).
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    crate::durable::flush(&file)?;
    std::fs::rename(tmp, path)
}

/// Commits `bytes` as `path` after `dir`'s artifacts: the pointer's own
/// bytes are flushed first, so one barrier makes both it and everything it
/// commits durable before the rename can expose it. The caller syncs the
/// rename's directory in full.
fn commit(dir: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    crate::durable::flush(&file)?;
    crate::durable::barrier(dir)?;
    std::fs::rename(tmp, path)
}

/// Commits every written artifact of `dir` by hash and makes it CURRENT.
/// Returns the committed hashes, so the publisher can adopt the generation
/// without reading it back.
pub(crate) fn prepare_pointer(root: &Path, dir: &Path) -> io::Result<BTreeMap<String, String>> {
    let id = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::other("invalid generation name"))?;
    let mut files = BTreeMap::new();
    for name in REQUIRED.iter().chain(OPTIONAL.iter()) {
        match std::fs::read(dir.join(name)) {
            Ok(bytes) => {
                files.insert(
                    (*name).to_owned(),
                    graph_search_core::hash::content_hash(&bytes),
                );
            }
            Err(error) if OPTIONAL.contains(name) && error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let pointer = serde_json::to_vec(&Pointer {
        format: FORMAT,
        id: id.to_owned(),
        files: files.clone(),
    })
    .map_err(io::Error::other)?;
    // Everything the pointer commits is durable before the pointer is.
    commit(dir, &root.join(CURRENT), &pointer)?;
    Ok(files)
}

/// A committed descriptor for a generation this process just published.
pub(crate) fn committed(dir: &Path, files: BTreeMap<String, String>) -> Committed {
    Committed {
        dir: dir.to_path_buf(),
        files,
    }
}

/// Removes every object no remaining generation lists. Runs under the writer
/// lock after reclamation; a generation directory without an object list (an
/// unfinished publication) makes it keep everything until that directory goes.
pub(crate) fn collect(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root.join("generations")) else {
        return;
    };
    let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(valid_id) {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path().join(OBJECT_LIST)) else {
            return;
        };
        let Ok(names) = serde_json::from_slice::<Vec<String>>(&bytes) else {
            return;
        };
        live.extend(names);
    }
    let Ok(families) = std::fs::read_dir(root.join(OBJECTS)) else {
        return;
    };
    for family in families.flatten() {
        let Some(family_name) = family.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(objects) = std::fs::read_dir(family.path()) else {
            continue;
        };
        for object in objects.flatten() {
            let Some(name) = object.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !live.contains(&format!("{family_name}/{name}")) {
                let _ = std::fs::remove_file(object.path());
            }
        }
    }
}

/// Flushes a directory's entries; durable after the publication barrier.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    crate::durable::flush_dir(dir)
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
            match std::fs::File::open(path.join(LEASE)) {
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
            crate::NativeStore::open(root.path(), &crate::StoreOptions::default()).unwrap();
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
