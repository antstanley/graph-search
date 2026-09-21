//! Composed native module lookup uses published facts, including negative paths.
#![allow(clippy::unwrap_used, clippy::panic)]
use graph_search::module_presence::Capture;
use graph_search::{Index, OpenOptions};
use graph_search_core::{
    ports::GraphStore,
    typescript::inherit,
    typescript_aliases::{Aliases, Dispatch},
    typescript_files::{Availability, Lookup, Mode, Options, Probe},
};
use graph_search_engine::{GrafeoStore, StoreOptions};
use std::{collections::BTreeSet, path::Path};

fn open(root: &Path, store: &Path) -> Index {
    Index::open(OpenOptions {
        root: root.into(),
        store: Some(store.into()),
        ..Default::default()
    })
    .unwrap()
}

type Result = (
    std::result::Result<Dispatch<String>, &'static str>,
    Vec<Probe>,
);

fn resolve(root: &Path, store: &Path, specifier: &str) -> Result {
    let store = GrafeoStore::open(store, &StoreOptions::default()).unwrap();
    let packages = store.manifest().unwrap().unwrap().package_boundaries;
    let snapshot = store.snapshot().unwrap();
    let config = inherit("tsconfig.json", snapshot.source_files()).unwrap();
    let aliases = Aliases::compile(&config).unwrap();
    let options = Options::compile(&config, Mode::Bundler).unwrap();
    let known: BTreeSet<_> = snapshot.source_files().keys().cloned().collect();
    let mut capture = Capture::new(root).unwrap();
    let mut presence = |path: &str| capture.classify(path);
    let mut lookup = Lookup::new(&options, &known, &packages, &mut presence);
    let result = aliases.resolve(specifier, |path, substitution| {
        lookup.load(path, substitution)
    });
    let probes = lookup.probes().to_vec();
    capture.validate().unwrap();
    (result, probes)
}

fn assert_target(result: &Result, target: &str) {
    assert!(matches!(&result.0, Ok(Dispatch::Paths { target: Some(path), .. }) if path == target));
}

fn parity(root: &Path, store: &Path, specifier: &str) -> Result {
    let actual = resolve(root, store, specifier);
    let clean = tempfile::tempdir().unwrap();
    open(root, clean.path()).reindex().unwrap();
    assert_eq!(actual, resolve(root, clean.path(), specifier));
    actual
}

#[test]
fn higher_priority_file_creation_removal_rename_and_config_edits_match_clean_rebuilds() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    let config = root.path().join("tsconfig.json");
    std::fs::write(
        &config,
        r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@/*":["./src/*"]}}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/target.js"),
        "export function target() {}\n",
    )
    .unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let original = resolve(root.path(), store.path(), "@/target.js");
    assert_target(&original, "src/target.js");
    assert!(
        original
            .1
            .iter()
            .any(|probe| probe.path == "src/target.ts"
                && probe.availability != Availability::Admitted)
    );

    std::fs::write(
        root.path().join("src/target.ts"),
        "export function target() {}\n",
    )
    .unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/target.js"),
        "src/target.ts",
    );

    std::fs::write(
        root.path().join("src/target.native.ts"),
        "export function target() {}\n",
    )
    .unwrap();
    std::fs::write(&config, r#"{"compilerOptions":{"moduleResolution":"bundler","moduleSuffixes":[".native",""],"paths":{"@/*":["./src/*"]}}}"#).unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/target.js"),
        "src/target.native.ts",
    );

    std::fs::remove_file(root.path().join("src/target.native.ts")).unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/target.js"),
        "src/target.ts",
    );

    std::fs::rename(
        root.path().join("src/target.ts"),
        root.path().join("src/renamed.ts"),
    )
    .unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/target.js"),
        "src/target.js",
    );

    std::fs::write(&config, r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@/target.js":["./src/renamed.ts"]}}}"#).unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/target.js"),
        "src/renamed.ts",
    );
    assert_target(&original, "src/target.js");
    assert!(
        original
            .1
            .iter()
            .any(|probe| probe.path == "src/target.ts"
                && probe.availability != Availability::Admitted)
    );
}

#[test]
fn unavailable_package_boundaries_block_directory_guessing_and_are_not_source_files() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".graph-search")).unwrap();
    std::fs::write(
        root.path().join(".graph-search/config.toml"),
        "max_file_bytes = 256\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("pkg")).unwrap();
    std::fs::write(root.path().join("tsconfig.json"), r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@pkg":["./pkg"],"@manifest":["./pkg/package.json"]}}}"#).unwrap();
    std::fs::write(
        root.path().join("pkg/index.ts"),
        "export function misleading() {}\n",
    )
    .unwrap();
    let package = root.path().join("pkg/package.json");
    std::fs::write(
        &package,
        serde_json::json!({"name":"pkg","padding":"x".repeat(1024)}).to_string(),
    )
    .unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let blocked = parity(root.path(), store.path(), "@pkg");
    assert_eq!(blocked.0, Err("ts_module_package_directory_unmodeled"));
    assert!(
        blocked
            .1
            .iter()
            .any(|probe| probe.path == "pkg/package.json"
                && probe.availability != Availability::Admitted
                && probe.package_boundary)
    );
    assert_eq!(
        resolve(root.path(), store.path(), "@manifest").0,
        Err("ts_module_target_unavailable")
    );

    std::fs::write(&package, r#"{"name":"pkg"}"#).unwrap();
    index.sync().unwrap();
    assert_eq!(
        parity(root.path(), store.path(), "@pkg").0,
        Err("ts_module_package_directory_unmodeled")
    );
    assert_target(
        &parity(root.path(), store.path(), "@manifest"),
        "pkg/package.json",
    );
}

#[test]
fn oversized_and_ignored_preferred_files_block_fallback_until_removed_or_admitted() {
    let root = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".graph-search")).unwrap();
    std::fs::write(
        root.path().join(".graph-search/config.toml"),
        "max_file_bytes = 256\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"moduleResolution":"bundler","paths":{"@/*":["./src/*"]}}}"#,
    )
    .unwrap();
    let preferred = root.path().join("src/entry.ts");
    std::fs::write(
        &preferred,
        format!("// {}\nexport const entry = 1;", "x".repeat(1024)),
    )
    .unwrap();
    std::fs::write(root.path().join("src/entry.js"), "export const entry = 2;").unwrap();
    let index = open(root.path(), store.path());
    index.reindex().unwrap();
    let blocked = parity(root.path(), store.path(), "@/entry.js");
    assert_eq!(blocked.0, Err("ts_module_target_unavailable"));
    assert_eq!(blocked.1.len(), 1);
    assert_eq!(blocked.1[0].path, "src/entry.ts");
    assert_eq!(blocked.1[0].availability, Availability::Unavailable);

    std::fs::remove_file(&preferred).unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/entry.js"),
        "src/entry.js",
    );
    std::fs::write(&preferred, "export const entry = 1;").unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/entry.js"),
        "src/entry.ts",
    );

    std::fs::write(root.path().join(".ignore"), "src/entry.ts\n").unwrap();
    index.sync().unwrap();
    let ignored = parity(root.path(), store.path(), "@/entry.js");
    assert_eq!(ignored.0, Err("ts_module_target_unavailable"));
    assert_eq!(ignored.1, blocked.1);
    std::fs::remove_file(root.path().join(".ignore")).unwrap();
    index.sync().unwrap();
    assert_target(
        &parity(root.path(), store.path(), "@/entry.js"),
        "src/entry.ts",
    );
}
