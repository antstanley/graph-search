//! Native workspace membership and unique package identity. No installed tree lookup.
use graph_search_types::package::PackageManifest;
use std::collections::BTreeMap;
use std::path::Path;

type Facts = BTreeMap<String, Option<PackageManifest>>;
type Decision = Result<String, &'static str>;

#[derive(Clone, Debug, Default)]
pub(crate) struct Workspaces {
    owners: BTreeMap<String, Decision>,
    names: BTreeMap<String, BTreeMap<String, Option<String>>>,
    incomplete: BTreeMap<String, &'static str>,
    exhausted: bool,
    overridden: BTreeMap<String, std::collections::BTreeSet<String>>,
}

fn declaration<'a>(
    facts: &'a Facts,
    manifest: &str,
    work: &mut usize,
    skip_unavailable: bool,
) -> Result<(&'a str, &'a [String]), &'static str> {
    // pnpm membership comes only from its workspace file, even if a child
    // package.json also happens to contain a workspaces field.
    for name in ["pnpm-workspace.yaml", "package.json"] {
        let mut directory = Path::new(manifest).parent();
        while let Some(dir) = directory {
            directory = dir.parent();
            *work = work.checked_sub(1).ok_or("node_workspace_work_limit")?;
            let candidate = dir.join(name).to_string_lossy().replace('\\', "/");
            let Some((path, value)) = facts.get_key_value(&candidate) else {
                continue;
            };
            let Some(metadata) = value.as_ref().and_then(|value| value.node.as_ref()) else {
                if skip_unavailable {
                    continue;
                }
                return Err("node_workspace_facts_unavailable");
            };
            if name == "package.json"
                && metadata
                    .package_manager
                    .as_deref()
                    .is_some_and(|manager| manager == "pnpm" || manager.starts_with("pnpm@"))
            {
                return Err("node_workspace_missing");
            }
            if let Some(patterns) = &metadata.workspaces {
                return Ok((path, patterns));
            }
            if name == "pnpm-workspace.yaml" {
                return Err("node_workspace_facts_unavailable");
            }
        }
    }
    Err("node_workspace_missing")
}

fn segments(pattern: &str) -> Option<Vec<&str>> {
    let pattern = pattern.strip_prefix('!').unwrap_or(pattern);
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    if pattern.is_empty() || pattern.starts_with('/') || pattern.ends_with('/') {
        return None;
    }
    let parts: Vec<_> = pattern.split('/').collect();
    parts
        .iter()
        .all(|part| {
            !part.is_empty()
                && !matches!(*part, "." | "..")
                && !part.eq_ignore_ascii_case("node_modules")
                && (matches!(*part, "*" | "**")
                    || !part.contains(['*', '?', '[', ']', '{', '}', '\\', '!', '#', '%']))
                && !part.chars().any(char::is_whitespace)
        })
        .then_some(parts)
}

/// Dynamic programming over whole path segments, charging each transition.
fn matches(pattern: &[&str], path: &[&str], work: &mut usize) -> Option<bool> {
    let length = path.len().checked_add(1)?;
    let mut previous = vec![false; length];
    previous[0] = true;
    for part in pattern {
        let mut current = vec![false; length];
        current[0] = *part == "**" && previous[0];
        for (i, component) in path.iter().enumerate() {
            *work = work.checked_sub(1)?;
            let next = i.saturating_add(1);
            current[next] = if *part == "**" {
                previous[next] || (current[i] && !component.starts_with('.'))
            } else {
                previous[i] && ((*part == "*" && !component.starts_with('.')) || part == component)
            };
        }
        previous = current;
    }
    previous.last().copied()
}

fn member(patterns: &[String], relative: &str, work: &mut usize) -> Result<bool, &'static str> {
    let compiled: Vec<_> = patterns
        .iter()
        .map(|pattern| {
            *work = work
                .checked_sub(pattern.len())
                .ok_or("node_workspace_work_limit")?;
            segments(pattern).ok_or("node_workspace_pattern_unmodeled")
        })
        .collect::<Result<_, _>>()?;
    if relative.is_empty() {
        return Ok(true);
    }
    let path: Vec<_> = relative.split('/').collect();
    let mut included = false;
    let mut excluded = false;
    for (original, pattern) in patterns.iter().zip(compiled) {
        *work = work.checked_sub(1).ok_or("node_workspace_work_limit")?;
        if matches(&pattern, &path, work).ok_or("node_workspace_work_limit")? {
            if original.starts_with('!') {
                excluded = true;
            } else {
                included = true;
            }
        }
    }
    Ok(included && !excluded)
}

fn admitted(
    declaration: &str,
    manifest: &str,
    patterns: &[String],
    work: &mut usize,
) -> Result<bool, &'static str> {
    let root = Path::new(declaration)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let directory = Path::new(manifest)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| "node_workspace_scope_mismatch")?
        .to_string_lossy()
        .replace('\\', "/");
    member(patterns, &relative, work)
}

impl Workspaces {
    pub(crate) fn build(facts: &Facts) -> Self {
        Self::build_with_work(facts, graph_search_types::limits::MAX_NODE_WORKSPACE_WORK)
    }

    fn build_with_work(facts: &Facts, mut work: usize) -> Self {
        let mut result = Self::default();
        for (manifest, definition) in facts {
            if Path::new(manifest)
                .file_name()
                .is_none_or(|name| name != "package.json")
            {
                continue;
            }
            let owner = declaration(facts, manifest, &mut work, false).and_then(
                |(declaration, patterns)| {
                    if admitted(declaration, manifest, patterns, &mut work)? {
                        Ok(declaration.to_owned())
                    } else {
                        Err("node_workspace_member_excluded")
                    }
                },
            );
            if owner == Err("node_workspace_facts_unavailable") {
                // A missing projection may conceal a duplicate name or nested
                // workspace. Never infer uniqueness by omitting that package.
                match declaration(facts, manifest, &mut work, true) {
                    Ok((root, patterns)) => match admitted(root, manifest, patterns, &mut work) {
                        Ok(true) => {
                            result
                                .incomplete
                                .insert(root.into(), "node_workspace_member_unavailable");
                        }
                        Err("node_workspace_work_limit") => result.exhausted = true,
                        _ => {}
                    },
                    Err("node_workspace_work_limit") => result.exhausted = true,
                    Err(_) => {}
                }
            }
            if let Ok(root) = &owner {
                if let Some(package) = definition {
                    if let Some(name) = &package.name {
                        result
                            .names
                            .entry(root.clone())
                            .or_default()
                            .entry(name.clone())
                            .and_modify(|value| *value = None)
                            .or_insert_with(|| Some(manifest.clone()));
                    }
                } else {
                    result
                        .incomplete
                        .insert(root.clone(), "node_workspace_member_unavailable");
                }
            }
            if owner == Err("node_workspace_work_limit") {
                result.exhausted = true;
            }
            result.owners.insert(manifest.clone(), owner);
            if result.exhausted {
                break;
            }
        }
        if !result.exhausted {
            result.override_barriers(facts, &mut work);
        }
        result
    }

    fn override_barriers(&mut self, facts: &Facts, work: &mut usize) {
        for (root, names) in &self.names {
            let package = Path::new(root)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join("package.json")
                .to_string_lossy()
                .replace('\\', "/");
            let selectors: std::collections::BTreeSet<_> = [root.as_str(), package.as_str()]
                .into_iter()
                .filter_map(|path| {
                    facts
                        .get(path)
                        .and_then(Option::as_ref)
                        .and_then(|fact| fact.node.as_ref())
                })
                .flat_map(|node| &node.workspace_overrides)
                .collect();
            for name in names.keys() {
                for selector in &selectors {
                    let Some(remaining) = work.checked_sub(selector.len().saturating_add(1)) else {
                        self.exhausted = true;
                        return;
                    };
                    *work = remaining;
                    if selector_affects(selector, name) {
                        self.overridden
                            .entry(root.clone())
                            .or_default()
                            .insert(name.clone());
                        break;
                    }
                }
            }
        }
    }

    pub(crate) fn target(&self, from: &str, name: &str) -> Result<&str, &'static str> {
        if self.exhausted {
            return Err("node_workspace_work_limit");
        }
        let root = self
            .owners
            .get(from)
            .ok_or("node_workspace_missing")?
            .as_ref()
            .map_err(|reason| *reason)?;
        if self
            .overridden
            .get(root)
            .is_some_and(|names| names.contains(name))
        {
            return Err("node_workspace_override_unmodeled");
        }
        if let Some(reason) = self.incomplete.get(root) {
            return Err(reason);
        }
        self.names
            .get(root)
            .and_then(|names| names.get(name))
            .ok_or("node_workspace_target_missing")?
            .as_deref()
            .ok_or("node_workspace_target_ambiguous")
    }
}

fn selector_affects(selector: &str, name: &str) -> bool {
    let child = selector.rsplit('>').next().unwrap_or(selector).trim();
    let package = if let Some(scoped) = child.strip_prefix('@') {
        scoped
            .find('@')
            .map_or(child, |offset| &child[..offset.saturating_add(1)])
    } else {
        child.split('@').next().unwrap_or(child)
    };
    let component = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    };
    let valid = if let Some(scoped) = package.strip_prefix('@') {
        scoped
            .split_once('/')
            .is_some_and(|(scope, part)| component(scope) && component(part))
    } else {
        component(package)
    };
    !valid || package == name
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn override_selectors_cannot_silently_redirect_workspace_identity() {
        assert!(selector_affects("@scope/api", "@scope/api"));
        assert!(selector_affects("@scope/api@^1", "@scope/api"));
        assert!(selector_affects("parent@1>@scope/api", "@scope/api"));
        assert!(!selector_affects("@scope/other@^1", "@scope/api"));
        assert!(selector_affects("**/api", "@scope/api"));
    }

    #[test]
    fn exhaustion_never_exposes_a_partially_collected_name_map() {
        use graph_search_types::package::{NodePackageMetadata, PackageEcosystem, PackageRole};
        let package = |name: &str, patterns| {
            Some(PackageManifest {
                ecosystem: PackageEcosystem::Node,
                role: PackageRole::Package,
                name: Some(name.into()),
                unavailable_reason: None,
                cargo_targets: None,
                node: Some(NodePackageMetadata {
                    workspaces: patterns,
                    ..NodePackageMetadata::default()
                }),
            })
        };
        let facts = BTreeMap::from([
            (
                "package.json".into(),
                package("root", Some(vec!["**".into()])),
            ),
            ("z/api/package.json".into(), package("api", None)),
        ]);
        let partial = Workspaces::build_with_work(&facts, 5);
        assert!(
            partial
                .names
                .get("package.json")
                .unwrap()
                .contains_key("root")
        );
        assert_eq!(
            partial.target("package.json", "root"),
            Err("node_workspace_work_limit")
        );
    }
    #[test]
    fn segment_globs_and_exclusions_are_complete_and_bounded() {
        let patterns = vec!["packages/**".into(), "!packages/**/test".into()];
        assert!(member(&patterns, "packages/core", &mut 1000).unwrap());
        assert!(!member(&patterns, "packages/core/test", &mut 1000).unwrap());
        assert!(!member(&patterns, "other/core", &mut 1000).unwrap());
        assert!(!member(&patterns, "packages/.hidden", &mut 1000).unwrap());
        assert!(member(&["packages/.hidden".into()], "packages/.hidden", &mut 1000).unwrap());
        assert_eq!(
            member(&patterns, "packages/core", &mut 0),
            Err("node_workspace_work_limit")
        );
        assert_eq!(
            member(&["packages/{a,b}".into()], "packages/a", &mut 1000),
            Err("node_workspace_pattern_unmodeled")
        );
    }
}
