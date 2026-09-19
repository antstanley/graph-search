//! Contract tests: the CLI is driven as a process, the way `nanus` drives
//! it through `bash` (`SPEC.md` §15.6, §10.3). Exit codes, empty-result
//! strings, cap notices, and include rejections are the contract.

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

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
        serde_json::json!(graph_search_types::SCHEMA_VERSION)
    );
    assert_eq!(first["command"], serde_json::json!("search.files"));
    assert_eq!(first["truncations"], serde_json::json!([]));
}
