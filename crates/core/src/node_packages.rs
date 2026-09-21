//! Native package self-references and private import maps from authored facts.
use crate::resolve::resolve_specifier;
use graph_search_types::{
    Language, Node,
    package::{NodePackageTarget, PackageEcosystem, PackageManifest, PackageRole},
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub(crate) struct Packages {
    facts: BTreeMap<String, Option<PackageManifest>>,
    workspaces: crate::node_workspaces::Workspaces,
}

impl Packages {
    pub(crate) fn build(files: &BTreeMap<String, Node>, boundaries: &BTreeSet<String>) -> Self {
        let facts: BTreeMap<_, _> = boundaries
            .iter()
            .filter(|path| {
                Path::new(path)
                    .file_name()
                    .is_some_and(|name| name == "package.json" || name == "pnpm-workspace.yaml")
            })
            .map(|path| {
                let definition = files
                    .get(path)
                    .and_then(|node| node.attribute("package_definition"))
                    .and_then(|text| serde_json::from_str::<PackageManifest>(text).ok())
                    .filter(|definition| {
                        definition.ecosystem == PackageEcosystem::Node
                            && matches!(
                                definition.role,
                                PackageRole::Package | PackageRole::Workspace
                            )
                            && definition.node.as_ref().is_some_and(|metadata| {
                                metadata.valid() && metadata.unavailable_reason.is_none()
                            })
                    });
                (path.clone(), definition)
            })
            .collect();
        let workspaces = crate::node_workspaces::Workspaces::build(&facts);
        Self { facts, workspaces }
    }

    pub(crate) fn resolve(
        &self,
        from: &str,
        specifier: &str,
        known: &BTreeSet<String>,
    ) -> Result<String, &'static str> {
        if specifier.starts_with('.') || specifier.starts_with('/') {
            return resolve_specifier(from, specifier, known, Language::TypeScript)
                .ok_or("module_target_missing");
        }
        if (!specifier.starts_with('#') && specifier.contains([':', '#']))
            || specifier.chars().any(char::is_control)
            || specifier.contains(['\\', '%', '?', '*', '\0'])
            || specifier.split('/').any(|part| {
                part.is_empty()
                    || (matches!(part, "." | "..") || part.eq_ignore_ascii_case("node_modules"))
            })
        {
            return Err("node_specifier_unsupported");
        }
        let (mut manifest, package) = self.scope(from)?;
        let metadata = package
            .node
            .as_ref()
            .ok_or("node_package_facts_unavailable")?;
        let (map, key) = if specifier.starts_with('#') {
            (
                metadata.imports.as_ref().ok_or("node_import_map_missing")?,
                specifier.to_owned(),
            )
        } else {
            let mut parts = specifier.split('/');
            let first = parts.next().ok_or("node_specifier_unsupported")?;
            let name = if first.starts_with('@') {
                if first.len() == 1 {
                    return Err("node_specifier_unsupported");
                }
                format!(
                    "{first}/{}",
                    parts.next().ok_or("node_specifier_unsupported")?
                )
            } else {
                first.to_owned()
            };
            let tail = parts.collect::<Vec<_>>().join("/");
            let key = if tail.is_empty() {
                ".".into()
            } else {
                format!("./{tail}")
            };
            if package.name.as_deref() == Some(name.as_str()) {
                (
                    metadata
                        .exports
                        .as_ref()
                        .ok_or("node_self_export_map_missing")?,
                    key,
                )
            } else {
                let dependency = metadata
                    .dependencies
                    .get(&name)
                    .ok_or("node_dependency_not_declared")?;
                if !matches!(
                    dependency.as_str(),
                    "workspace:*" | "workspace:^" | "workspace:~"
                ) {
                    return Err("node_dependency_range_unmodeled");
                }
                manifest = self.workspaces.target(manifest, &name)?;
                let target = self
                    .facts
                    .get(manifest)
                    .and_then(Option::as_ref)
                    .and_then(|package| package.node.as_ref())
                    .ok_or("node_package_facts_unavailable")?;
                (
                    target
                        .exports
                        .as_ref()
                        .ok_or("node_workspace_export_map_missing")?,
                    key,
                )
            }
        };
        let target = map
            .get(&key)
            .ok_or(if map.keys().any(|key| key.contains('*')) {
                "node_mapping_pattern_unmodeled"
            } else {
                "node_mapping_missing"
            })?;
        resolve_target(manifest, target, known)
    }

    fn scope(&self, from: &str) -> Result<(&str, &PackageManifest), &'static str> {
        let mut directory = Path::new(from).parent();
        while let Some(path) = directory {
            let candidate = path
                .join("package.json")
                .to_string_lossy()
                .replace('\\', "/");
            if let Some((key, definition)) = self.facts.get_key_value(&candidate) {
                return definition
                    .as_ref()
                    .map(|definition| (key.as_str(), definition))
                    .ok_or("node_package_facts_unavailable");
            }
            directory = path.parent();
        }
        Err("node_package_scope_missing")
    }
}

fn resolve_target(
    manifest: &str,
    target: &NodePackageTarget,
    known: &BTreeSet<String>,
) -> Result<String, &'static str> {
    let path = match target {
        NodePackageTarget::Path(path) | NodePackageTarget::InvariantPath(path) => path,
        NodePackageTarget::Blocked => return Err("node_mapping_blocked"),
        NodePackageTarget::Unsupported => return Err("node_mapping_conditions_unmodeled"),
    };
    let relative = path
        .strip_prefix("./")
        .ok_or("node_mapping_target_unmodeled")?;
    if relative.is_empty()
        || relative.contains(['\\', '%', '?', '#', '*', '\0'])
        || relative.split('/').any(|part| {
            part.is_empty()
                || (matches!(part, "." | "..") || part.eq_ignore_ascii_case("node_modules"))
        })
    {
        return Err("node_mapping_target_unmodeled");
    }
    // Package maps name exact files. Only the existing source/runtime extension
    // substitution is available; directory and implicit-extension guessing is not.
    let target = Path::new(manifest)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(relative)
        .to_string_lossy()
        .replace('\\', "/");
    if known.contains(&target) {
        return Ok(target);
    }
    let substitute = match Path::new(&target)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("js") => &["ts", "tsx"][..],
        Some("mjs") => &["mts"][..],
        Some("cjs") => &["cts"][..],
        _ => &[],
    };
    let choices: Vec<_> = substitute
        .iter()
        .map(|extension| {
            Path::new(&target)
                .with_extension(extension)
                .to_string_lossy()
                .into_owned()
        })
        .filter(|path| known.contains(path))
        .collect();
    match choices.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err("node_mapping_target_missing"),
        _ => Err("node_mapping_target_ambiguous"),
    }
}
