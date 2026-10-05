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

/// Maximum admitted configuration files considered for one generation. A
/// lookup walks the importing file's ancestor directories, so its cost does
/// not grow with the number of projects.
pub(crate) const MAX_PROJECT_CONFIGS: usize = 1024;

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
    resolution: Option<Resolved>,
    /// The compiled output directory and the source directory it mirrors.
    output: Option<(String, String)>,
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
    /// By configuration directory.
    projects: BTreeMap<String, Project>,
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
        let projects = configs
            .into_iter()
            .map(|(directory, path)| {
                (
                    directory,
                    Project {
                        resolution: compile(path, sources),
                        output: output_mapping(path, sources),
                    },
                )
            })
            .collect();
        Self { projects }
    }

    /// The nearest project whose directory contains `path`.
    fn nearest(&self, path: &str) -> Option<&Project> {
        let mut directory = path;
        loop {
            directory = directory.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(project) = self.projects.get(directory) {
                return Some(project);
            }
            if directory.is_empty() {
                return None;
            }
        }
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
        let project = self.nearest(from)?;
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
impl Projects {
    /// The source file a project compiles to `output`, when `output` lies
    /// under the project's `outDir`: the same relative path under `rootDir`
    /// with its source extension (`dist/a.js` -> `src/a.ts`).
    pub(crate) fn output_source(&self, output: &str, known: &BTreeSet<String>) -> Option<String> {
        self.nearest(output).and_then(|project| {
            let (out_dir, root_dir) = project.output.as_ref()?;
            let relative = output.strip_prefix(out_dir.as_str())?.strip_prefix('/')?;
            let (stem, extensions): (&str, &[&str]) = if let Some(stem) = relative
                .strip_suffix(".d.ts")
                .or_else(|| relative.strip_suffix(".js"))
            {
                (stem, &["ts", "tsx"])
            } else if let Some(stem) = relative.strip_suffix(".mjs") {
                (stem, &["mts"])
            } else {
                (relative.strip_suffix(".cjs")?, &["cts"])
            };
            let choices: Vec<String> = extensions
                .iter()
                .map(|extension| format!("{root_dir}/{stem}.{extension}"))
                .filter(|candidate| known.contains(candidate))
                .collect();
            match choices.as_slice() {
                [source] => Some(source.clone()),
                _ => None,
            }
        })
    }
}

/// A project's `outDir` and the `rootDir` it mirrors, both workspace-relative.
/// Without an explicit `rootDir`, only `include` patterns that all start in
/// one top directory name it.
fn output_mapping(
    path: &str,
    sources: &BTreeMap<String, SourceFileUnits>,
) -> Option<(String, String)> {
    let effective = typescript::inherit(path, sources).ok()?;
    let options = effective
        .configuration
        .fields
        .get("compilerOptions")?
        .as_object()?;
    let directory = |origin: &str| {
        origin
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory)
            .to_owned()
    };
    let join = |base: &str, relative: &str| -> Option<String> {
        let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
        for part in relative.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop()?;
                }
                other => parts.push(other),
            }
        }
        (!parts.is_empty()).then(|| parts.join("/"))
    };
    let out_dir = join(
        &directory(effective.option_origins.get("outDir")?),
        options.get("outDir")?.as_str()?,
    )?;
    let root_dir = if let Some(root) = options.get("rootDir").and_then(Value::as_str) {
        join(&directory(effective.option_origins.get("rootDir")?), root)?
    } else {
        let include = effective.configuration.fields.get("include")?.as_array()?;
        let mut tops = include.iter().map(|pattern| {
            pattern
                .as_str()
                .map(|pattern| pattern.trim_start_matches("./"))
                .and_then(|pattern| pattern.split('/').next())
                .filter(|top| !top.contains('*') && !top.is_empty())
        });
        let first = tops.next()??;
        if !tops.all(|top| top == Some(first)) {
            return None;
        }
        join(&directory(effective.field_origins.get("include")?), first)?
    };
    (out_dir != root_dir).then_some((out_dir, root_dir))
}

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
    fn a_sveltekit_app_without_its_generated_base_still_resolves_lib() {
        let mut sources = BTreeMap::new();
        sources.insert(
            "app/tsconfig.json".into(),
            source(
                r#"{"extends":"./.svelte-kit/tsconfig.json","compilerOptions":{"strict":true}}"#,
            ),
        );
        let effective = crate::typescript::inherit("app/tsconfig.json", &sources);
        assert!(effective.is_ok(), "{effective:?}");
        let effective = effective.unwrap();
        assert!(mode(&effective).is_some());
        let aliases = Aliases::compile(&effective);
        assert!(aliases.is_ok(), "{aliases:?}");
        let options = Options::compile(&effective, mode(&effective).unwrap());
        assert!(options.is_ok(), "{options:?}");
        let projects = Projects::build(&sources);
        let known: BTreeSet<String> = ["app/src/hooks.ts", "app/src/lib/logger.ts"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            projects.resolve("app/src/hooks.ts", "$lib/logger", &known),
            Some("app/src/lib/logger.ts".into())
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
