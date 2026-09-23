//! Native module-declaration paths. Cargo roots are facts, never filesystem probes.
use graph_search_types::package::{
    CargoBuildScript, CargoTargetKind as Kind, PackageManifest, PackageRole,
};
use graph_search_types::{Node, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path};

#[derive(Clone, Debug, Default)]
pub(crate) struct Catalog {
    roots: BTreeSet<String>,
    declarations: BTreeMap<NodeId, Result<String, &'static str>>,
    uncertain: BTreeSet<String>,
    packages: BTreeSet<String>,
    /// Workspace library crates by the name other crates spell them with
    /// (`nanus_domain`), to their library root. Two packages claiming one
    /// name are ambiguous, never a guess.
    crates: BTreeMap<String, Result<String, &'static str>>,
}

/// Normalize only within the walked workspace; never read or follow a path.
fn join(base: &str, tail: &str) -> Option<String> {
    if tail.contains('\\') || tail.contains('\0') || Path::new(tail).is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    let combined = Path::new(base).join(tail);
    for part in combined.components() {
        match part {
            Component::Normal(value) => parts.push(value.to_str()?),
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Catalog {
    pub(crate) fn roots(&self) -> impl Iterator<Item = &str> {
        self.roots.iter().map(String::as_str)
    }

    /// Workspace library crate roots by the name a path spells them with.
    pub(crate) fn libraries(&self) -> BTreeMap<String, Result<NodeId, &'static str>> {
        self.crates
            .iter()
            .map(|(name, root)| (name.clone(), root.clone().map(|root| NodeId::file(&root))))
            .collect()
    }
    pub(crate) fn build(
        files: &BTreeMap<String, Node>,
        known: &BTreeSet<String>,
        boundaries: &BTreeSet<String>,
    ) -> Self {
        let mut result = Self::default();
        for path in boundaries.iter().filter(|path| {
            Path::new(path)
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
        }) {
            result.packages.insert(parent(path).into());
        }
        let mut grouped: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for path in known.iter().filter(|path| {
            Path::new(path)
                .extension()
                .is_some_and(|extension| extension == "rs")
        }) {
            if let Some(package) = result.package(path) {
                grouped.entry(package.to_owned()).or_default().push(path);
            } else if matches!(
                Path::new(path).file_name().and_then(|n| n.to_str()),
                Some("lib.rs" | "main.rs")
            ) {
                result.roots.insert(path.clone());
            }
        }
        let manifest = |directory: &str| {
            join(directory, "Cargo.toml")
                .and_then(|path| files.get(&path))
                .and_then(|node| node.attribute("package_definition"))
                .and_then(|value| serde_json::from_str::<PackageManifest>(value).ok())
        };
        for directory in &result.packages {
            let definition = manifest(directory);
            // `edition.workspace = true` inherits the nearest enclosing
            // workspace root's `[workspace.package] edition`.
            let inherited = definition
                .as_ref()
                .and_then(|definition| definition.cargo_targets.as_ref())
                .filter(|metadata| metadata.edition_workspace)
                .and_then(|_| {
                    let mut dir = Some(directory.as_str());
                    while let Some(current) = dir {
                        if let Some(edition) = manifest(current)
                            .and_then(|definition| definition.cargo_targets)
                            .and_then(|metadata| metadata.workspace_edition)
                        {
                            return Some(edition);
                        }
                        dir = (!current.is_empty()).then(|| parent(current));
                    }
                    None
                });
            let paths = grouped.get(directory).map_or(&[][..], Vec::as_slice);
            if let Some((definition, roots)) = definition.as_ref().and_then(|definition| {
                package_roots(directory, definition, inherited.as_deref(), paths, known)
                    .map(|roots| (definition, roots))
            }) {
                if let Some((name, root)) = library(directory, definition, &roots) {
                    result
                        .crates
                        .entry(name)
                        .and_modify(|existing| *existing = Err("rust_crate_name_ambiguous"))
                        .or_insert(Ok(root));
                }
                result.roots.extend(roots);
            } else {
                result.uncertain.insert(directory.clone());
            }
        }
        result
    }

    /// A file has at most two directory contexts: physical parent (root/path
    /// attribute) and physical stem (ordinary module). Cycles cannot grow this
    /// worklist beyond twice the walked file set.
    pub(crate) fn populate(&mut self, symbols: &BTreeMap<NodeId, Node>, known: &BTreeSet<String>) {
        let mut modules: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
        for node in symbols
            .values()
            .filter(|node| node.attribute("rust_module_form") == Some("external"))
        {
            modules.entry(&node.path).or_default().push(node);
        }
        let mut contexts: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut pending = VecDeque::new();
        for path in &self.roots {
            let directory = parent(path).to_owned();
            contexts
                .entry(path.clone())
                .or_default()
                .insert(directory.clone());
            pending.push_back((path.clone(), directory));
        }
        let mut choices: BTreeMap<NodeId, BTreeSet<Result<String, &'static str>>> = BTreeMap::new();
        while let Some((path, directory)) = pending.pop_front() {
            for node in modules.get(path.as_str()).into_iter().flatten() {
                let resolved = in_directory(node, symbols, known, &directory);
                if let Ok(target) = &resolved
                    && contexts
                        .entry(target.path.clone())
                        .or_default()
                        .insert(target.directory.clone())
                {
                    pending.push_back((target.path.clone(), target.directory.clone()));
                }
                choices
                    .entry(node.id.clone())
                    .or_default()
                    .insert(resolved.map(|target| target.path));
            }
        }
        for (path, nodes) in modules {
            for node in nodes {
                let result = match choices.get(&node.id) {
                    Some(choices) if choices.len() == 1 => choices
                        .first()
                        .cloned()
                        .unwrap_or(Err("module_target_missing")),
                    Some(_) => Err("rust_module_context_ambiguous"),
                    None if self
                        .package(path)
                        .is_some_and(|package| self.uncertain.contains(package)) =>
                    {
                        Err("rust_target_roots_unavailable")
                    }
                    None => Err("rust_module_context_unknown"),
                };
                self.declarations.insert(node.id.clone(), result);
            }
        }
    }

    pub(crate) fn declaration(&self, id: &NodeId) -> Result<String, &'static str> {
        self.declarations
            .get(id)
            .cloned()
            .unwrap_or(Err("rust_module_context_unknown"))
    }

    fn package(&self, path: &str) -> Option<&str> {
        let mut dir = parent(path);
        loop {
            if let Some(key) = self.packages.get(dir) {
                return Some(key);
            }
            if dir.is_empty() {
                return None;
            }
            dir = parent(dir);
        }
    }
}

fn package_roots(
    directory: &str,
    definition: &PackageManifest,
    inherited_edition: Option<&str>,
    paths: &[&str],
    known: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    if definition.role != PackageRole::Package {
        return None;
    }
    let metadata = definition
        .cargo_targets
        .as_ref()
        .filter(|value| value.unavailable_reason.is_none())?;
    let inferred_default = |kind| {
        let declared = metadata.empty_target_tables.contains(&kind)
            || metadata.targets.iter().any(|target| target.kind == kind);
        let edition = metadata
            .edition
            .as_deref()
            .or(inherited_edition.filter(|_| metadata.edition_workspace));
        match edition {
            Some("2018" | "2021" | "2024") => Some(true),
            None if metadata.edition_workspace => (!declared).then_some(true),
            None | Some("2015") => Some(!declared),
            _ => None,
        }
    };
    let mut candidates: BTreeMap<(Kind, String), BTreeSet<String>> = BTreeMap::new();
    for path in paths {
        let relative = if directory.is_empty() {
            *path
        } else {
            path.strip_prefix(directory)?.strip_prefix('/')?
        };
        if let Some((kind, name)) =
            inferred_target(relative, definition.name.as_deref().unwrap_or_default())
        {
            candidates
                .entry((kind, name))
                .or_default()
                .insert((*path).into());
        }
    }
    let mut explicit = BTreeSet::new();
    for target in &metadata.targets {
        let name = if target.kind == Kind::Lib {
            String::new()
        } else {
            target.name.clone()?
        };
        let key = (target.kind, name);
        if !explicit.insert(key.clone()) {
            return None;
        }
        let selected = if let Some(path) = &target.path {
            let normalized = join(directory, path)?;
            known
                .contains(&normalized)
                .then_some(normalized)
                .into_iter()
                .collect()
        } else {
            candidates.get(&key).cloned().unwrap_or_default()
        };
        candidates.insert(key, selected);
    }
    let mut roots = BTreeSet::new();
    let build = match &metadata.build_script {
        Some(CargoBuildScript::Disabled) => None,
        Some(CargoBuildScript::Path(path)) => Some(join(directory, path)?),
        None | Some(CargoBuildScript::Default) => join(directory, "build.rs"),
    };
    roots.extend(build.filter(|path| known.contains(path)));
    for ((kind, name), paths) in candidates {
        let enabled = explicit.contains(&(kind, name))
            || metadata
                .auto_discovery
                .get(&kind)
                .copied()
                .or_else(|| inferred_default(kind))?;
        if enabled {
            if paths.len() > 1 {
                return None;
            }
            roots.extend(paths);
        }
    }
    Some(roots)
}

/// A package's library crate: the name dependents spell it with (the `[lib]`
/// name, else the package name with `-` as `_`) and its selected root.
fn library(
    directory: &str,
    definition: &PackageManifest,
    roots: &BTreeSet<String>,
) -> Option<(String, String)> {
    let metadata = definition.cargo_targets.as_ref()?;
    let explicit = metadata
        .targets
        .iter()
        .find(|target| target.kind == Kind::Lib);
    let root = match explicit.and_then(|target| target.path.as_deref()) {
        Some(path) => join(directory, path)?,
        None => join(directory, "src/lib.rs")?,
    };
    if !roots.contains(&root) {
        return None;
    }
    let name = explicit
        .and_then(|target| target.name.clone())
        .or_else(|| definition.name.clone())?
        .replace('-', "_");
    (!name.is_empty()).then_some((name, root))
}

fn inferred_target(path: &str, package: &str) -> Option<(Kind, String)> {
    match path {
        "src/lib.rs" => return Some((Kind::Lib, String::new())),
        "src/main.rs" => return Some((Kind::Bin, package.into())),
        _ => {}
    }
    for (kind, prefix) in [
        (Kind::Bin, "src/bin/"),
        (Kind::Example, "examples/"),
        (Kind::Test, "tests/"),
        (Kind::Bench, "benches/"),
    ] {
        let Some(tail) = path.strip_prefix(prefix) else {
            continue;
        };
        let name = tail
            .strip_suffix("/main.rs")
            .or_else(|| tail.strip_suffix(".rs"))?;
        if !name.is_empty() && !name.starts_with('.') && !name.contains('/') {
            return Some((kind, name.into()));
        }
    }
    None
}

struct Target {
    path: String,
    directory: String,
}

fn in_directory(
    node: &Node,
    symbols: &BTreeMap<NodeId, Node>,
    known: &BTreeSet<String>,
    base: &str,
) -> Result<Target, &'static str> {
    let mut chain = vec![node];
    let mut ancestor = node.parent.as_ref();
    while let Some(id) = ancestor {
        let parent = symbols.get(id).ok_or("rust_module_parent_missing")?;
        if parent.kind != NodeKind::Module || parent.attribute("rust_module_form") != Some("inline")
        {
            return Err("rust_block_module_unsupported");
        }
        if chain.len() >= 256 {
            return Err("rust_module_depth_limit");
        }
        chain.push(parent);
        ancestor = parent.parent.as_ref();
    }
    chain.reverse();
    let mut directory = base.to_owned();
    for (index, module) in chain.iter().enumerate() {
        if module.attribute("rust_module_unavailable").is_some() {
            return Err("rust_module_attribute_unsupported");
        }
        let name = module
            .name
            .as_deref()
            .and_then(|name| name.strip_prefix("r#").or(Some(name)))
            .ok_or("rust_module_name_missing")?;
        let external = index.checked_add(1) == Some(chain.len());
        if let Some(path) = module.attribute("rust_module_path") {
            // A top-level path attribute uses the physical file's directory.
            let base = if index == 0 {
                parent(&node.path)
            } else {
                &directory
            };
            directory = join(base, path).ok_or("rust_module_path_outside_workspace")?;
            if external {
                return known
                    .contains(&directory)
                    .then(|| Target {
                        directory: parent(&directory).into(),
                        path: directory,
                    })
                    .ok_or("module_target_missing");
            }
        } else {
            directory = join(&directory, name).ok_or("rust_module_name_unsupported")?;
            if external {
                let candidates = [format!("{directory}.rs"), format!("{directory}/mod.rs")];
                let found: Vec<_> = candidates
                    .into_iter()
                    .filter(|path| known.contains(path))
                    .collect();
                return match found.as_slice() {
                    [only] => Ok(Target {
                        path: only.clone(),
                        directory: if Path::new(only)
                            .file_name()
                            .is_some_and(|name| name == "mod.rs")
                        {
                            parent(only).into()
                        } else {
                            only.strip_suffix(".rs")
                                .ok_or("rust_module_file_unsupported")?
                                .into()
                        },
                    }),
                    [] => Err("module_target_missing"),
                    _ => Err("rust_module_files_ambiguous"),
                };
            }
        }
    }
    Err("module_target_missing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::package::{CargoTarget, CargoTargetMetadata, PackageEcosystem};

    fn catalog(metadata: CargoTargetMetadata, paths: &[&str]) -> Catalog {
        let definition = PackageManifest {
            node: None,
            ecosystem: PackageEcosystem::Cargo,
            role: PackageRole::Package,
            name: Some("p".into()),
            unavailable_reason: None,
            cargo_targets: Some(metadata),
        };
        let file = Node {
            kind: NodeKind::File,
            path: "p/Cargo.toml".into(),
            attributes: BTreeMap::from([(
                "package_definition".into(),
                serde_json::to_string(&definition).unwrap(),
            )]),
            ..Node::default()
        };
        Catalog::build(
            &BTreeMap::from([("p/Cargo.toml".into(), file)]),
            &paths.iter().map(|path| format!("p/{path}")).collect(),
            &BTreeSet::from(["p/Cargo.toml".into()]),
        )
    }

    #[test]
    fn cargo_root_discovery_honors_explicit_targets_edition_flags_and_layout() {
        let paths = [
            "src/lib.rs",
            "src/main.rs",
            "src/bin/worker.rs",
            "src/bin/admin/main.rs",
            "examples/demo.rs",
            "tests/check/main.rs",
            "benches/speed.rs",
            "src/lib/child.rs",
            "src/bin/.hidden.rs",
            "examples/.hidden/main.rs",
        ];
        let modern = CargoTargetMetadata {
            edition: Some("2021".into()),
            ..Default::default()
        };
        let roots = catalog(modern.clone(), &paths);
        assert_eq!(roots.roots.len(), 7);
        assert!(roots.roots.contains("p/src/bin/worker.rs"));
        assert!(!roots.roots.contains("p/src/lib/child.rs"));
        let target = CargoTarget {
            kind: Kind::Lib,
            name: None,
            path: Some("entry.rs".into()),
            required_features: vec![],
        };
        let mut explicit = modern.clone();
        explicit.targets.push(target.clone());
        let roots = catalog(explicit, &["entry.rs", "src/lib.rs", "src/main.rs"]);
        assert_eq!(
            roots.roots,
            BTreeSet::from(["p/entry.rs".into(), "p/src/main.rs".into()])
        );
        let legacy = CargoTargetMetadata {
            targets: vec![target.clone()],
            ..Default::default()
        };
        let roots = catalog(legacy.clone(), &["entry.rs", "src/lib.rs", "src/main.rs"]);
        assert_eq!(
            roots.roots,
            BTreeSet::from(["p/entry.rs".into(), "p/src/main.rs".into()])
        );
        let mut enabled = legacy;
        enabled.auto_discovery.insert(Kind::Bin, true);
        assert!(
            catalog(enabled, &["entry.rs", "src/main.rs"])
                .roots
                .contains("p/src/main.rs")
        );
        let mut disabled = modern;
        disabled.auto_discovery.insert(Kind::Bin, false);
        assert!(!catalog(disabled, &paths).roots.contains("p/src/main.rs"));
        let inherited = CargoTargetMetadata {
            edition_workspace: true,
            targets: vec![CargoTarget {
                kind: Kind::Bin,
                name: Some("worker".into()),
                ..target
            }],
            ..Default::default()
        };
        assert!(
            catalog(inherited, &["entry.rs", "src/main.rs"])
                .uncertain
                .contains("p")
        );
    }

    #[test]
    fn edition_2015_defaults_are_per_family_and_empty_arrays_are_explicit() {
        let paths = [
            "src/lib.rs",
            "src/main.rs",
            "src/bin/worker.rs",
            "examples/demo.rs",
            "tests/check.rs",
            "benches/speed.rs",
            "entry.rs",
        ];
        let cases: &[(Kind, &[&str])] = &[
            (
                Kind::Lib,
                &[
                    "src/main.rs",
                    "src/bin/worker.rs",
                    "examples/demo.rs",
                    "tests/check.rs",
                    "benches/speed.rs",
                ],
            ),
            (
                Kind::Bin,
                &[
                    "src/lib.rs",
                    "examples/demo.rs",
                    "tests/check.rs",
                    "benches/speed.rs",
                ],
            ),
            (
                Kind::Example,
                &[
                    "src/lib.rs",
                    "src/main.rs",
                    "src/bin/worker.rs",
                    "tests/check.rs",
                    "benches/speed.rs",
                ],
            ),
            (
                Kind::Test,
                &[
                    "src/lib.rs",
                    "src/main.rs",
                    "src/bin/worker.rs",
                    "examples/demo.rs",
                    "benches/speed.rs",
                ],
            ),
            (
                Kind::Bench,
                &[
                    "src/lib.rs",
                    "src/main.rs",
                    "src/bin/worker.rs",
                    "examples/demo.rs",
                    "tests/check.rs",
                ],
            ),
        ];
        for &(kind, expected_paths) in cases {
            let target = CargoTarget {
                kind,
                name: Some("explicit".into()),
                path: Some("entry.rs".into()),
                required_features: vec![],
            };
            let metadata = CargoTargetMetadata {
                targets: vec![target],
                ..Default::default()
            };
            // Literal target sets captured from Cargo, independent of native inference.
            let expected: BTreeSet<_> = expected_paths
                .iter()
                .map(|path| format!("p/{path}"))
                .chain(["p/entry.rs".into()])
                .collect();
            assert_eq!(catalog(metadata, &paths).roots, expected, "{kind:?}");
            if kind != Kind::Lib {
                let empty = CargoTargetMetadata {
                    empty_target_tables: BTreeSet::from([kind]),
                    ..Default::default()
                };
                let mut expected = expected;
                expected.remove("p/entry.rs");
                assert_eq!(catalog(empty, &paths).roots, expected, "empty {kind:?}");
            }
        }
    }

    #[test]
    fn build_scripts_are_roots_without_execution() {
        for (setting, expected) in [
            (None, Some("p/build.rs")),
            (Some(CargoBuildScript::Default), Some("p/build.rs")),
            (Some(CargoBuildScript::Disabled), None),
            (
                Some(CargoBuildScript::Path("tools/setup.rs".into())),
                Some("p/tools/setup.rs"),
            ),
        ] {
            let metadata = CargoTargetMetadata {
                build_script: setting,
                ..Default::default()
            };
            let roots = catalog(metadata, &["build.rs", "tools/setup.rs"]);
            assert_eq!(
                roots.roots,
                expected.into_iter().map(str::to_owned).collect()
            );
        }
    }

    #[test]
    fn conflicting_roots_unknown_manifests_and_workspace_escapes_do_not_guess() {
        let modern = CargoTargetMetadata {
            edition: Some("2024".into()),
            ..Default::default()
        };
        assert!(
            catalog(modern, &["src/bin/task.rs", "src/bin/task/main.rs"])
                .uncertain
                .contains("p")
        );
        let known = BTreeSet::from(["p/src/lib.rs".into()]);
        let roots = Catalog::build(
            &BTreeMap::new(),
            &known,
            &BTreeSet::from(["p/Cargo.toml".into()]),
        );
        assert!(roots.roots.is_empty());
        assert!(roots.uncertain.contains("p"));
        for (base, path, expected) in [
            ("p", "./entry.rs", Some("p/entry.rs")),
            ("p", "../entry.rs", Some("entry.rs")),
            ("p", "../../entry.rs", None),
            ("p", "/entry.rs", None),
            ("p", r"..\entry.rs", None),
        ] {
            assert_eq!(join(base, path).as_deref(), expected);
        }
    }
}
