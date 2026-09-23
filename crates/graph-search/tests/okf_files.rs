//! Open Knowledge Format bundles end to end: bundle membership, concepts,
//! sections, cross-links and citations through one [`SearchService`] handle
//! (`SPEC.md` §7.6).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search::{Index, OpenOptions, Reconcile};
use graph_search_types::Language;
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::query::{NeighborsQuery, SymbolQuery};

fn write(dir: &std::path::Path, rel: &str, contents: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap_or_else(|| std::path::Path::new(".")))
        .unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(path, contents).unwrap_or_else(|e| panic!("write: {e}"));
}

fn open(root: &std::path::Path) -> Index {
    Index::open(OpenOptions {
        root: root.to_path_buf(),
        reconcile: Reconcile::BeforeQuery,
        ..OpenOptions::default()
    })
    .unwrap_or_else(|e| panic!("open: {e}"))
}

const GROSS_MARGIN: &str = "---
type: Metric
title: Gross Margin
description: Revenue minus full COGS.
sources:
  - id: standard
    resource: policies/margin-standard.md
---

# Definition

Gross margin equals [Revenue](./revenue.md) minus COGS.[^standard]

The sanctioned computation is [pending](/computations/gross-margin.md).

[^standard]: The FY2026 standard.
";

fn workspace() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
    let root = tmp.path();
    write(
        root,
        "kb/index.md",
        "# Metrics\n\n* [metrics](metrics/) - Headline numbers.\n",
    );
    write(
        root,
        "kb/metrics/revenue.md",
        "---\ntype: Metric\ntitle: Revenue\n---\n\n# Definition\n\nRecognized revenue.\n",
    );
    write(root, "kb/metrics/gross-margin.md", GROSS_MARGIN);
    write(
        root,
        "kb/policies/margin-standard.md",
        "---\ntype: Policy\ntitle: Cost Allocation Standard\n---\n\nFull COGS.\n",
    );
    // Outside every bundle: stays unknown-language prose.
    write(
        root,
        "README.md",
        "# Project\n\nSee [the bundle](kb/index.md).\n",
    );
    tmp
}

fn out_edges(index: &Index, target: &str, rel: EdgeKind) -> Vec<(bool, String)> {
    let mut query = NeighborsQuery::new(target);
    query.rel = Some(rel);
    let result = index
        .search()
        .neighbors(&query)
        .unwrap_or_else(|e| panic!("neighbors: {e}"));
    result
        .edges
        .iter()
        .filter(|edge| edge.kind == rel)
        .map(|edge| (edge.resolved, edge.to_name.clone()))
        .collect()
}

#[test]
fn okf_bundle_concepts_links_and_citations_resolve() {
    let tmp = workspace();
    let index = open(tmp.path());
    let report = index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    assert!(report.quarantined.is_empty(), "{report:?}");

    let status = index
        .search()
        .status()
        .unwrap_or_else(|e| panic!("status: {e}"));
    let counts = status.counts.expect("counts");
    assert_eq!(counts.files_by_language[&Language::Okf], 4, "{counts:?}");
    assert_eq!(
        counts.files_by_language[&Language::Unknown],
        1,
        "{counts:?}"
    );

    let found = index
        .search()
        .symbol(&SymbolQuery::new("Gross Margin"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert_eq!(found.nodes.len(), 1, "{found:?}");
    assert_eq!(found.nodes[0].kind, NodeKind::Concept);
    assert_eq!(found.nodes[0].path, "kb/metrics/gross-margin.md");

    // The section's relative link lands on the Revenue concept; the
    // bundle-relative link to an unwritten concept is dangling, not dropped.
    let links = out_edges(&index, "Gross Margin > Definition", EdgeKind::LinksTo);
    assert!(links.contains(&(true, "Revenue".to_owned())), "{links:?}");
    assert!(
        links.contains(&(false, "/computations/gross-margin.md".to_owned())),
        "{links:?}"
    );

    // `sources[].resource` is cited by the concept and, through `[^standard]`,
    // by the section making the claim.
    let standard = (true, "Cost Allocation Standard".to_owned());
    let cites = out_edges(&index, "Gross Margin", EdgeKind::Cites);
    assert!(cites.contains(&standard), "{cites:?}");
    let cites = out_edges(&index, "Gross Margin > Definition", EdgeKind::Cites);
    assert!(cites.contains(&standard), "{cites:?}");

    // Writing the missing concept resolves the dangling link on sync.
    write(
        tmp.path(),
        "kb/computations/gross-margin.md",
        "---\ntype: Attested Computation\ntitle: Gross Margin Period\n---\n",
    );
    index.sync().unwrap_or_else(|e| panic!("sync: {e}"));
    let links = out_edges(&index, "Gross Margin > Definition", EdgeKind::LinksTo);
    assert!(
        links.contains(&(true, "Gross Margin Period".to_owned())),
        "{links:?}"
    );
}

#[test]
fn removing_the_bundle_index_returns_its_documents_to_prose() {
    let tmp = workspace();
    let index = open(tmp.path());
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    std::fs::remove_file(tmp.path().join("kb/index.md")).unwrap_or_else(|e| panic!("rm: {e}"));
    index.sync().unwrap_or_else(|e| panic!("sync: {e}"));

    let counts = index
        .search()
        .status()
        .unwrap_or_else(|e| panic!("status: {e}"))
        .counts
        .expect("counts");
    assert!(
        !counts.files_by_language.contains_key(&Language::Okf),
        "{counts:?}"
    );
    let found = index
        .search()
        .symbol(&SymbolQuery::new("Gross Margin"))
        .unwrap_or_else(|e| panic!("symbol: {e}"));
    assert!(found.nodes.is_empty(), "{found:?}");
}

#[test]
fn language_filtered_search_finds_bundle_documents() {
    let tmp = workspace();
    let index = Index::open(OpenOptions {
        root: tmp.path().to_path_buf(),
        reconcile: Reconcile::Explicit,
        ..OpenOptions::default()
    })
    .unwrap_or_else(|e| panic!("open: {e}"));
    index.reindex().unwrap_or_else(|e| panic!("reindex: {e}"));
    let search = |text: &str| {
        let mut query = graph_search_types::ExploreQuery::new(text);
        query.filters.lang = Some(Language::Okf);
        let result = index
            .search()
            .explore(&query)
            .unwrap_or_else(|e| panic!("explore: {e}"));
        result
            .items
            .iter()
            .map(|item| item.node.path.clone())
            .collect::<Vec<_>>()
    };
    let revenue = "kb/metrics/revenue.md".to_owned();
    // Indexed documents, by terms and by phrase.
    for text in ["recognized revenue", "\"Recognized revenue\""] {
        assert!(search(text).contains(&revenue), "{text}");
    }
    // An edit not yet synced is searched live, still as an OKF document.
    write(
        tmp.path(),
        "kb/metrics/revenue.md",
        "---\ntype: Metric\ntitle: Revenue\n---\n\n# Definition\n\nZirconium ledger totals.\n",
    );
    for text in ["zirconium ledger", "\"Zirconium ledger\""] {
        assert!(search(text).contains(&revenue), "{text}");
    }
}
