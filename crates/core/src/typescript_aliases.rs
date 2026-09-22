//! Native TypeScript paths/baseUrl dispatch, independent of the module-mode loader.
use crate::typescript::EffectiveConfig;
use std::collections::BTreeMap;

/// Bounded substitution attempts for one specifier.
pub const MAX_SUBSTITUTIONS: usize = 128;

/// Compiled authored aliases for one explicitly selected effective configuration.
#[derive(Clone, Debug, Default)]
pub struct Aliases {
    base_url: Option<String>,
    paths_base: String,
    paths: BTreeMap<String, Vec<String>>,
    patterns: Vec<String>,
}

/// Which optional-resolution branch ran. A matching paths key suppresses baseUrl
/// even when every substitution misses; package fallback remains the caller's job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatch<T> {
    /// No eligible optional setting; continue the ordinary module loader.
    Unmatched,
    /// A paths key matched, with its authored identity and optional loaded target.
    Paths {
        /// Exact or wildcard key selected before testing substitutions.
        pattern: String,
        /// First loaded target, or no target after all substitutions missed.
        target: Option<T>,
    },
    /// No paths key matched; baseUrl was attempted.
    BaseUrl(Option<T>),
}

impl Aliases {
    /// Whether this configuration declared neither `paths` nor `baseUrl`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.base_url.is_none() && self.paths.is_empty()
    }

    /// Compile origin-aware paths/baseUrl settings without accessing source files.
    /// Other compiler settings are interpreted by the caller's module loader.
    ///
    /// # Errors
    /// Invalid metadata, option shapes, wildcard syntax, missing origins or limits.
    pub fn compile(config: &EffectiveConfig) -> Result<Self, &'static str> {
        if !config.configuration.valid() || config.configuration.unavailable_reason.is_some() {
            return Err("ts_alias_config_unavailable");
        }
        let Some(options) = config.configuration.fields.get("compilerOptions") else {
            return Ok(Self::default());
        };
        let options = options.as_object().ok_or("ts_alias_options_shape")?;
        let origin = |key: &str| -> Result<&str, &'static str> {
            let path = config
                .option_origins
                .get(key)
                .ok_or("ts_alias_origin_missing")?;
            if join("", path)? != *path || path.is_empty() || path.ends_with('/') {
                return Err("ts_alias_origin_invalid");
            }
            Ok(path.rsplit_once('/').map_or("", |(directory, _)| directory))
        };
        let base_url = options
            .get("baseUrl")
            .map(|value| {
                join(
                    origin("baseUrl")?,
                    value.as_str().ok_or("ts_alias_base_url_shape")?,
                )
            })
            .transpose()?;
        let Some(paths) = options.get("paths") else {
            return Ok(Self {
                base_url,
                ..Self::default()
            });
        };
        let paths = paths.as_object().ok_or("ts_alias_paths_shape")?;
        let paths_base = base_url.clone().unwrap_or(origin("paths")?.into());
        let mut compiled = BTreeMap::new();
        for (pattern, values) in paths {
            if pattern.matches('*').count() > 1 || pattern.is_empty() {
                return Err("ts_alias_pattern_unsupported");
            }
            let values = values.as_array().ok_or("ts_alias_substitutions_shape")?;
            if values.is_empty() || values.len() > MAX_SUBSTITUTIONS {
                return Err("ts_alias_substitution_limit");
            }
            let mut substitutions = Vec::with_capacity(values.len());
            for value in values {
                let value = value.as_str().ok_or("ts_alias_substitutions_shape")?;
                if value.matches('*').count() > 1 {
                    return Err("ts_alias_substitution_unsupported");
                }
                // Validate authored paths before any lookup. The wildcard remains
                // literal here; the expanded candidate is independently bounded.
                join(&paths_base, value)?;
                substitutions.push(value.into());
            }
            compiled.insert(pattern.clone(), substitutions);
        }
        Ok(Self {
            base_url,
            paths_base,
            paths: compiled,
            patterns: config.configuration.path_patterns.clone(),
        })
    }

    /// Dispatch an eligible bare specifier in compiler order.
    ///
    /// `load(candidate, substitution)` receives a normalized workspace-relative
    /// path and the unexpanded authored substitution (`None` for baseUrl). The
    /// loader must distinguish an extension authored in that substitution from
    /// one introduced by wildcard expansion when deciding exact-file priority. The
    /// callback must implement the selected module mode, suffixes and package
    /// directory rules; this function does not guess them. Errors stop fallback.
    ///
    /// # Errors
    /// Unsupported specifier/path, exhausted bounds, or an error from the loader.
    pub fn resolve<T>(
        &self,
        specifier: &str,
        mut load: impl FnMut(&str, Option<&str>) -> Result<Option<T>, &'static str>,
    ) -> Result<Dispatch<T>, &'static str> {
        if matches!(specifier, "." | "..")
            || specifier.starts_with("./")
            || specifier.starts_with("../")
            || specifier.starts_with('/')
        {
            return Ok(Dispatch::Unmatched);
        }
        if specifier.is_empty() || specifier.len() > 4096 || specifier.contains(['\\', ':', '\0']) {
            return Err("ts_alias_specifier_unsupported");
        }
        let selected = if self.paths.contains_key(specifier) {
            Some((specifier, None))
        } else {
            let mut best = None;
            let mut longest = None;
            for pattern in &self.patterns {
                let Some((prefix, suffix)) = pattern.split_once('*') else {
                    continue;
                };
                if let Some(matched) = specifier
                    .strip_prefix(prefix)
                    .and_then(|tail| tail.strip_suffix(suffix))
                    && longest.is_none_or(|length| prefix.len() > length)
                {
                    longest = Some(prefix.len());
                    best = Some((pattern.as_str(), Some(matched)));
                }
            }
            best
        };
        if let Some((pattern, matched)) = selected {
            for substitution in &self.paths[pattern] {
                // Compiler dispatch (`tryLoadModuleUsingPaths`):
                // `matchedStar ? replaceFirstStar(subst, matchedStar) : subst`.
                // An empty wildcard capture is falsy there, so the substitution
                // is tried literally — its `*` stays and cannot load a file. The
                // key still counts as matched (no baseUrl fallback). Expanding
                // `*` to "" instead would try `src/` and invent a directory-index
                // resolution the compiler never makes.
                let expanded = match matched.filter(|text| !text.is_empty()) {
                    Some(text) => substitution.replacen('*', text, 1),
                    None => substitution.clone(),
                };
                let candidate = join(&self.paths_base, &expanded)?;
                if let Some(target) = load(&candidate, Some(substitution))? {
                    return Ok(Dispatch::Paths {
                        pattern: pattern.into(),
                        target: Some(target),
                    });
                }
            }
            return Ok(Dispatch::Paths {
                pattern: pattern.into(),
                target: None,
            });
        }
        self.base_url
            .as_ref()
            .map_or(Ok(Dispatch::Unmatched), |base| {
                let candidate = join(base, specifier)?;
                load(&candidate, None).map(Dispatch::BaseUrl)
            })
    }
}

fn join(directory: &str, path: &str) -> Result<String, &'static str> {
    if path.len() > 4096 || path.starts_with('/') || path.contains(['\\', ':', '\0']) {
        return Err("ts_alias_path_unsupported");
    }
    let mut parts: Vec<_> = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or("ts_alias_workspace_escape")?;
            }
            other => parts.push(other),
        }
    }
    let mut result = parts.join("/");
    if path.ends_with('/') && !result.is_empty() {
        result.push('/');
    }
    if result.len() > 4096 {
        return Err("ts_alias_path_unsupported");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn loader_receives_authored_substitution_even_when_expanded_paths_are_identical() {
        let aliases = Aliases::compile(&config(
            json!({"paths":{"explicit":["./x.js"],"*":["./*"]}}),
            &["*"],
        ))
        .unwrap();
        let mut calls = Vec::new();
        for specifier in ["explicit", "x.js"] {
            aliases
                .resolve::<String>(specifier, |path, substitution| {
                    calls.push((path.to_owned(), substitution.map(str::to_owned)));
                    Ok(None)
                })
                .unwrap();
        }
        assert_eq!(
            calls,
            [
                ("config/x.js".into(), Some("./x.js".into())),
                ("config/x.js".into(), Some("./*".into())),
            ]
        );
    }

    fn config(options: Value, order: &[&str]) -> EffectiveConfig {
        let origins = options
            .as_object()
            .unwrap()
            .keys()
            .map(|key| (key.clone(), "config/base.json".into()))
            .collect();
        EffectiveConfig {
            configuration: graph_search_types::typescript::TypeScriptConfig {
                fields: BTreeMap::from([("compilerOptions".into(), options)]),
                path_patterns: order.iter().map(|key| (*key).into()).collect(),
                unavailable_reason: None,
            },
            option_origins: origins,
            ..EffectiveConfig::default()
        }
    }

    fn attempts(aliases: &Aliases, specifier: &str) -> (Dispatch<String>, Vec<(String, bool)>) {
        let mut calls = Vec::new();
        let result = aliases
            .resolve(specifier, |path, substitution| {
                calls.push((path.into(), substitution.is_some()));
                Ok(None::<String>)
            })
            .unwrap();
        (result, calls)
    }

    #[test]
    fn exact_keys_longest_prefix_and_authored_ties_choose_one_pattern() {
        let settings = config(
            json!({"paths":{"a*Z":["./first/*"],"a*YZ":["./second/*"],"ab*":["./long/*"],"aYZ":["./exact"]}}),
            &["a*Z", "a*YZ", "ab*"],
        );
        let aliases = Aliases::compile(&settings).unwrap();
        assert_eq!(
            attempts(&aliases, "aYZ").1,
            vec![("config/exact".into(), true)]
        );
        assert_eq!(
            attempts(&aliases, "abYZ").1,
            vec![("config/long/YZ".into(), true)]
        );
        assert_eq!(
            attempts(&aliases, "axYZ").1,
            vec![("config/first/xY".into(), true)]
        );
        let mut reversed = settings;
        reversed.configuration.path_patterns.swap(0, 1);
        assert_eq!(
            attempts(&Aliases::compile(&reversed).unwrap(), "axYZ").1,
            vec![("config/second/x".into(), true)]
        );
    }

    #[test]
    fn mapped_misses_suppress_base_url_and_substitutions_stop_on_success_or_error() {
        let aliases = Aliases::compile(&config(
            json!({"baseUrl":"../base","paths":{"@/*":["first/*","second/*","third/*"]}}),
            &["@/*"],
        ))
        .unwrap();
        let (miss, calls) = attempts(&aliases, "@/file");
        assert_eq!(
            miss,
            Dispatch::Paths {
                pattern: "@/*".into(),
                target: None
            }
        );
        assert_eq!(calls.len(), 3);
        assert!(calls.iter().all(|(_, mapped)| *mapped));
        assert_eq!(
            attempts(&aliases, "other").1,
            vec![("base/other".into(), false)]
        );
        let mut calls = Vec::new();
        let result = aliases
            .resolve("@/file", |path, _| {
                calls.push(path.to_owned());
                Ok((path == "base/second/file").then(|| path.to_owned()))
            })
            .unwrap();
        assert_eq!(calls, ["base/first/file", "base/second/file"]);
        assert_eq!(
            result,
            Dispatch::Paths {
                pattern: "@/*".into(),
                target: Some("base/second/file".into())
            }
        );
        let mut count = 0;
        assert_eq!(
            aliases.resolve::<String>("@/file", |_, _| {
                count += 1;
                Err("loader_unavailable")
            }),
            Err("loader_unavailable")
        );
        assert_eq!(count, 1);
    }

    #[test]
    fn effective_base_url_overrides_paths_origin_and_trailing_directory_intent_survives() {
        let mut settings = config(
            json!({"baseUrl":"./src","paths":{"@/*":["./lib/*/"]}}),
            &["@/*"],
        );
        settings
            .option_origins
            .insert("baseUrl".into(), "app/tsconfig.json".into());
        let aliases = Aliases::compile(&settings).unwrap();
        assert_eq!(
            attempts(&aliases, "@/widget").1,
            vec![("app/src/lib/widget/".into(), true)]
        );
        settings
            .configuration
            .fields
            .get_mut("compilerOptions")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("baseUrl");
        assert_eq!(
            attempts(&Aliases::compile(&settings).unwrap(), "@/widget").1,
            vec![("config/lib/widget/".into(), true)]
        );
    }

    #[test]
    fn empty_wildcards_unicode_and_dot_prefixed_bare_names_preserve_compiler_dispatch() {
        let aliases = Aliases::compile(&config(
            json!({"baseUrl":"..","paths":{"a*":["src/*"],"é*終":["src/*"],".bare":["src/bare"]}}),
            &["a*", "é*終"],
        ))
        .unwrap();
        // An empty wildcard capture is falsy in the compiler
        // (`matchedStar ? replaceFirstStar(subst, matchedStar) : subst`), so the
        // substitution is tried literally: `src/*`, which loads nothing. It must
        // NOT become `src/` — that would resolve `src/index.ts`, an edge tsc
        // never produces. The key still matched, so baseUrl is not consulted.
        let (dispatch, calls) = attempts(&aliases, "a");
        assert_eq!(calls, vec![("src/*".into(), true)]);
        assert_eq!(
            dispatch,
            Dispatch::Paths {
                pattern: "a*".into(),
                target: None
            }
        );
        assert_eq!(attempts(&aliases, "é中終").1, vec![("src/中".into(), true)]);
        assert_eq!(
            attempts(&aliases, ".bare").1,
            vec![("src/bare".into(), true)]
        );
        assert_eq!(attempts(&aliases, "./bare"), (Dispatch::Unmatched, vec![]));
        assert_eq!(attempts(&aliases, "../bare"), (Dispatch::Unmatched, vec![]));
    }

    #[test]
    fn invalid_shapes_origins_escapes_and_expansion_limits_fail_before_loader_use() {
        for options in [
            json!({"baseUrl":null}),
            json!({"paths":null}),
            json!({"paths":{"x":[]}}),
            json!({"paths":{"x":[7]}}),
            json!({"paths":{"x":["../../outside"]}}),
            json!({"paths":{"x":["./a*b*"]}}),
        ] {
            assert!(Aliases::compile(&config(options, &[])).is_err());
        }
        let mut settings = config(
            json!({"baseUrl":".","paths":{"*":vec!["./x"; MAX_SUBSTITUTIONS + 1]}}),
            &["*"],
        );
        assert!(Aliases::compile(&settings).is_err());
        settings = config(json!({"baseUrl":"."}), &[]);
        settings.option_origins.clear();
        assert!(Aliases::compile(&settings).is_err());
        let aliases = Aliases::compile(&config(
            json!({"paths":{"*": [format!("./{}/*", "x".repeat(3000))]}}),
            &["*"],
        ))
        .unwrap();
        let mut called = false;
        assert!(
            aliases
                .resolve::<String>(&"y".repeat(2000), |_, _| {
                    called = true;
                    Ok(None)
                })
                .is_err()
        );
        assert!(!called);
    }
}
