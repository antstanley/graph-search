//! Finding P1.1: test-owned code follows other discovery hits unless the query
//! names it or asks about tests.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use graph_search::{Index, OpenOptions};
use graph_search_types::query::ExploreQuery;
use graph_search_types::retrieval::TestRanking;

fn fixture() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "src/lib.rs",
        "/// Reports a port failure.\npub fn port_failure(reason: &str) -> String { format!(\"port failure: {reason}\") }\n\
         pub fn open_port() -> u16 { 80 }\n\
         #[cfg(test)]\nmod checks {\n    use super::*;\n    #[test]\n    fn port_failure_port_failure_names_the_reason() {\n        assert!(port_failure(\"port failure\").contains(\"port failure\"));\n    }\n}\n",
    );
    write(
        "tests/port.rs",
        "#[test]\nfn port_failure_port_failure_is_reported() { let _ = demo::port_failure(\"port failure\"); }\n",
    );
    let index = Index::open(OpenOptions {
        root: dir.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    (dir, index)
}

/// `(name, test-owned)` for each returned symbol, best first.
fn ranked(index: &Index, text: &str, policy: TestRanking) -> Vec<(String, bool)> {
    let mut query = ExploreQuery::new(text);
    query.retrieval.tests = policy;
    index
        .search()
        .explore(&query)
        .unwrap()
        .items
        .into_iter()
        .map(|item| {
            let owned = item.node.name.contains("port_failure_port_failure");
            (item.node.name, owned)
        })
        .collect()
}

fn tests_follow_others(ranked: &[(String, bool)]) -> bool {
    let first_test = ranked.iter().position(|(_, test)| *test);
    first_test.is_none_or(|first| ranked[first..].iter().all(|(_, test)| *test))
}

#[test]
fn test_owned_hits_follow_other_hits_but_are_kept() {
    let (_dir, index) = fixture();
    let neutral = ranked(&index, "port failure", TestRanking::Neutral);
    // The test code matches the query more often than the code under test.
    assert!(!tests_follow_others(&neutral), "{neutral:?}");
    let deferred = ranked(&index, "port failure", TestRanking::Defer);
    assert!(tests_follow_others(&deferred), "{deferred:?}");
    // Deferred, never dropped: both tests still fill slots.
    assert_eq!(
        deferred.iter().filter(|(_, test)| *test).count(),
        2,
        "{deferred:?}"
    );
}

#[test]
fn a_query_about_tests_or_naming_a_test_ranks_it_neutrally() {
    let (_dir, index) = fixture();
    for text in [
        "port failure tests",
        "port failure port failure is reported",
    ] {
        assert_eq!(
            ranked(&index, text, TestRanking::Defer)[..1],
            ranked(&index, text, TestRanking::Neutral)[..1],
            "{text}"
        );
    }
    let named = ranked(
        &index,
        "port failure port failure is reported",
        TestRanking::Defer,
    );
    assert_eq!(
        named[0].0, "port_failure_port_failure_is_reported",
        "{named:?}"
    );
}
