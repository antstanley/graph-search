//! Native package-boundary selection over hash-bound manifest facts.
use graph_search_types::package::{
    PackageEcosystem, PackageIdentity, PackageManifest, PackageRole,
};
use graph_search_types::source::SourceFileUnits;
use graph_search_types::{Language, Node, NodeId};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn manifest_family(path: &str) -> Option<PackageEcosystem> {
    match Path::new(path).file_name()?.to_str()? {
        "Cargo.toml" => Some(PackageEcosystem::Cargo),
        "package.json" | "pnpm-workspace.yaml" => Some(PackageEcosystem::Node),
        _ => None,
    }
}

fn preferred_family(path: &str, language: Language) -> Option<PackageEcosystem> {
    manifest_family(path).or(match language {
        Language::Rust => Some(PackageEcosystem::Cargo),
        Language::TypeScript
        | Language::JavaScript
        | Language::Svelte
        | Language::Vue
        | Language::Astro => Some(PackageEcosystem::Node),
        Language::Html | Language::Css | Language::Unknown => None,
        // Python package boundaries (`pyproject.toml`, `__init__.py` packages)
        // are not modeled yet; files carry no package identity.
        Language::Python => None,
    })
}

fn valid_targets(metadata: &graph_search_types::package::CargoTargetMetadata) -> bool {
    use graph_search_types::{
        limits::{
            MAX_CARGO_TARGET_FEATURES, MAX_CARGO_TARGET_PATH_BYTES, MAX_CARGO_TARGETS,
            MAX_PACKAGE_NAME_BYTES,
        },
        package::CargoTargetKind,
    };
    let text =
        |value: &str, limit| !value.is_empty() && value.len() <= limit && !value.contains('\0');
    if let Some(reason) = &metadata.unavailable_reason {
        return text(reason, 64)
            && metadata.build_script.is_none()
            && metadata.edition.is_none()
            && !metadata.edition_workspace
            && metadata.auto_discovery.is_empty()
            && metadata.empty_target_tables.is_empty()
            && metadata.targets.is_empty();
    }
    metadata
        .edition
        .as_ref()
        .is_none_or(|edition| text(edition, 32) && !metadata.edition_workspace)
        && metadata
            .build_script
            .as_ref()
            .is_none_or(|build| match build {
                graph_search_types::package::CargoBuildScript::Path(path) => {
                    text(path, MAX_CARGO_TARGET_PATH_BYTES)
                }
                _ => true,
            })
        && !metadata.empty_target_tables.contains(&CargoTargetKind::Lib)
        && metadata
            .targets
            .iter()
            .all(|target| !metadata.empty_target_tables.contains(&target.kind))
        && metadata.targets.len() <= MAX_CARGO_TARGETS
        && metadata
            .targets
            .iter()
            .filter(|target| target.kind == CargoTargetKind::Lib)
            .count()
            <= 1
        && metadata.targets.iter().all(|target| {
            target
                .name
                .as_ref()
                .is_none_or(|name| text(name, MAX_PACKAGE_NAME_BYTES))
                && target
                    .path
                    .as_ref()
                    .is_none_or(|path| text(path, MAX_CARGO_TARGET_PATH_BYTES))
                && target.required_features.len() <= MAX_CARGO_TARGET_FEATURES
                && target
                    .required_features
                    .iter()
                    .all(|feature| text(feature, MAX_PACKAGE_NAME_BYTES))
        })
}

fn valid_definition(definition: &PackageManifest) -> bool {
    if let Some(metadata) = &definition.node
        && (definition.ecosystem != PackageEcosystem::Node
            || !matches!(
                definition.role,
                PackageRole::Package | PackageRole::Workspace
            )
            || !metadata.valid())
    {
        return false;
    }
    if let Some(metadata) = &definition.cargo_targets
        && (definition.ecosystem != PackageEcosystem::Cargo
            || definition.role != PackageRole::Package
            || !valid_targets(metadata))
    {
        return false;
    }
    if definition.name.as_ref().is_some_and(|name| {
        name.is_empty() || name.len() > graph_search_types::limits::MAX_PACKAGE_NAME_BYTES
    }) {
        return false;
    }
    match definition.role {
        PackageRole::Package => {
            definition.unavailable_reason.is_none()
                && (definition.ecosystem == PackageEcosystem::Node || definition.name.is_some())
        }
        PackageRole::Workspace => {
            (definition.ecosystem == PackageEcosystem::Cargo
                || definition.node.as_ref().is_some_and(|node| {
                    node.workspaces.is_some()
                        && node.package_manager.is_none()
                        && node.module_type.is_none()
                        && node.main.is_none()
                        && node.exports.is_none()
                        && node.imports.is_none()
                        && node.dependencies.is_empty()
                        && node.unavailable_reason.is_none()
                }))
                && definition.name.is_none()
                && definition.unavailable_reason.is_none()
        }
        PackageRole::Unavailable => {
            definition.name.is_none()
                && definition
                    .unavailable_reason
                    .as_ref()
                    .is_some_and(|reason| !reason.is_empty() && reason.len() <= 64)
        }
    }
}

fn valid_definition_path(path: &str, definition: &PackageManifest) -> bool {
    if manifest_family(path) != Some(definition.ecosystem) || !valid_definition(definition) {
        return false;
    }
    if definition.ecosystem == PackageEcosystem::Node {
        let pnpm = Path::new(path)
            .file_name()
            .is_some_and(|name| name == "pnpm-workspace.yaml");
        match definition.role {
            PackageRole::Package => !pnpm,
            PackageRole::Workspace => pnpm,
            PackageRole::Unavailable => true,
        }
    } else {
        true
    }
}

#[derive(Default)]
pub(crate) struct Catalog {
    manifests: BTreeMap<String, (String, PackageManifest)>,
}

impl Catalog {
    pub(crate) fn new(known: &BTreeSet<String>) -> Self {
        Self {
            manifests: known
                .iter()
                .filter_map(|path| {
                    let ecosystem = manifest_family(path)?;
                    Some((
                        path.clone(),
                        (
                            String::new(),
                            PackageManifest {
                                node: None,
                                cargo_targets: None,
                                ecosystem,
                                role: PackageRole::Unavailable,
                                name: None,
                                unavailable_reason: Some("manifest_metadata_unavailable".into()),
                            },
                        ),
                    ))
                })
                .collect(),
        }
    }

    pub(crate) fn add(&mut self, path: &str, source: &SourceFileUnits) {
        if let Some(definition) = &source.package_manifest
            && manifest_family(path) == Some(definition.ecosystem)
            && self.manifests.contains_key(path)
        {
            self.manifests.insert(
                path.into(),
                (source.source_hash.clone(), definition.clone()),
            );
        }
    }

    fn context(&self, path: &str, language: Language) -> (Option<PackageIdentity>, bool) {
        let preferred = preferred_family(path, language);
        let mut directory = Path::new(path).parent();
        while let Some(dir) = directory {
            let candidates: Vec<_> = ["Cargo.toml", "package.json"]
                .into_iter()
                .filter_map(|name| {
                    let candidate = dir.join(name).to_string_lossy().replace('\\', "/");
                    let (hash, definition) = self.manifests.get(&candidate)?;
                    preferred
                        .is_none_or(|family| family == definition.ecosystem)
                        .then_some((candidate, hash, definition))
                })
                .collect();
            match candidates.as_slice() {
                [(manifest_path, hash, definition)] => {
                    return match definition.role {
                        PackageRole::Package => (
                            Some(PackageIdentity {
                                manifest_path: manifest_path.clone(),
                                manifest_hash: (*hash).clone(),
                                ecosystem: definition.ecosystem,
                                name: definition.name.clone(),
                            }),
                            false,
                        ),
                        PackageRole::Workspace => (None, false),
                        PackageRole::Unavailable => (None, true),
                    };
                }
                [] => {
                    // A pnpm workspace root (`pnpm-workspace.yaml`) with no sibling
                    // `package.json` is still a hard boundary: files beneath it do
                    // not belong to any ancestor package. Only Node resolution stops
                    // here; a Rust file keeps ascending to its Cargo owner.
                    if preferred.is_none_or(|family| family == PackageEcosystem::Node)
                        && let Some(incomplete) = self.pnpm_boundary(dir)
                    {
                        return (None, incomplete);
                    }
                    directory = dir.parent();
                }
                _ => return (None, true),
            }
        }
        (None, false)
    }

    /// A `pnpm-workspace.yaml` marks a workspace root even without a sibling
    /// `package.json`. Returns `Some(incomplete)` when `dir` is such a boundary;
    /// `incomplete` is `true` only when the marker's own metadata is unavailable.
    fn pnpm_boundary(&self, dir: &Path) -> Option<bool> {
        let candidate = dir
            .join("pnpm-workspace.yaml")
            .to_string_lossy()
            .replace('\\', "/");
        let (_, definition) = self.manifests.get(&candidate)?;
        (definition.ecosystem == PackageEcosystem::Node)
            .then_some(!matches!(definition.role, PackageRole::Workspace))
    }

    pub(crate) fn annotate(&self, file: &mut Node, source: Option<&mut SourceFileUnits>) {
        let (package, incomplete) =
            self.context(&file.path, file.language.unwrap_or(Language::Unknown));
        file.attributes.remove("package_context");
        file.attributes.remove("package_definition");
        file.attributes.remove("package_scope_incomplete");
        if let Some(package) = &package
            && let Ok(encoded) = serde_json::to_string(package)
        {
            file.attributes.insert("package_context".into(), encoded);
        }
        if incomplete {
            file.attributes
                .insert("package_scope_incomplete".into(), "true".into());
        }
        if let Some(source) = source {
            if let Some(definition) = &source.package_manifest
                && let Ok(encoded) = serde_json::to_string(definition)
            {
                file.attributes.insert("package_definition".into(), encoded);
            }
            source.package = package;
            source.package_scope_incomplete = incomplete;
        }
    }
}

pub(crate) fn valid<'a>(
    file: &Node,
    source: &SourceFileUnits,
    lookup: &impl Fn(&NodeId) -> Option<&'a Node>,
) -> bool {
    if source.package_scope_incomplete
        != (file.attribute("package_scope_incomplete") == Some("true"))
        || source
            .package_manifest
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .as_deref()
            != file.attribute("package_definition")
        || source
            .package
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .as_deref()
            != file.attribute("package_context")
    {
        return false;
    }
    if let Some(definition) = &source.package_manifest
        && !valid_definition_path(&file.path, definition)
    {
        return false;
    }
    let Some(package) = &source.package else {
        return true;
    };
    if source.package_scope_incomplete
        || manifest_family(&package.manifest_path) != Some(package.ecosystem)
        || preferred_family(&file.path, file.language.unwrap_or(Language::Unknown))
            .is_some_and(|family| family != package.ecosystem)
    {
        return false;
    }
    let Some(directory) = Path::new(&package.manifest_path).parent() else {
        return false;
    };
    if !Path::new(&file.path).starts_with(directory) {
        return false;
    }
    let Some(manifest) = lookup(&NodeId::file(&package.manifest_path)) else {
        return false;
    };
    let definition = manifest
        .attribute("package_definition")
        .and_then(|text| serde_json::from_str::<PackageManifest>(text).ok());
    manifest.is_file()
        && manifest.path == package.manifest_path
        && manifest.content_hash.as_deref() == Some(package.manifest_hash.as_str())
        && definition.is_some_and(|definition| {
            valid_definition_path(&manifest.path, &definition)
                && definition.role == PackageRole::Package
                && definition.ecosystem == package.ecosystem
                && definition.name == package.name
        })
}

/// Intern only identities repeated among selected seeds. Full path/hash/family/name
/// equality prevents same-named packages or different generations from conflating.
pub(crate) fn share_result_identities<'a>(
    evidence: &mut BTreeMap<NodeId, graph_search_types::source::SourceEvidence>,
    selected: impl IntoIterator<Item = &'a NodeId>,
    context: &mut graph_search_types::context::ResultContext,
) {
    let mut groups: BTreeMap<PackageIdentity, BTreeSet<NodeId>> = BTreeMap::new();
    for id in selected {
        if let Some(identity) = evidence.get(id).and_then(|item| item.package.as_ref()) {
            groups
                .entry(identity.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    context.packages.clear();
    for (identity, ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        let key = format!("p{}", context.packages.len());
        context.packages.insert(key.clone(), identity);
        for id in ids {
            if let Some(item) = evidence.get_mut(&id) {
                item.package = None;
                item.package_ref = Some(key.clone());
            }
        }
    }
}

#[cfg(test)]
mod result_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{
        Span,
        context::ResultContext,
        source::{SourceEvidence, SourceUnitKind},
    };

    fn evidence(identity: PackageIdentity) -> SourceEvidence {
        SourceEvidence {
            package_scope_incomplete: false,
            span: Span::default(),
            kind: SourceUnitKind::Code,
            owner: None,
            documentation: None,
            match_line: 1,
            source_hash: "source".into(),
            package: Some(identity),
            package_ref: None,
            live: false,
        }
    }

    #[test]
    fn result_sharing_preserves_full_identity_and_stable_references_when_trimmed() {
        let identity = PackageIdentity {
            manifest_path: "Cargo.toml".into(),
            manifest_hash: "a".repeat(64),
            ecosystem: PackageEcosystem::Cargo,
            name: Some("same-name".into()),
        };
        let other_path = PackageIdentity {
            manifest_path: "nested/Cargo.toml".into(),
            ..identity.clone()
        };
        let other_hash = PackageIdentity {
            manifest_hash: "b".repeat(64),
            ..identity.clone()
        };
        let mut records: BTreeMap<_, _> = [
            ("a", identity.clone()),
            ("b", identity),
            ("c", other_path.clone()),
            ("d", other_hash.clone()),
            ("e", other_hash),
            ("unselected", other_path),
        ]
        .into_iter()
        .map(|(id, package)| (NodeId::new(id), evidence(package)))
        .collect();
        let original = records.clone();
        let selected: Vec<_> = ["a", "b", "c", "d", "e"].map(NodeId::new).into();
        let mut context = ResultContext::default();
        let before = serde_json::to_vec(&(&records, &context)).unwrap().len();
        share_result_identities(&mut records, selected.iter(), &mut context);
        assert_eq!(context.packages.len(), 2);
        assert!(serde_json::to_vec(&(&records, &context)).unwrap().len() < before);
        for id in &selected {
            assert_eq!(
                records[id].package_identity(&context),
                original[id].package.as_ref()
            );
        }
        assert_eq!(records[&NodeId::new("c")], original[&NodeId::new("c")]);
        assert_eq!(
            records[&NodeId::new("unselected")],
            original[&NodeId::new("unselected")]
        );
        let mut reversed = original.clone();
        let mut reversed_context = ResultContext::default();
        share_result_identities(&mut reversed, selected.iter().rev(), &mut reversed_context);
        assert_eq!(records, reversed);
        assert_eq!(context, reversed_context);
        let encoded = serde_json::to_vec(&(&records, &context)).unwrap();
        let (decoded, decoded_context): (BTreeMap<NodeId, SourceEvidence>, ResultContext) =
            serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, records);
        assert_eq!(decoded_context, context);
        let survivor = &records[&NodeId::new("d")];
        context.retain_packages(survivor.package_ref.iter().map(String::as_str));
        assert_eq!(context.packages.len(), 1);
        assert_eq!(
            survivor.package_identity(&context),
            original[&NodeId::new("d")].package.as_ref()
        );
        assert!(
            records[&NodeId::new("a")]
                .package_identity(&context)
                .is_none()
        );
        let mut contradictory = survivor.clone();
        contradictory.package = original[&NodeId::new("a")].package.clone();
        assert!(contradictory.package_identity(&context).is_none());
        // Historical inline evidence without the new field remains readable.
        let mut legacy = serde_json::to_value(&original[&NodeId::new("a")]).unwrap();
        legacy.as_object_mut().unwrap().remove("package_ref");
        let legacy: SourceEvidence = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            legacy.package_identity(&ResultContext::default()),
            legacy.package.as_ref()
        );
    }
}

#[cfg(test)]
mod pnpm_boundary_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn manifest(role: PackageRole, name: Option<&str>) -> (String, PackageManifest) {
        (
            "hash".into(),
            PackageManifest {
                node: None,
                cargo_targets: None,
                ecosystem: PackageEcosystem::Node,
                role,
                name: name.map(Into::into),
                unavailable_reason: None,
            },
        )
    }

    #[test]
    fn pnpm_workspace_yaml_bounds_the_nearest_package_walk() {
        // An outer package, a nested pnpm workspace root with no sibling
        // `package.json`, and a sub-package beneath it.
        let catalog = Catalog {
            manifests: BTreeMap::from([
                ("package.json".into(), manifest(PackageRole::Package, Some("workspace-tools"))),
                ("subproject/pnpm-workspace.yaml".into(), manifest(PackageRole::Workspace, None)),
                (
                    "subproject/packages/foo/package.json".into(),
                    manifest(PackageRole::Package, Some("foo")),
                ),
            ]),
        };
        // A file under the pnpm root (not in a sub-package) must NOT be attributed
        // to the outer package: the workspace boundary stops the walk.
        assert_eq!(
            catalog.context("subproject/tools/build.ts", Language::TypeScript),
            (None, false)
        );
        // A file inside a sub-package still resolves to that package.
        let (foo, incomplete) =
            catalog.context("subproject/packages/foo/index.ts", Language::TypeScript);
        assert!(!incomplete);
        assert_eq!(foo.unwrap().name.as_deref(), Some("foo"));
        // A file under the outer root still resolves to the outer package.
        let (outer, _) = catalog.context("app.ts", Language::TypeScript);
        assert_eq!(outer.unwrap().name.as_deref(), Some("workspace-tools"));
        // A Rust file ignores the pnpm (Node) boundary entirely.
        assert_eq!(
            catalog.context("subproject/tools/build.rs", Language::Rust),
            (None, false)
        );
    }
}

#[cfg(test)]
mod target_validation_tests {
    use super::*;
    use graph_search_types::package::{CargoTarget, CargoTargetKind, CargoTargetMetadata};

    #[test]
    fn persisted_targets_reject_partial_unavailable_and_malformed_metadata() {
        let base = CargoTargetMetadata::default();
        assert!(valid_targets(&base));
        let target = CargoTarget {
            kind: CargoTargetKind::Lib,
            name: None,
            path: Some("../custom.rs".into()),
            required_features: vec![],
        };
        let valid = CargoTargetMetadata {
            targets: vec![target.clone()],
            ..base.clone()
        };
        assert!(valid_targets(&valid));
        for invalid in [
            CargoTargetMetadata {
                edition: Some("2024".into()),
                edition_workspace: true,
                ..base.clone()
            },
            CargoTargetMetadata {
                unavailable_reason: Some("invalid_target_string".into()),
                ..valid.clone()
            },
            CargoTargetMetadata {
                targets: vec![target.clone(), target.clone()],
                ..base.clone()
            },
            CargoTargetMetadata {
                targets: vec![CargoTarget {
                    path: Some("a\0b".into()),
                    ..target.clone()
                }],
                ..base.clone()
            },
            CargoTargetMetadata {
                targets: vec![CargoTarget {
                    required_features: vec!["x".into(); 65],
                    ..target.clone()
                }],
                ..base.clone()
            },
            CargoTargetMetadata {
                edition: Some(String::new()),
                ..base.clone()
            },
        ] {
            assert!(!valid_targets(&invalid));
        }
        for invalid in [
            CargoTargetMetadata {
                empty_target_tables: BTreeSet::from([CargoTargetKind::Lib]),
                ..base.clone()
            },
            CargoTargetMetadata {
                build_script: Some(graph_search_types::package::CargoBuildScript::Path(
                    String::new(),
                )),
                ..base.clone()
            },
            CargoTargetMetadata {
                build_script: Some(graph_search_types::package::CargoBuildScript::Default),
                unavailable_reason: Some("invalid".into()),
                ..base.clone()
            },
            CargoTargetMetadata {
                empty_target_tables: BTreeSet::from([CargoTargetKind::Bin]),
                unavailable_reason: Some("invalid".into()),
                ..base.clone()
            },
        ] {
            assert!(!valid_targets(&invalid));
        }
        let mut definition = PackageManifest {
            node: None,
            ecosystem: PackageEcosystem::Cargo,
            role: PackageRole::Package,
            name: Some("native".into()),
            unavailable_reason: None,
            cargo_targets: Some(valid),
        };
        assert!(valid_definition(&definition));
        definition.ecosystem = PackageEcosystem::Node;
        assert!(!valid_definition(&definition));
        definition.ecosystem = PackageEcosystem::Cargo;
        definition.role = PackageRole::Workspace;
        definition.name = None;
        assert!(!valid_definition(&definition));
    }
}

#[cfg(test)]
mod node_metadata_tests {
    use super::*;
    use graph_search_types::package::{NodePackageMetadata, NodePackageTarget};
    #[test]
    fn workspace_metadata_requires_its_own_manifest_role_and_complete_fields() {
        let mut definition = PackageManifest {
            ecosystem: PackageEcosystem::Node,
            role: PackageRole::Workspace,
            name: None,
            unavailable_reason: None,
            cargo_targets: None,
            node: Some(NodePackageMetadata {
                workspaces: Some(vec!["packages/*".into()]),
                ..NodePackageMetadata::default()
            }),
        };
        assert!(valid_definition_path("pnpm-workspace.yaml", &definition));
        assert!(!valid_definition_path("package.json", &definition));
        definition.role = PackageRole::Package;
        assert!(!valid_definition_path("pnpm-workspace.yaml", &definition));
        definition.role = PackageRole::Workspace;
        definition.node.as_mut().unwrap().main = Some(NodePackageTarget::Path("./index.js".into()));
        assert!(!valid_definition_path("pnpm-workspace.yaml", &definition));
    }
    #[test]
    fn persisted_node_metadata_rejects_partial_unavailable_and_invalid_keys() {
        let mut definition = PackageManifest {
            ecosystem: PackageEcosystem::Node,
            role: PackageRole::Package,
            name: Some("pkg".into()),
            unavailable_reason: None,
            cargo_targets: None,
            node: Some(NodePackageMetadata::default()),
        };
        assert!(valid_definition(&definition));
        definition.ecosystem = PackageEcosystem::Cargo;
        assert!(!valid_definition(&definition));
        definition.ecosystem = PackageEcosystem::Node;
        for metadata in [
            NodePackageMetadata {
                unavailable_reason: Some("unavailable".into()),
                main: Some(NodePackageTarget::Path("./index.js".into())),
                ..Default::default()
            },
            NodePackageMetadata {
                exports: Some(BTreeMap::from([(
                    "invalid".into(),
                    NodePackageTarget::Blocked,
                )])),
                ..Default::default()
            },
            NodePackageMetadata {
                imports: Some(BTreeMap::from([(
                    "#/invalid".into(),
                    NodePackageTarget::Blocked,
                )])),
                ..Default::default()
            },
            NodePackageMetadata {
                module_type: Some("unknown".into()),
                ..Default::default()
            },
            NodePackageMetadata {
                main: Some(NodePackageTarget::Path("a".repeat(4097))),
                ..Default::default()
            },
        ] {
            definition.node = Some(metadata);
            assert!(!valid_definition(&definition));
        }
    }
}
