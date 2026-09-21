//! Explicit retention of cold extraction records during a header-only update.
use crate::{Error, Result, ports::GraphStore};
use graph_search_types::{Manifest, WriteBatch};
use std::collections::BTreeSet;

/// Proof request for retaining existing cached facts, separate from absent facts.
/// `None` extraction values in an ordinary publication still remove the cache.
#[derive(Clone, Debug)]
pub struct FactRetention {
    /// Generation observed when the update was prepared, if the adapter has one.
    pub generation: Option<String>,
    /// Exact previously observed header, including representation identities.
    pub previous: Manifest,
    /// Paths whose existing extraction values must survive this publication.
    pub paths: BTreeSet<String>,
}

impl FactRetention {
    /// Validates graph ownership as well as the old and new manifest identities.
    /// # Errors
    /// When a retained owner is being replaced or removed, or identity differs.
    pub fn validate_batch(
        &self,
        store: &(impl GraphStore + ?Sized),
        batch: &WriteBatch,
    ) -> Result<()> {
        self.validate(store, &batch.manifest)?;
        if batch
            .upserts
            .iter()
            .any(|file| self.paths.contains(&file.file.path))
            || batch
                .removed_files
                .iter()
                .any(|path| self.paths.contains(path))
        {
            return Err(Error::Store(
                "retained extraction owner is being replaced".into(),
            ));
        }
        Ok(())
    }

    /// Rejects stale requests and changes that would invalidate retained facts.
    /// Timestamp-only changes are allowed; their packed fingerprints need rewriting.
    /// # Errors
    /// On a stale generation, unavailable cache owner, or incompatible new entry.
    pub fn validate(&self, store: &(impl GraphStore + ?Sized), next: &Manifest) -> Result<()> {
        if store.generation()? != self.generation
            || store.manifest_header()?.as_ref() != Some(&self.previous)
            || self
                .previous
                .entries
                .values()
                .any(|entry| entry.extraction.is_some())
            || next.versions() != self.previous.versions()
            || next.policy_fingerprint != self.previous.policy_fingerprint
        {
            return Err(Error::Store("stale extraction retention request".into()));
        }
        for path in &self.paths {
            let valid = self
                .previous
                .get(path)
                .zip(next.get(path))
                .is_some_and(|(old, new)| {
                    let mut allowed = old.header();
                    allowed.mtime_ns = new.mtime_ns;
                    new.extraction.is_none() && new == &allowed
                });
            if !valid {
                return Err(Error::Store(format!(
                    "incompatible retained extraction: {path}"
                )));
            }
        }
        Ok(())
    }
}
