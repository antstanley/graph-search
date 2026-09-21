//! Metadata-only capture of module-candidate presence. This adapter qualifies
//! absent source records; it never reads excluded source contents or admits them.
use graph_search_core::typescript_files::Availability;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Maximum distinct namespace metadata observations in one capture.
pub const MAX_OBSERVATIONS: usize = 4096;

/// Observed namespace entry kind, independent of source admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The path was absent or obstructed by a non-directory prefix.
    Absent,
    /// A regular file exists; admission remains the index's decision.
    File,
    /// A directory exists.
    Directory,
    /// A symlink or other opaque entry exists.
    Opaque,
    /// Metadata could not be inspected reliably.
    Unknown,
}

impl Kind {
    /// Stable name for observation reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::File => "file",
            Self::Directory => "directory",
            Self::Opaque => "opaque",
            Self::Unknown => "unknown",
        }
    }
}

/// A bounded namespace capture for use while constructing a resolution context.
/// Observed opaque prefixes stop traversal. Call `validate` before accepting its
/// decisions; this is not an atomic filesystem snapshot or persisted freshness
/// integration. The caller still owns the admitted generation's source identity.
pub struct Capture {
    root: PathBuf,
    observations: BTreeMap<String, Kind>,
    limit: usize,
}

impl Capture {
    /// Capture the workspace root without reading source content.
    ///
    /// # Errors
    /// The root cannot be canonicalized or is not a directory.
    pub fn new(root: &Path) -> std::io::Result<Self> {
        Self::with_limit(root, MAX_OBSERVATIONS)
    }

    /// Create a capture with a lower observation limit (clamped to the hard cap).
    ///
    /// # Errors
    /// The root cannot be canonicalized or is not a directory.
    pub fn with_limit(root: &Path, limit: usize) -> std::io::Result<Self> {
        let root = root.canonicalize()?;
        if kind(&root) != Kind::Directory {
            return Err(std::io::Error::other(
                "module presence root is not a directory",
            ));
        }
        Ok(Self {
            root,
            observations: BTreeMap::from([(String::new(), Kind::Directory)]),
            limit: limit.clamp(1, MAX_OBSERVATIONS),
        })
    }

    /// Qualify a normalized workspace-relative candidate that is not admitted.
    /// Existing files/opaque paths are unavailable, not absent. An observed file
    /// in an intermediate component proves that a descendant cannot be a file.
    ///
    /// # Errors
    /// Invalid path, observation budget exhausted, or metadata unavailable.
    pub fn classify(&mut self, path: &str) -> Result<Availability, &'static str> {
        if path.len() > 4096 || path.starts_with('/') || path.contains(['\\', ':', '\0']) {
            return Err("ts_module_presence_path_invalid");
        }
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return Ok(Availability::Absent);
        }
        let parts: Vec<_> = path.split('/').collect();
        if parts.iter().any(|part| matches!(*part, "" | "." | "..")) {
            return Err("ts_module_presence_path_invalid");
        }
        let mut prefix = String::new();
        let mut parts = parts.into_iter().peekable();
        while let Some(part) = parts.next() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            match self.observe(&prefix)? {
                Kind::Absent => return Ok(Availability::Absent),
                Kind::File if parts.peek().is_some() => return Ok(Availability::Absent),
                Kind::File | Kind::Opaque => return Ok(Availability::Unavailable),
                Kind::Directory => {}
                Kind::Unknown => return Err("ts_module_presence_io"),
            }
        }
        Ok(Availability::Absent)
    }

    /// Captured namespace footprint, including missing parent paths.
    #[must_use]
    pub fn observations(&self) -> &BTreeMap<String, Kind> {
        &self.observations
    }

    /// Recheck the observed footprint in prefix order. Changed prefixes stop the
    /// check before descendants, including a directory replaced by an opaque path.
    /// Contents/mtime of regular files are deliberately not presence identities.
    ///
    /// # Errors
    /// A captured observation changed or metadata is unavailable. Callers must
    /// discard a positive resolution instead of publishing a mixed observation.
    pub fn validate(&self) -> Result<(), &'static str> {
        for (path, expected) in &self.observations {
            let actual = kind(&self.root.join(path));
            if actual == Kind::Unknown || *expected == Kind::Unknown {
                return Err("ts_module_presence_io");
            }
            if actual != *expected {
                return Err("ts_module_presence_changed");
            }
        }
        Ok(())
    }

    fn observe(&mut self, path: &str) -> Result<Kind, &'static str> {
        if let Some(kind) = self.observations.get(path) {
            return Ok(*kind);
        }
        if self.observations.len() >= self.limit {
            return Err("ts_module_presence_limit");
        }
        let kind = kind(&self.root.join(path));
        self.observations.insert(path.into(), kind);
        Ok(kind)
    }
}

fn kind(path: &Path) -> Kind {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Kind::File,
        Ok(metadata) if metadata.is_dir() => Kind::Directory,
        Ok(_) => Kind::Opaque,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Kind::Absent
        }
        Err(_) => Kind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_directories_missing_paths_and_obstructions_have_distinct_presence() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/opaque.ts"), [0xff, 0xfe, 0]).unwrap();
        std::fs::write(root.path().join("plain"), b"not a directory").unwrap();
        let mut capture = Capture::new(root.path()).unwrap();
        assert_eq!(
            capture.classify("src/opaque.ts"),
            Ok(Availability::Unavailable)
        );
        assert_eq!(capture.classify("src"), Ok(Availability::Absent));
        assert_eq!(capture.classify("src/missing.ts"), Ok(Availability::Absent));
        assert_eq!(capture.classify("plain/child.ts"), Ok(Availability::Absent));
        assert!(!capture.observations().contains_key("plain/child.ts"));
        assert_eq!(capture.validate(), Ok(()));
    }

    #[test]
    fn changed_namespace_observations_invalidate_but_file_contents_are_not_presence() {
        let root = tempfile::tempdir().unwrap();
        let mut missing = Capture::new(root.path()).unwrap();
        assert_eq!(missing.classify("new.ts"), Ok(Availability::Absent));
        std::fs::write(root.path().join("new.ts"), b"first").unwrap();
        assert_eq!(missing.validate(), Err("ts_module_presence_changed"));
        let mut existing = Capture::new(root.path()).unwrap();
        assert_eq!(existing.classify("new.ts"), Ok(Availability::Unavailable));
        std::fs::write(root.path().join("new.ts"), b"different content").unwrap();
        assert_eq!(existing.validate(), Ok(()));
        std::fs::remove_file(root.path().join("new.ts")).unwrap();
        assert_eq!(existing.validate(), Err("ts_module_presence_changed"));
    }

    #[test]
    fn missing_parent_creation_limits_and_invalid_paths_never_become_false_absence() {
        let root = tempfile::tempdir().unwrap();
        let mut capture = Capture::new(root.path()).unwrap();
        assert_eq!(
            capture.classify("missing/child.ts"),
            Ok(Availability::Absent)
        );
        assert_eq!(capture.observations().len(), 2);
        std::fs::create_dir(root.path().join("missing")).unwrap();
        assert_eq!(capture.validate(), Err("ts_module_presence_changed"));
        let mut limited = Capture::with_limit(root.path(), 1).unwrap();
        assert_eq!(limited.classify("new.ts"), Err("ts_module_presence_limit"));
        assert_eq!(limited.observations().len(), 1);
        for path in ["../escape.ts", "/absolute", "a//b", "a/../b", "a\\b"] {
            assert_eq!(
                limited.classify(path),
                Err("ts_module_presence_path_invalid")
            );
        }
        assert_eq!(limited.observations().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn observed_symlink_prefixes_stop_traversal_and_replacements_invalidate() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("outside.ts"), b"not admitted").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let mut capture = Capture::new(root.path()).unwrap();
        assert_eq!(
            capture.classify("link/outside.ts"),
            Ok(Availability::Unavailable)
        );
        assert_eq!(capture.observations()["link"], Kind::Opaque);
        assert!(!capture.observations().contains_key("link/outside.ts"));
        assert_eq!(capture.validate(), Ok(()));
        std::fs::create_dir(root.path().join("dir")).unwrap();
        let mut replaced = Capture::new(root.path()).unwrap();
        assert_eq!(
            replaced.classify("dir/outside.ts"),
            Ok(Availability::Absent)
        );
        std::fs::remove_dir(root.path().join("dir")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("dir")).unwrap();
        assert_eq!(replaced.validate(), Err("ts_module_presence_changed"));
    }
}
