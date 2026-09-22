//! Contract tests: the CLI is driven as a process, the way `nanus` drives
//! it through `bash` (`SPEC.md` §15.6, §10.3). Exit codes, empty-result
//! strings, cap notices, and include rejections are the contract.

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::process::Command;
use tempfile::TempDir;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_graph-search")
}

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("the binary runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn workspace() -> TempDir {
    let tmp = TempDir::new().expect("tmp");
    let root = tmp.path();
    std::fs::write(root.join("a.rs"), "fn alpha() { beta(); }\nfn beta() {}\n")
        .expect("write a.rs");
    tmp
}

#[test]
fn an_empty_glob_result_is_a_success_with_the_nanus_string() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "files",
        "*.md",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "No files found.\n");
}

#[test]
fn an_empty_text_result_after_a_cap_hits_the_honest_string() {
    let tmp = workspace();
    // 1-match cap across 2 matching files: the second is dropped.
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "text",
        "beta",
        "--limit",
        "1",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("stopped at 1 matches"), "{out}");
    assert!(!out.is_empty());
}

#[test]
fn a_comma_include_is_refused_with_a_usage_error() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "text",
        "beta",
        "--include",
        "*.rs,*.md",
    ]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("lists several"), "{err}");
    assert!(out.is_empty());
}

#[test]
fn a_negated_include_is_refused_with_the_same_guidance() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "text",
        "beta",
        "--include",
        "!*.rs",
    ]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("negates"), "{err}");
}

#[test]
fn an_empty_pattern_is_refused() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "text",
        "",
    ]);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("the pattern is empty"), "{err}");
}

#[test]
fn graph_modes_without_an_index_and_without_reconcile_exit_4() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "--no-reconcile",
        "search",
        "symbol",
        "alpha",
    ]);
    assert_eq!(code, 4, "{out}{err}");
    assert!(err.contains("no usable index"), "{err}");
}

#[test]
fn the_json_envelope_is_one_document_and_deterministic() {
    let tmp = workspace();
    let root = tmp.path().to_str().expect("path").to_owned();
    let index = Command::new(bin())
        .args(["--root", &root, "index"])
        .output()
        .expect("index runs");
    assert!(index.status.success());

    let args = [
        "--root".to_owned(),
        root.clone(),
        "search".into(),
        "files".into(),
        "**/*.rs".into(),
        "--json".into(),
    ];
    let first = Command::new(bin()).args(&args).output().expect("run");
    let second = Command::new(bin()).args(&args).output().expect("run");
    let parse = |bytes: &[u8]| -> serde_json::Value {
        serde_json::from_slice(bytes).expect("one JSON document")
    };
    let mut first = parse(&first.stdout);
    let mut second = parse(&second.stdout);
    // Only elapsed_ms may differ between runs over an unchanged tree.
    first["stats"]["elapsed_ms"] = serde_json::json!(0);
    second["stats"]["elapsed_ms"] = serde_json::json!(0);
    assert_eq!(
        first, second,
        "two runs must be byte-stable modulo elapsed_ms"
    );
    assert_eq!(
        first["schema_version"],
        serde_json::json!(graph_search_types::RESULT_SCHEMA_VERSION)
    );
    assert_eq!(first["command"], serde_json::json!("search.files"));
    assert_eq!(first["truncations"], serde_json::json!([]));
}

#[test]
fn json_exposes_source_mismatch_and_strict_verification_refreshes_it() {
    let tmp = workspace();
    let root = tmp.path().to_str().expect("path");
    let (code, _, err) = run(&["--root", root, "index"]);
    assert_eq!(code, 0, "{err}");
    let path = tmp.path().join("a.rs");
    let before = std::fs::metadata(&path)
        .expect("metadata")
        .modified()
        .expect("mtime");
    std::fs::write(&path, "fn gamma() { beta(); }\nfn beta() {}\n")
        .expect("replace same-size source");
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("open")
        .set_modified(before)
        .expect("restore mtime");
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "--no-reconcile",
        "search",
        "explore",
        "alpha",
    ]);
    assert_eq!(code, 0, "{err}");
    let json: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(json["stale"], true);
    assert_eq!(
        json["context"]["sources"]["a.rs"]["verification"],
        "mismatch"
    );
    let alpha = json["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|item| item["node"]["name"] == "alpha")
        .expect("indexed symbol");
    assert!(alpha["snippet"].is_null());
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "--verify-content",
        "search",
        "symbol",
        "gamma",
    ]);
    assert_eq!(code, 0, "{err}");
    let json: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(json["stale"], false);
    assert_eq!(json["context"]["freshness"], "content");
    assert_eq!(json["results"][0]["name"], "gamma");
}

#[test]
fn explore_intent_and_channel_diagnostics_survive_the_cli_envelope() {
    let tmp = workspace();
    let root = tmp.path().to_str().expect("path");
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "alpha",
        "--intent",
        "exact-name",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["plan"]["query"], "alpha");
    assert_eq!(value["plan"]["routes"], serde_json::json!(["exact_name"]));
    assert_eq!(value["query"]["retrieval"]["mode"], "exact_name");
    assert_eq!(value["results"][0]["retrieval"]["exact"], true);
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "alph",
        "--intent",
        "name-prefix",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["plan"]["routes"], serde_json::json!(["name_prefix"]));
    assert_eq!(value["results"][0]["node"]["name"], "alpha");
    assert_eq!(value["results"][0]["retrieval"]["exact"], false);

    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "alpha beta",
        "--ranking",
        "body",
        "--all-terms",
        "--per-file",
        "0",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["plan"]["routes"], serde_json::json!(["body"]));
    assert_eq!(value["query"]["retrieval"]["term_match"], "all");
    assert_eq!(value["query"]["retrieval"]["per_file"], 0);
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "where alpha",
        "--analysis",
        "identifiers",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["plan"]["options"]["analysis"], "identifiers");

    let (code, _, _) = run(&[
        "--root",
        root,
        "search",
        "explore",
        "alpha",
        "--all-terms",
        "--min-terms",
        "1",
    ]);
    assert_eq!(code, 2);
    let (code, _, _) = run(&[
        "--root",
        root,
        "search",
        "explore",
        "alpha",
        "--min-terms",
        "0",
    ]);
    assert_eq!(code, 2);
}

#[test]
fn explore_defaults_to_compact_detail_and_full_opts_in() {
    // Finding E2: the CLI is the context-cost-sensitive surface nanus drives,
    // so its default omits labelled excerpts and matched-body evidence; the
    // library default keeps them and `--detail full` restores them here.
    let tmp = workspace();
    let root = tmp.path().to_str().expect("path");
    let evidence_count = |out: &str| -> usize {
        let value: serde_json::Value = serde_json::from_str(out).expect("JSON");
        value["results"]
            .as_array()
            .expect("results")
            .iter()
            .filter(|item| {
                item["evidence"] != serde_json::Value::Null
                    || item["excerpts"].as_array().is_some_and(|e| !e.is_empty())
            })
            .count()
    };
    let (code, out, err) = run(&["--root", root, "--json", "search", "explore", "alpha beta"]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["query"]["detail"], "compact");
    assert_eq!(evidence_count(&out), 0, "compact must not publish evidence");

    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "alpha beta",
        "--detail",
        "full",
    ]);
    assert_eq!(code, 0, "{err}");
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["query"]["detail"], "full");
    assert!(
        evidence_count(&out) > 0,
        "full detail must publish evidence: {out}"
    );
}

#[test]
fn explore_envelope_keeps_symbols_when_source_lines_exceed_the_byte_cap() {
    let tmp = workspace();
    let text = format!(
        "fn budget_large() {{ let value = \"{}\"; }}\nfn budget_small() {{}}\n",
        "x".repeat(20_000)
    );
    std::fs::write(tmp.path().join("a.rs"), text).expect("write");
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("root"),
        "--json",
        "search",
        "explore",
        "budget_",
        "--intent",
        "name-prefix",
        "--max-bytes",
        "4096",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.len() <= 4096);
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["results"].as_array().expect("results").len(), 2);
    assert!(
        value["truncations"]
            .as_array()
            .expect("truncations")
            .iter()
            .any(|t| t["kind"] == "bytes" && t["cap"] == 4096)
    );
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("root"),
        "--json",
        "search",
        "explore",
        "budget_",
        "--intent",
        "name-prefix",
        "--max-bytes",
        "1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(out.is_empty());
}

#[test]
fn graph_and_impact_json_fit_the_final_transport_budget() {
    let tmp = workspace();
    let mut source = String::from("fn hub() {}\n");
    for i in 0..200 {
        writeln!(
            source,
            "fn caller_{i:03}_{}() {{ hub(); }}",
            "long_identifier_".repeat(12)
        )
        .expect("source text");
    }
    std::fs::write(tmp.path().join("a.rs"), source).expect("source");
    for mode in ["callers", "impact"] {
        let (code, out, err) = run(&[
            "--root",
            tmp.path().to_str().expect("root"),
            "--json",
            "search",
            mode,
            "hub",
            "--depth",
            "1",
            "--limit",
            "500",
        ]);
        assert_eq!(code, 0, "{mode}: {err}");
        assert!(out.len() <= 65_536, "{mode}: {} bytes", out.len());
        let value: serde_json::Value = serde_json::from_str(&out).expect("one JSON document");
        let nodes = if mode == "impact" {
            assert_eq!(value["results"]["by_depth"][0]["total"], 200);
            value["results"]["top"].as_array().expect("top")
        } else {
            value["results"].as_array().expect("nodes")
        };
        assert!(!nodes.is_empty());
        assert!(nodes.len() < 200);
        assert!(value["stats"]["candidates"].as_u64().expect("candidates") >= 200);
        let edges = value["edges"].as_array().expect("edges");
        let resolved = edges.iter().filter(|edge| edge["resolved"] == true).count();
        assert_eq!(value["approximation"]["resolved"], resolved);
        assert_eq!(
            value["approximation"]["unresolved"],
            edges.len().saturating_sub(resolved)
        );
        assert!(
            value["truncations"]
                .as_array()
                .expect("truncations")
                .iter()
                .any(|t| t["kind"] == "bytes" && t["cap"] == 65_536)
        );
        assert!(value["context"]["generation"].is_string());
    }
}

#[test]
fn occurrence_json_has_individual_sites_and_fits_transport_budget() {
    let tmp = workspace();
    std::fs::write(
        tmp.path().join("a.rs"),
        format!("fn alpha(){{{}}}\n", "missing();".repeat(600)),
    )
    .expect("source");
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("root"),
        "--json",
        "search",
        "occurrences",
        "missing",
        "--by",
        "name",
        "--limit",
        "500",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.trim_end().len() <= 65_536);
    let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(value["command"], "search.graph.occurrences");
    let items = value["results"]["items"].as_array().expect("items");
    assert!(!items.is_empty() && items.len() < 500);
    assert!(items[0]["source_hash"].is_string());
    assert_eq!(items[0]["occurrence"]["resolution"], "unresolved");
    assert!(
        value["truncations"]
            .as_array()
            .expect("caps")
            .iter()
            .any(|t| t["kind"] == "bytes")
    );
}

#[test]
fn file_and_text_json_include_envelope_bytes_in_the_cap() {
    let tmp = workspace();
    for n in 0..600 {
        std::fs::write(
            tmp.path().join(format!("{n:04}-{}.txt", "x".repeat(180))),
            "",
        )
        .expect("file");
    }
    std::fs::write(
        tmp.path().join("matches.txt"),
        format!("needle {}\n", "é".repeat(140)).repeat(1000),
    )
    .expect("source");
    for (mode, pattern) in [("files", "*.txt"), ("text", "needle")] {
        let (code, out, err) = run(&[
            "--root",
            tmp.path().to_str().expect("root"),
            "--json",
            "search",
            mode,
            pattern,
            "--limit",
            "500",
        ]);
        assert_eq!(code, 0, "{err}");
        assert!(out.len() <= 65_536);
        let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
        let items = value["results"].as_array().expect("items");
        assert!(!items.is_empty() && items.len() < 500);
        assert!(
            value["truncations"]
                .as_array()
                .expect("caps")
                .iter()
                .any(|t| t["kind"] == "bytes")
        );
        if mode == "text" {
            assert_eq!(
                value["context"]["sources"]["matches.txt"]["verification"],
                "live"
            );
        }
    }
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("root"),
        "search",
        "text",
        "needle",
        "--limit",
        "500",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("byte cap"), "{out}");
    assert!(!out.contains("stopped at 500 matches"));
}

#[test]
fn status_bounds_changed_path_details_and_preserves_exact_totals() {
    let root = TempDir::new().expect("tmp");
    let names: Vec<_> = (0..500)
        .map(|i| format!("{i:04}-{}.txt", "long-name".repeat(20)))
        .collect();
    for name in &names {
        std::fs::write(root.path().join(name), "before").expect("source");
    }
    let index = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        reconcile: graph_search::Reconcile::Never,
        ..Default::default()
    })
    .expect("index");
    index.reindex().expect("build");
    for name in &names {
        std::fs::write(root.path().join(name), "changed content").expect("edit");
    }
    let status = index.search().status().expect("status");
    assert!(serde_json::to_vec(&status).expect("json").len() <= 65_536);
    assert_eq!(status.staleness.as_ref().expect("stale").changed, 500);
    assert_eq!(status.counts.as_ref().expect("counts").total_nodes, 500);
    drop(index);
    let (code, out, err) = run(&[
        "--root",
        root.path().to_str().expect("path"),
        "--json",
        "status",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.trim_end().len() <= 65_536);
    let value: serde_json::Value = serde_json::from_str(&out).expect("json");
    let result = &value["results"];
    assert_eq!(result["staleness"]["changed"], 500);
    assert_eq!(result["counts"]["total_nodes"], 500);
    let retained = result["staleness"]["changed_paths"]
        .as_array()
        .expect("paths");
    assert!(!retained.is_empty() && retained.len() < names.len());
    for (actual, expected) in retained.iter().zip(&names) {
        assert_eq!(actual, expected);
    }
    assert!(
        result["coverage"]["truncations"]
            .as_array()
            .expect("limits")
            .iter()
            .any(|t| t["kind"] == "bytes")
    );
    let (code, out, err) = run(&[
        "--root",
        root.path().to_str().expect("path"),
        "--json",
        "--fail-if-stale",
        "--no-reconcile",
        "search",
        "symbol",
        "unused",
    ]);
    assert_eq!(code, 3, "{err}");
    assert!(out.trim_end().len() <= 65_536);
    let value: serde_json::Value = serde_json::from_str(&out).expect("stale json");
    assert_eq!(value["context"]["staleness"]["changed"], 500);
    assert_eq!(value["stale"], true);
    assert_eq!(value["results"], value["stale_paths"]);
    assert_eq!(
        value["results"],
        value["context"]["staleness"]["changed_paths"]
    );
    assert!(!value["results"].as_array().expect("paths").is_empty());
    let (code, text, err) = run(&[
        "--root",
        root.path().to_str().expect("path"),
        "--fail-if-stale",
        "--no-reconcile",
        "search",
        "symbol",
        "unused",
    ]);
    assert_eq!(code, 3, "{err}");
    assert!(text.contains("500 changed paths"), "{text}");
}

#[test]
fn index_report_bounds_details_without_losing_committed_totals() {
    let root = TempDir::new().expect("tmp");
    for i in 0..500 {
        let path = root
            .path()
            .join(format!("{i:04}-{}.txt", "long-name".repeat(20)));
        std::fs::write(
            path,
            if i < 5 {
                b"\0binary".as_slice()
            } else {
                b"source text".as_slice()
            },
        )
        .expect("source");
    }
    let index = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .expect("open");
    let report = index.reindex().expect("build");
    assert!(serde_json::to_vec(&report).expect("json").len() <= 65_536);
    assert_eq!(report.totals().added, 500);
    assert_eq!(report.totals().quarantined, 5);
    assert_eq!(report.coverage.quarantined_files, 5);
    assert_eq!(report.coverage.source_indexed_files, 495);
    assert!(!report.added.is_empty() && report.added.len() < 500);
    assert!(!report.is_empty());
    assert_eq!(
        index
            .search()
            .status()
            .expect("status")
            .counts
            .expect("counts")
            .total_nodes,
        500
    );
    drop(index);
    let (code, out, err) = run(&[
        "--root",
        root.path().to_str().expect("path"),
        "--json",
        "index",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.trim_end().len() <= 65_536);
    let value: serde_json::Value = serde_json::from_str(&out).expect("json");
    let report = &value["results"];
    assert_eq!(report["counts"]["added"], 500);
    assert_eq!(report["counts"]["quarantined"], 5);
    assert_eq!(report["coverage"]["source_indexed_files"], 495);
    assert!(report["added"].as_array().expect("details").len() < 500);
    assert!(
        report["coverage"]["truncations"]
            .as_array()
            .expect("limits")
            .iter()
            .any(|t| t["kind"] == "bytes")
    );
}

#[test]
fn scoped_live_scans_do_not_require_an_unrelated_status_walk() {
    let root = TempDir::new().expect("tmp");
    for dir in ["clean", "unrelated"] {
        std::fs::create_dir(root.path().join(dir)).expect("dir");
    }
    std::fs::write(root.path().join("clean/a.rs"), "fn needle() {}\n").expect("source");
    std::fs::write(root.path().join("unrelated/b.rs"), "fn other() {}\n").expect("source");
    let index = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .expect("index");
    index.reindex().expect("build");
    drop(index);
    std::fs::write(root.path().join("unrelated/.ignore"), "[z-a]\n")
        .expect("invalid sibling ignore");
    let path = root.path().to_str().expect("path");
    for (mode, query) in [("files", "*.rs"), ("text", "needle")] {
        let (code, out, err) = run(&[
            "--root",
            path,
            "--json",
            "--no-reconcile",
            "search",
            mode,
            query,
            "--path",
            "clean",
        ]);
        assert_eq!(code, 0, "{mode}: {err}");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["results"].as_array().expect("hits").len(), 1);
        assert_eq!(value["context"]["coverage"]["enumeration_complete"], true);
        assert_eq!(value["context"]["freshness"], "live");
    }
    let (code, _, err) = run(&[
        "--root",
        path,
        "--no-reconcile",
        "--fail-if-stale",
        "search",
        "text",
        "needle",
        "--path",
        "clean",
    ]);
    assert_ne!(code, 0);
    assert!(err.contains("incomplete source enumeration"), "{err}");
}

#[test]
fn phrase_and_near_intents_have_explicit_distance_contracts() {
    let tmp = workspace();
    std::fs::write(tmp.path().join("guide.md"), "alpha middle beta\n").expect("guide");
    let root = tmp.path().to_str().expect("path");
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "alpha beta",
        "--intent",
        "phrase",
        "--phrase-gap",
        "1",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("guide.md"), "{out}");
    assert!(out.contains("\"phrase\""), "{out}");
    let (code, out, err) = run(&[
        "--root",
        root,
        "--json",
        "search",
        "explore",
        "beta alpha",
        "--intent",
        "near",
        "--near-window",
        "3",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("guide.md"), "{out}");
    assert!(out.contains("\"near\""), "{out}");
}

#[test]
fn impossible_positional_query_is_rejected_before_opening_index() {
    let tmp = workspace();
    let (code, _, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "search",
        "explore",
        "alpha beta",
        "--intent",
        "near",
        "--near-window",
        "1",
    ]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("shorter than the query"), "{err}");
    assert!(!tmp.path().join(".graph-search").exists());
}

#[test]
fn explicit_metadata_normalization_is_explained_without_changing_navigation() {
    let tmp = workspace();
    let (code, out, err) = run(&[
        "--root",
        tmp.path().to_str().expect("path"),
        "--json",
        "search",
        "explore",
        "alpha",
        "--intent",
        "exact-name",
        "--normalization",
        "bm25f",
        "--explain",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("\"normalization\":\"bm25f\""), "{out}");
    assert!(out.contains("\"exact_name\""), "{out}");
    assert!(out.contains("alpha"), "{out}");
}

#[test]
fn graph_context_mode_is_exposed_and_validated_by_the_cli() {
    let tmp = workspace();
    for mode in ["none", "calls", "imports", "types", "semantic"] {
        let (code, out, err) = run(&[
            "--root",
            tmp.path().to_str().expect("path"),
            "--json",
            "search",
            "explore",
            "alpha beta",
            "--graph-context",
            mode,
            "--explain",
        ]);
        assert_eq!(code, 0, "{err}");
        assert!(
            out.contains(&format!("\"graph_context\":\"{mode}\"")),
            "{out}"
        );
        if mode == "none" {
            assert!(!out.contains("\"direct_callers\""), "{out}");
        }
    }
    let (code, _, _) = run(&[
        "search",
        "explore",
        "alpha",
        "--graph-context",
        "everything",
    ]);
    assert_eq!(code, 2);
}

#[test]
fn task_query_policy_keeps_original_input_and_reports_omitted_suffixes() {
    let tmp = workspace();
    let query = "alpha beta. Do not modify files.";
    for policy in ["verbatim", "task"] {
        let (code, out, err) = run(&[
            "--root",
            tmp.path().to_str().expect("path"),
            "--json",
            "search",
            "explore",
            query,
            "--query-policy",
            policy,
            "--ranking",
            "body",
            "--all-terms",
            "--explain",
        ]);
        assert_eq!(code, 0, "{err}");
        let value: serde_json::Value = serde_json::from_str(&out).expect("JSON");
        assert_eq!(value["plan"]["query"], query);
        assert_eq!(value["plan"]["options"]["query_policy"], policy);
        let results = value["results"].as_array().expect("results");
        if policy == "task" {
            assert!(!results.is_empty());
            assert_eq!(
                value["plan"]["omitted_boilerplate"],
                serde_json::json!(["Do not modify files."])
            );
        } else {
            assert!(results.is_empty());
            assert!(value["plan"]["omitted_boilerplate"].is_null());
        }
    }
}

#[test]
fn explore_package_references_survive_final_envelope_trimming() {
    let root = TempDir::new().expect("tmp");
    for directory in ["one", "two"] {
        let base = root.path().join(directory);
        std::fs::create_dir(&base).expect("directory");
        std::fs::write(
            base.join("Cargo.toml"),
            "[package]\nname='same-name'\nversion='0.1.0'\n",
        )
        .expect("manifest");
        for name in ["alpha", "beta"] {
            std::fs::write(
                base.join(format!("{name}.rs")),
                format!("pub fn {name}() {{\n // saffron boundary\n}}\n"),
            )
            .expect("source");
        }
    }
    let path = root.path().to_str().expect("path");
    let index = graph_search::Index::open(graph_search::OpenOptions {
        root: root.path().into(),
        ..Default::default()
    })
    .expect("open");
    index.reindex().expect("index");
    drop(index);
    for cap in ["2500", "4000", "16384"] {
        let (code, out, err) = run(&[
            "--root",
            path,
            "--json",
            "search",
            "explore",
            "saffron boundary",
            "--detail",
            "full",
            "--max-bytes",
            cap,
        ]);
        assert_eq!(code, 0, "{err}");
        assert!(out.trim_end().len() <= cap.parse::<usize>().expect("cap"));
        let envelope: serde_json::Value = serde_json::from_str(&out).expect("envelope");
        let items: Vec<graph_search_types::result::ExploreItem> =
            serde_json::from_value(envelope["results"].clone()).expect("items");
        let context: graph_search_types::context::ResultContext =
            serde_json::from_value(envelope["context"].clone()).expect("context");
        let mut used = std::collections::BTreeSet::new();
        for item in &items {
            let evidence = item.evidence.as_ref().expect("evidence");
            used.insert(evidence.package_ref.as_ref().expect("reference").clone());
            let identity = evidence
                .package_identity(&context)
                .expect("referenced package retained");
            assert_eq!(identity.name.as_deref(), Some("same-name"));
            assert!(
                item.node
                    .path
                    .starts_with(identity.manifest_path.split('/').next().expect("directory"))
            );
        }
        assert_eq!(used, context.packages.keys().cloned().collect());
        if cap == "16384" {
            assert_eq!(items.len(), 4);
            assert_eq!(used.len(), 2);
        }
    }
}
