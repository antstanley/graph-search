//! Raw source captures shared across phases of one explore request.

use crate::{Result, work::WorkBudget};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

#[derive(Clone)]
pub(crate) enum ReadOutcome {
    Bytes(Arc<Vec<u8>>),
    Unavailable,
    BudgetExceeded,
}

#[derive(Clone, PartialEq, Eq)]
struct Fingerprint {
    size: u64,
    modified: SystemTime,
}

fn fingerprint(path: &Path) -> Option<Fingerprint> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(Fingerprint {
        size: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

#[derive(Clone)]
struct Captured {
    outcome: ReadOutcome,
    limit: u64,
    fingerprint: Option<Fingerprint>,
}

#[derive(Default)]
pub(crate) struct Captures(BTreeMap<PathBuf, Captured>);

/// `limit` includes the caller's overflow sentinel. A previously truncated read
/// cannot satisfy a larger request. Never reopen it or present its prefix as EOF.
pub(crate) fn read(path: &Path, limit: u64, work: &mut WorkBudget) -> Result<ReadOutcome> {
    work.check()?;
    if let Some(captured) = work
        .source_captures
        .as_ref()
        .and_then(|cache| cache.0.get(path))
    {
        if fingerprint(path) != captured.fingerprint {
            return Err(crate::Error::IncompleteVerification(format!(
                "source changed during request: {}",
                path.display()
            )));
        }
        return Ok(match &captured.outcome {
            ReadOutcome::Bytes(bytes)
                if bytes.len() as u64 >= captured.limit && limit > captured.limit =>
            {
                return Err(crate::Error::IncompleteVerification(format!(
                    "captured source prefix cannot satisfy a larger read: {}",
                    path.display()
                )));
            }
            ReadOutcome::Bytes(bytes) if bytes.len() as u64 > limit => ReadOutcome::Bytes(
                Arc::new(bytes[..usize::try_from(limit).unwrap_or(bytes.len())].to_vec()),
            ),
            outcome => outcome.clone(),
        });
    }
    if !work.source_file()? {
        return Ok(ReadOutcome::BudgetExceeded);
    }
    let before = work.source_captures.as_ref().map(|_| fingerprint(path));
    let bytes = match std::fs::File::open(path) {
        Ok(file) => crate::source::read_checked(file, limit, work)?,
        Err(_) => None,
    };
    let outcome = if work.source_over_budget() {
        ReadOutcome::BudgetExceeded
    } else {
        bytes.map_or(ReadOutcome::Unavailable, |bytes| {
            ReadOutcome::Bytes(bytes.into())
        })
    };
    if let Some(cache) = work.source_captures.as_mut() {
        let after = fingerprint(path);
        if before.as_ref() != Some(&after)
            || (matches!(outcome, ReadOutcome::Bytes(_)) && after.is_none())
        {
            return Err(crate::Error::IncompleteVerification(format!(
                "source changed while being captured: {}",
                path.display()
            )));
        }
        // Entries are bounded by admitted open attempts; retained bytes by the
        // shared source allowance. Denied opens above never allocate entries.
        cache.0.insert(
            path.to_path_buf(),
            Captured {
                outcome: outcome.clone(),
                limit,
                fingerprint: after,
            },
        );
    }
    work.check()?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::{CancellationToken, WorkLimits};

    fn bytes(outcome: ReadOutcome) -> Arc<Vec<u8>> {
        match outcome {
            ReadOutcome::Bytes(bytes) => bytes,
            _ => panic!("expected captured bytes"),
        }
    }

    #[test]
    fn repeated_capture_reuses_raw_bytes_after_exact_allowance_is_spent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        std::fs::write(&path, b"abcd").unwrap();
        let mut work = WorkBudget::new(WorkLimits {
            source_files: 1,
            source_bytes: 4,
            ..WorkLimits::default()
        });
        work.enable_source_capture();
        let first = bytes(read(&path, 5, &mut work).unwrap());
        let second = bytes(read(&path, 5, &mut work).unwrap());
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(&*first, b"abcd");
        assert_eq!(work.source_report(), (1, 4));
        assert!(work.report().2.is_empty());
        let mut sources = crate::source::SourceCache::new(4, 4);
        assert_eq!(
            sources
                .read_with_work(root.path(), "a.rs", &mut work)
                .unwrap(),
            Some("abcd")
        );
        assert_eq!(work.source_report(), (1, 4));
        assert!(work.report().2.is_empty());
    }

    #[test]
    fn prefixes_and_evidence_limits_cannot_be_mistaken_for_complete_source() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        std::fs::write(&path, b"abcdef").unwrap();
        let mut work = WorkBudget::new(WorkLimits::default());
        work.enable_source_capture();
        assert_eq!(&*bytes(read(&path, 4, &mut work).unwrap()), b"abcd");
        assert_eq!(&*bytes(read(&path, 3, &mut work).unwrap()), b"abc");
        assert!(matches!(
            read(&path, 8, &mut work),
            Err(crate::Error::IncompleteVerification(_))
        ));
        assert_eq!(work.source_report(), (1, 4));

        let mut work = WorkBudget::new(WorkLimits::default());
        work.enable_source_capture();
        assert_eq!(&*bytes(read(&path, 8, &mut work).unwrap()), b"abcdef");
        let mut sources = crate::source::SourceCache::new(3, 10);
        assert!(
            sources
                .read_with_work(root.path(), "a.rs", &mut work)
                .unwrap()
                .is_none()
        );
        assert_eq!(work.source_report(), (1, 6));
        let mut coverage = graph_search_types::coverage::Coverage::default();
        sources.add_read_coverage(&mut coverage);
        assert_eq!(coverage.source_budget_exceeded_files, 1);
        assert!(
            coverage
                .truncations
                .iter()
                .any(|t| t.kind == graph_search_types::TruncationKind::SourceBytes)
        );
    }

    #[test]
    fn drift_creation_and_cancellation_fail_closed_on_reuse() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        let mut work = WorkBudget::new(WorkLimits::default());
        work.enable_source_capture();
        assert!(matches!(
            read(&path, 100, &mut work).unwrap(),
            ReadOutcome::Unavailable
        ));
        std::fs::write(&path, b"abcd").unwrap();
        assert!(matches!(
            read(&path, 100, &mut work),
            Err(crate::Error::IncompleteVerification(_))
        ));
        assert_eq!(work.source_report(), (1, 0));

        let token = CancellationToken::default();
        let mut work = WorkBudget::new(WorkLimits {
            cancellation: Some(token.clone()),
            ..WorkLimits::default()
        });
        work.enable_source_capture();
        assert_eq!(&*bytes(read(&path, 100, &mut work).unwrap()), b"abcd");
        std::fs::write(&path, b"changed size").unwrap();
        assert!(matches!(
            read(&path, 100, &mut work),
            Err(crate::Error::IncompleteVerification(_))
        ));
        token.cancel();
        assert!(matches!(
            read(&path, 100, &mut work),
            Err(crate::Error::QueryCancelled)
        ));
        assert_eq!(work.source_report(), (1, 4));
    }

    #[test]
    fn raw_binary_and_invalid_encoding_remain_verified_but_not_text() {
        let root = tempfile::tempdir().unwrap();
        for (path, raw) in [("binary", &[0, 1, 2][..]), ("invalid", &[255, 254][..])] {
            std::fs::write(root.path().join(path), raw).unwrap();
            let mut work = WorkBudget::new(WorkLimits::default());
            work.enable_source_capture();
            assert_eq!(
                &*bytes(read(&root.path().join(path), 100, &mut work).unwrap()),
                raw
            );
            let mut sources = crate::source::SourceCache::new(100, 100);
            assert!(
                sources
                    .read_with_work(root.path(), path, &mut work)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                sources.hash(path),
                Some(crate::hash::content_hash(raw).as_str())
            );
            assert_eq!(work.source_report(), (1, raw.len() as u64));
            let mut coverage = graph_search_types::coverage::Coverage::default();
            sources.add_read_coverage(&mut coverage);
            assert_eq!(coverage.binary_files + coverage.invalid_utf8_files, 1);
        }
    }

    #[test]
    fn denied_opens_do_not_allocate_and_uncached_requests_keep_accounting() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.rs");
        std::fs::write(&path, b"abcd").unwrap();
        let mut work = WorkBudget::new(WorkLimits {
            source_files: 0,
            ..WorkLimits::default()
        });
        work.enable_source_capture();
        for n in 0..100 {
            assert!(matches!(
                read(&root.path().join(n.to_string()), 100, &mut work).unwrap(),
                ReadOutcome::BudgetExceeded
            ));
        }
        assert!(work.source_captures.as_ref().unwrap().0.is_empty());
        let mut work = WorkBudget::new(WorkLimits::default());
        for _ in 0..2 {
            assert_eq!(&*bytes(read(&path, 100, &mut work).unwrap()), b"abcd");
        }
        assert_eq!(work.source_report(), (2, 8));
        assert!(work.source_captures.is_none());
    }
}
