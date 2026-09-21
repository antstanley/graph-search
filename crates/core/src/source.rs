//! Request-local source reads. Indexed snippets are emitted only from bytes
//! matching their file projection; live body hits carry their own fingerprint.

use crate::ports::GraphSnapshot;
use graph_search_types::context::{SourceIdentity, SourceVerification};
use graph_search_types::{Node, NodeId};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

struct CachedSource {
    text: Option<String>,
    hash: Option<String>,
    status: SourceVerification,
}

/// Bounded, request-local source blobs shared by candidates and evidence.
pub struct SourceCache {
    files: BTreeMap<String, CachedSource>,
    remaining: u64,
    total: u64,
    total_exceeded: bool,
    per_file_exceeded: bool,
    per_file: u64,
}

impl SourceCache {
    /// Creates a cache bounded by total bytes and bytes per file.
    #[must_use]
    pub fn new(total_bytes: u64, per_file: u64) -> Self {
        Self {
            files: BTreeMap::new(),
            remaining: total_bytes,
            total: total_bytes,
            total_exceeded: false,
            per_file_exceeded: false,
            per_file,
        }
    }

    /// Reads each path at most once, retaining raw UTF-8 bytes for this request.
    pub fn read(&mut self, root: &Path, path: &str) -> Option<&str> {
        let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits {
            source_bytes: usize::try_from(self.total).unwrap_or(usize::MAX),
            ..crate::work::WorkLimits::default()
        });
        self.read_with_work(root, path, &mut work).ok().flatten()
    }

    /// Reads a source once under cache limits and the shared request work budget.
    /// Cached paths perform no new read and consume no additional source allowance.
    /// # Errors
    /// On cancellation or deadline.
    pub fn read_with_work(
        &mut self,
        root: &Path,
        path: &str,
        work: &mut crate::work::WorkBudget,
    ) -> crate::Result<Option<&str>> {
        work.check()?;
        if !self.files.contains_key(path) {
            let cap = self.remaining.min(self.per_file);
            let total_limited = self.remaining <= self.per_file;
            let cached = if cap == 0 {
                self.total_exceeded |= total_limited;
                self.per_file_exceeded |= !total_limited;
                CachedSource {
                    text: None,
                    hash: None,
                    status: SourceVerification::BudgetExceeded,
                }
            } else {
                let before = work.source_report().1;
                let read =
                    crate::source_capture::read(&root.join(path), cap.saturating_add(1), work)?;
                let retained = match &read {
                    crate::source_capture::ReadOutcome::Bytes(bytes) => bytes.len() as u64,
                    _ => 0,
                };
                // Charge this cache for bytes captured elsewhere as well as bytes
                // physically consumed by a failed read in this phase.
                let consumed = retained.max(work.source_report().1.saturating_sub(before));
                self.remaining = self.remaining.saturating_sub(consumed);
                if consumed > cap {
                    self.total_exceeded |= total_limited;
                    self.per_file_exceeded |= !total_limited;
                }
                match read {
                    crate::source_capture::ReadOutcome::BudgetExceeded => CachedSource {
                        text: None,
                        hash: None,
                        status: SourceVerification::BudgetExceeded,
                    },
                    crate::source_capture::ReadOutcome::Unavailable => CachedSource {
                        text: None,
                        hash: None,
                        status: SourceVerification::Unavailable,
                    },
                    crate::source_capture::ReadOutcome::Bytes(bytes) => {
                        if bytes.len() as u64 > cap {
                            self.total_exceeded |= total_limited;
                            self.per_file_exceeded |= !total_limited;
                            CachedSource {
                                text: None,
                                hash: None,
                                status: SourceVerification::BudgetExceeded,
                            }
                        } else {
                            let hash = crate::hash::content_hash(&bytes);
                            let (text, status) = if bytes.iter().take(8192).any(|b| *b == 0) {
                                (None, SourceVerification::Binary)
                            } else {
                                match String::from_utf8(
                                    std::sync::Arc::try_unwrap(bytes)
                                        .unwrap_or_else(|bytes| (*bytes).clone()),
                                ) {
                                    Ok(text) => (Some(text), SourceVerification::Live),
                                    Err(_) => (None, SourceVerification::InvalidEncoding),
                                }
                            };
                            CachedSource {
                                text,
                                hash: Some(hash),
                                status,
                            }
                        }
                    }
                }
            };
            self.files.insert(path.to_owned(), cached);
        }
        work.check()?;
        Ok(self
            .files
            .get(path)
            .and_then(|source| source.text.as_deref()))
    }

    /// Adds source-read exclusions observed in this request, counting each
    /// cached path once regardless of how many symbols requested its bytes.
    pub fn add_read_coverage(&self, coverage: &mut graph_search_types::coverage::Coverage) {
        for source in self.files.values() {
            match source.status {
                SourceVerification::BudgetExceeded => {
                    coverage.source_budget_exceeded_files =
                        coverage.source_budget_exceeded_files.saturating_add(1);
                }
                SourceVerification::Unavailable => {
                    coverage.source_read_errors = coverage.source_read_errors.saturating_add(1);
                }
                SourceVerification::Binary => {
                    coverage.binary_files = coverage.binary_files.saturating_add(1);
                }
                SourceVerification::InvalidEncoding => {
                    coverage.invalid_utf8_files = coverage.invalid_utf8_files.saturating_add(1);
                }
                _ => {}
            }
        }
        for (exceeded, kind, cap, message) in [
            (
                self.total_exceeded,
                graph_search_types::TruncationKind::SourceBytes,
                self.total,
                "request source reads reached their total byte cap",
            ),
            (
                self.per_file_exceeded,
                graph_search_types::TruncationKind::SourceFileBytes,
                self.per_file,
                "source reads reached the per-file byte cap",
            ),
        ] {
            if exceeded
                && !coverage
                    .truncations
                    .iter()
                    .any(|t| t.kind == kind && t.cap == cap)
            {
                coverage
                    .truncations
                    .push(graph_search_types::Truncation::new(kind, cap, message));
            }
        }
    }

    /// The previously captured source bytes, without reading the live tree.
    #[must_use]
    pub fn text(&self, path: &str) -> Option<&str> {
        self.files.get(path)?.text.as_deref()
    }

    /// The raw fingerprint of a successfully read source file.
    #[must_use]
    pub fn hash(&self, path: &str) -> Option<&str> {
        self.files.get(path)?.hash.as_deref()
    }

    /// Describes observed bytes against the file projection in this snapshot.
    ///
    /// # Errors
    /// Propagates snapshot read failures.
    pub fn identity(
        &self,
        snapshot: &dyn GraphSnapshot,
        path: &str,
    ) -> crate::Result<SourceIdentity> {
        let indexed_hash = snapshot
            .node_by_id(&NodeId::file(path))?
            .and_then(|node| node.content_hash);
        let Some(source) = self.files.get(path) else {
            return Ok(SourceIdentity {
                indexed_hash,
                ..SourceIdentity::default()
            });
        };
        let verification = match source.status {
            SourceVerification::Live => match (&indexed_hash, &source.hash) {
                (Some(expected), Some(observed)) if expected == observed => {
                    SourceVerification::Verified
                }
                (Some(_), Some(_)) => SourceVerification::Mismatch,
                _ => SourceVerification::Live,
            },
            status => status,
        };
        Ok(SourceIdentity {
            indexed_hash,
            observed_hash: source.hash.clone(),
            verification,
        })
    }

    /// Whether cached bytes can safely accompany this node's coordinates.
    /// Live file hits explicitly carry the hash used when their span was chosen.
    ///
    /// # Errors
    /// Propagates snapshot read failures.
    pub fn matches(&self, snapshot: &dyn GraphSnapshot, node: &Node) -> crate::Result<bool> {
        if node.is_file() {
            return Ok(node
                .content_hash
                .as_deref()
                .is_some_and(|hash| self.hash(&node.path) == Some(hash)));
        }
        Ok(self.identity(snapshot, &node.path)?.verification == SourceVerification::Verified)
    }
}

// None means an I/O failure; cancellation/deadline remain explicit query errors.
pub(crate) fn read_checked(
    reader: impl std::io::Read,
    limit: u64,
    work: &mut crate::work::WorkBudget,
) -> crate::Result<Option<Vec<u8>>> {
    let mut reader = reader.take(limit.min(work.source_read_limit()));
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        work.check()?;
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                work.charge_source_bytes(count);
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Ok(None),
        }
    }
    work.check()?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::GraphStore;

    #[test]
    fn request_cache_keeps_one_source_version() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        std::fs::write(&path, "original").unwrap();
        let mut cache = SourceCache::new(100, 100);
        assert_eq!(cache.read(root.path(), "a.rs"), Some("original"));
        let hash = cache.hash("a.rs").unwrap().to_owned();
        std::fs::write(&path, "replacement").unwrap();
        assert_eq!(cache.read(root.path(), "a.rs"), Some("original"));
        assert_eq!(cache.hash("a.rs"), Some(hash.as_str()));
    }

    #[test]
    fn exhausted_budget_is_visible_and_does_not_return_partial_text() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.rs"), "long source").unwrap();
        let mut cache = SourceCache::new(4, 100);
        assert!(cache.read(root.path(), "a.rs").is_none());
        let store = crate::memory::MemoryStore::new();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(
            cache
                .identity(snapshot.as_ref(), "a.rs")
                .unwrap()
                .verification,
            SourceVerification::BudgetExceeded
        );
    }

    #[test]
    fn read_budget_losses_are_reported_without_selected_results() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("large.rs"), "long source").unwrap();
        for (total, per_file, expected) in [
            (4, 100, graph_search_types::TruncationKind::SourceBytes),
            (100, 4, graph_search_types::TruncationKind::SourceFileBytes),
            (0, 100, graph_search_types::TruncationKind::SourceBytes),
            (100, 0, graph_search_types::TruncationKind::SourceFileBytes),
        ] {
            let mut cache = SourceCache::new(total, per_file);
            assert!(cache.read(root.path(), "large.rs").is_none());
            assert!(cache.read(root.path(), "large.rs").is_none());
            let mut coverage = graph_search_types::coverage::Coverage::default();
            cache.add_read_coverage(&mut coverage);
            assert_eq!(coverage.source_budget_exceeded_files, 1);
            assert_eq!(coverage.truncations.len(), 1);
            assert_eq!(coverage.truncations[0].kind, expected);
            assert_eq!(coverage.truncations[0].cap, total.min(per_file));
        }
    }
}
