//! Manifest-owned package boundaries, independent of graph declarations.
use serde::{Deserialize, Serialize};

/// Native manifest family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageEcosystem {
    /// Cargo.toml package/workspace declarations.
    Cargo,
    /// package.json boundaries for JavaScript/TypeScript projects.
    Node,
    /// pyproject.toml project boundaries for Python.
    Python,
}

/// What a recognized manifest establishes in the supported subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageRole {
    /// An authored package boundary, possibly unnamed for package.json or
    /// pyproject.toml.
    Package,
    /// A workspace declaration without its own package (Cargo or pnpm).
    Workspace,
    /// Invalid, unsupported or over-budget metadata; blocks outer inheritance.
    Unavailable,
}

/// Small raw manifest facts tied to the containing source record's hash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    /// Authored Node entry points and workspace declarations; absent in legacy facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodePackageMetadata>,
    /// Manifest syntax family.
    pub ecosystem: PackageEcosystem,
    /// Declared boundary role.
    pub role: PackageRole,
    /// Authored name; no dependency installation or package-name resolution implied.
    pub name: Option<String>,
    /// Fixed diagnostic code for unavailable metadata, without source text.
    pub unavailable_reason: Option<String>,
    /// Authored Cargo targets; absent for other ecosystems or legacy facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo_targets: Option<CargoTargetMetadata>,
}

/// Identity of a package boundary in the selected source generation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PackageIdentity {
    /// Workspace-relative manifest path; distinct paths are distinct packages.
    pub manifest_path: String,
    /// Hash of the manifest bytes that established this association.
    pub manifest_hash: String,
    /// Manifest syntax family.
    pub ecosystem: PackageEcosystem,
    /// Authored package name, when present.
    pub name: Option<String>,
}

/// Authored Cargo target table kind; this does not imply that a file was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoTargetKind {
    /// `[lib]`.
    Lib,
    /// `[[bin]]`.
    Bin,
    /// `[[example]]`.
    Example,
    /// `[[test]]`.
    Test,
    /// `[[bench]]`.
    Bench,
}

/// One authored target, preserving omitted names/paths instead of guessing roots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoTarget {
    /// Cargo target table family.
    pub kind: CargoTargetKind,
    /// Authored target name, if present.
    pub name: Option<String>,
    /// Authored path, without filesystem access, normalization or expansion.
    pub path: Option<String>,
    /// Authored feature gates; no active build configuration is inferred.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_features: Vec<String>,
}

/// Authored build-script setting; discovering a script never executes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoBuildScript {
    /// `build = false`.
    Disabled,
    /// `build = true`.
    Default,
    /// An explicit manifest-relative script path.
    Path(String),
}

/// Bounded raw Cargo target facts, independent of package-boundary identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoTargetMetadata {
    /// Explicit package build-script setting; absence retains Cargo discovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_script: Option<CargoBuildScript>,
    /// Explicit edition, if present. No default is inferred here.
    pub edition: Option<String>,
    /// Whether edition is explicitly inherited from the workspace.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edition_workspace: bool,
    /// Explicit auto-discovery switches; absent keys remain unspecified.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub auto_discovery: std::collections::BTreeMap<CargoTargetKind, bool>,
    /// Explicit empty target arrays, significant for edition-2015 discovery defaults.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub empty_target_tables: std::collections::BTreeSet<CargoTargetKind>,
    /// Explicit target tables in deterministic kind/table order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<CargoTarget>,
    /// Fixed diagnostic if the entire target projection is unavailable.
    /// Unavailable projections contain no partial target/settings data.
    pub unavailable_reason: Option<String>,
}

/// An authored package-map target without selecting runtime conditions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePackageTarget {
    /// Raw authored path or package specifier; resolution validates its meaning.
    Path(String),
    /// Conditional object with a default and the same path under every branch.
    /// No runtime condition was selected.
    InvariantPath(String),
    /// Explicit null target, which blocks this mapping.
    Blocked,
    /// Condition-dependent, array, or otherwise unsupported target; never a fallback path.
    Unsupported,
}

/// Bounded authored Node package metadata. No dependency installation is implied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePackageMetadata {
    /// Override selectors that may redirect workspace dependencies; targets are not executed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_overrides: Vec<String>,
    /// Explicit manager declaration, used to avoid treating pnpm package.json workspaces as membership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    /// Explicit module type, when authored.
    pub module_type: Option<String>,
    /// Legacy main entry, kept separately from the authoritative exports map.
    pub main: Option<NodePackageTarget>,
    /// None means absent; Some(empty) remains an explicitly empty export map.
    pub exports: Option<std::collections::BTreeMap<String, NodePackageTarget>>,
    /// Package-private specifier mappings, including unsupported branches.
    pub imports: Option<std::collections::BTreeMap<String, NodePackageTarget>>,
    /// Authored workspace patterns; no matching paths are inferred here.
    pub workspaces: Option<Vec<String>>,
    /// Authored dependency specifiers across dependency tables; conflicts invalidate metadata.
    pub dependencies: std::collections::BTreeMap<String, String>,
    /// Entire projection unavailable; all other fields must then be empty.
    pub unavailable_reason: Option<String>,
}

impl NodePackageMetadata {
    /// Check the persisted representation independently of the JSON adapter.
    #[must_use]
    pub fn valid(&self) -> bool {
        fn target_text(target: &NodePackageTarget) -> Option<&str> {
            match target {
                NodePackageTarget::Path(path) | NodePackageTarget::InvariantPath(path) => {
                    Some(path.as_str())
                }
                _ => None,
            }
        }

        if let Some(reason) = &self.unavailable_reason {
            return !reason.is_empty()
                && reason.len() <= 64
                && !reason.chars().any(char::is_control)
                && self.workspace_overrides.is_empty()
                && self.package_manager.is_none()
                && self.module_type.is_none()
                && self.main.is_none()
                && self.exports.is_none()
                && self.imports.is_none()
                && self.workspaces.is_none()
                && self.dependencies.is_empty();
        }
        if self
            .module_type
            .as_deref()
            .is_some_and(|kind| !matches!(kind, "module" | "commonjs"))
        {
            return false;
        }
        let mut records = 0usize;
        let mut bytes = 0usize;
        let mut admit = |text: &str| {
            records = records.saturating_add(1);
            bytes = bytes.saturating_add(text.len());
            !text.is_empty()
                && text.len() <= 4096
                && !text.chars().any(char::is_control)
                && records <= 4096
                && bytes <= crate::limits::MAX_PACKAGE_MANIFEST_BYTES
        };
        if self
            .package_manager
            .as_ref()
            .is_some_and(|value| !admit(value))
        {
            return false;
        }
        if let Some(main) = self.main.as_ref().and_then(target_text)
            && !admit(main)
        {
            return false;
        }
        for (map, exports) in [
            (self.exports.as_ref(), true),
            (self.imports.as_ref(), false),
        ] {
            let Some(map) = map else { continue };
            for (key, target) in map {
                let valid_key = if exports {
                    key == "." || key.starts_with("./")
                } else {
                    key.starts_with('#') && key.len() > 1 && !key.starts_with("#/")
                };
                if !valid_key || !admit(key) || target_text(target).is_some_and(|path| !admit(path))
                {
                    return false;
                }
            }
        }
        for pattern in self
            .workspaces
            .iter()
            .flatten()
            .chain(&self.workspace_overrides)
        {
            if !admit(pattern) {
                return false;
            }
        }
        for (name, specifier) in &self.dependencies {
            if !admit(name) || !admit(specifier) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod node_tests {
    use super::*;
    #[test]
    fn node_facts_roundtrip_and_legacy_manifests_do_not_invent_metadata() {
        let legacy: PackageManifest = serde_json::from_str(
            r#"{"ecosystem":"node","role":"package","name":"pkg","unavailable_reason":null}"#,
        )
        .unwrap();
        assert!(legacy.node.is_none());
        let mut current = legacy;
        current.node = Some(NodePackageMetadata {
            exports: Some(std::collections::BTreeMap::from([(
                ".".into(),
                NodePackageTarget::Blocked,
            )])),
            workspaces: Some(Vec::new()),
            ..NodePackageMetadata::default()
        });
        let encoded = serde_json::to_vec(&current).unwrap();
        assert_eq!(
            serde_json::from_slice::<PackageManifest>(&encoded).unwrap(),
            current
        );
        assert!(current.node.unwrap().valid());
    }
}
