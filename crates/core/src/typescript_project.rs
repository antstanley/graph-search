//! Native default TypeScript project selection for declared path aliases.
//!
//! A file resolves through the nearest admitted `tsconfig.json`/`jsconfig.json`
//! whose directory contains it. The selected configuration's bounded inheritance
//! chain and `paths`/`baseUrl` settings then feed the existing alias dispatcher
//! and module-mode file loader. This is deliberately a supported subset: an
//! unsupported resolution mode, missing/unavailable configuration facts or an
//! unmodeled package directory leaves the ordinary package resolution in place
//! rather than guessing a target.
//!
//! No compiler, package manager or filesystem lookup is executed here.

use crate::typescript::{self, EffectiveConfig};
use crate::typescript_aliases::{Aliases, Dispatch};
use crate::typescript_files::{Availability, Lookup, Mode, Options};
use graph_search_types::source::SourceFileUnits;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum admitted configuration files considered for one generation.
pub(crate) const MAX_PROJECT_CONFIGS: usize = 32;

/// Whether an admitted path names a configuration that can define aliases.
#[must_use]
pub(crate) fn is_project_config(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| matches!(name, "tsconfig.json" | "jsconfig.json"))
}

/// One directory's configuration and its compiled bounded resolver.
#[derive(Clone, Debug)]
struct Project {
    /// Directory of the configuration without a trailing slash; empty at the root.
    directory: String,
    resolution: Option<Resolved>,
}

#[derive(Clone, Debug)]
struct Resolved {
    aliases: Aliases,
    options: Options,
}

/// Generation-owned project selection. An empty catalog leaves resolution to
/// ordinary relative/package rules.
#[derive(Clone, Debug, Default)]
pub(crate) struct Projects {
    projects: Vec<Project>,
}

impl Projects {
    /// Selects configurations from admitted source records and their exact facts.
    pub(crate) fn build(sources: &BTreeMap<String, SourceFileUnits>) -> Self {
        let mut configs: BTreeMap<String, &str> = BTreeMap::new();
        for path in sources.keys() {
            let Some(name) = std::path::Path::new(path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
            else {
                continue;
            };
            if !matches!(name, "tsconfig.json" | "jsconfig.json") {
                continue;
            }
            let Some(source) = sources.get(path) else {
                continue;
            };
            if !source
                .typescript_config
                .as_ref()
                .is_some_and(|config| config.valid() && config.unavailable_reason.is_none())
            {
                continue;
            }
            let directory = std::path::Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            configs
                .entry(directory)
                .and_modify(|existing| {
                    // `tsconfig.json` wins over `jsconfig.json` in one directory.
                    if existing.ends_with("jsconfig.json") {
                        *existing = path.as_str();
                    }
                })
                .or_insert(path.as_str());
        }
        if configs.len() > MAX_PROJECT_CONFIGS {
            // Ambiguity above the bound is explicit: no project is selected.
            return Self::default();
        }
        let mut projects: Vec<Project> = configs
            .into_iter()
            .map(|(directory, path)| Project {
                resolution: compile(path, sources),
                directory,
            })
            .collect();
        // Longest directory first: `find` then returns the nearest configuration.
        projects.sort_by(|a, b| {
            b.directory
                .len()
                .cmp(&a.directory.len())
                .then_with(|| a.directory.cmp(&b.directory))
        });
        Self { projects }
    }

    /// Resolves one bare specifier through the importing file's nearest project.
    /// `None` means ordinary package resolution still applies.
    pub(crate) fn resolve(
        &self,
        from: &str,
        specifier: &str,
        known: &BTreeSet<String>,
    ) -> Option<String> {
        if specifier.is_empty()
            || specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier.starts_with('/')
            || matches!(specifier, "." | "..")
        {
            return None;
        }
        let project = self.projects.iter().find(|project| {
            project.directory.is_empty()
                || from
                    .strip_prefix(&project.directory)
                    .is_some_and(|rest| rest.starts_with('/'))
        })?;
        let resolved = project.resolution.as_ref()?;
        let mut presence = |_candidate: &str| Ok(Availability::Unknown);
        let packages = BTreeSet::new();
        let mut lookup = Lookup::new(&resolved.options, known, &packages, &mut presence);
        match resolved
            .aliases
            .resolve(specifier, |candidate, substitution| {
                lookup.load(candidate, substitution)
            })
            .ok()?
        {
            Dispatch::Paths { target, .. } | Dispatch::BaseUrl(target) => target,
            Dispatch::Unmatched => None,
        }
    }
}

/// Compiles one configuration's aliases and file-loader options, or records the
/// unsupported reason by returning `None`.
fn compile(path: &str, sources: &BTreeMap<String, SourceFileUnits>) -> Option<Resolved> {
    let effective = typescript::inherit(path, sources).ok()?;
    let mode = mode(&effective)?;
    let aliases = Aliases::compile(&effective).ok()?;
    if aliases.is_empty() {
        return None;
    }
    let options = Options::compile(&effective, mode).ok()?;
    Some(Resolved { aliases, options })
}

/// The supported native module modes. Anything else (including `classic` and
/// absent or `node10` defaults) leaves package resolution unchanged.
fn mode(config: &EffectiveConfig) -> Option<Mode> {
    let options = config.configuration.fields.get("compilerOptions")?;
    let options = options.as_object()?;
    let resolution = options
        .get("moduleResolution")?
        .as_str()?
        .to_ascii_lowercase();
    let module = options
        .get("module")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match resolution.as_str() {
        "bundler" => Some(Mode::Bundler),
        "node16" | "nodenext" => Some(if module == "commonjs" {
            Mode::NodeCommonJs
        } else {
            Mode::NodeEsm
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::typescript::TypeScriptConfig;

    fn source(config: &str) -> SourceFileUnits {
        let fields: BTreeMap<String, Value> = serde_json::from_str(config).unwrap();
        let patterns = fields
            .get("compilerOptions")
            .and_then(Value::as_object)
            .and_then(|options| options.get("paths"))
            .and_then(Value::as_object)
            .map_or_else(Vec::new, |paths| {
                paths
                    .keys()
                    .filter(|key| key.contains('*'))
                    .cloned()
                    .collect()
            });
        SourceFileUnits {
            typescript_config: Some(TypeScriptConfig {
                fields,
                path_patterns: patterns,
                unavailable_reason: None,
            }),
            source_hash: "hash".into(),
            version: graph_search_types::limits::SOURCE_INDEX_VERSION,
            ..SourceFileUnits::default()
        }
    }

    #[test]
    fn nearest_configuration_wins_and_wildcards_expand() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "tsconfig.json".into(),
            source(
                r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["src/lib/*"]},"moduleResolution":"bundler","module":"esnext"}}"#,
            ),
        );
        let projects = Projects::build(&sources);
        let known: BTreeSet<String> = ["src/app.ts", "src/lib/thing.ts", "tsconfig.json"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            projects.resolve("src/app.ts", "@lib/thing", &known),
            Some("src/lib/thing.ts".into())
        );
        assert_eq!(
            projects.resolve("src/app.ts", "missing/thing", &known),
            None
        );
    }

    #[test]
    fn unsupported_modes_and_absent_aliases_do_not_guess() {
        let known: BTreeSet<String> = ["src/app.ts", "src/lib/thing.ts", "tsconfig.json"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        for config in [
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@lib/*":["src/lib/*"]},"moduleResolution":"classic"}}"#,
            r#"{"compilerOptions":{"baseUrl":".","moduleResolution":"bundler"}}"#,
        ] {
            let mut sources = BTreeMap::new();
            sources.insert("tsconfig.json".into(), source(config));
            let projects = Projects::build(&sources);
            assert_eq!(projects.resolve("src/app.ts", "@lib/thing", &known), None);
        }
    }
}
