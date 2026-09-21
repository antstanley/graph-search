//! Root-file enumeration for an explicitly selected TypeScript configuration.
//! This is not imported-file closure, project discovery or source admission.
use crate::{
    typescript::EffectiveConfig,
    typescript_patterns::{Budget, Pattern},
};
use serde_json::Value;
use std::collections::BTreeSet;

/// Enumerate configured root paths over a complete case-sensitive file namespace.
/// Explicit `files` paths are retained even when missing, as in the compiler;
/// callers must separately check admission before publishing a binding. A partial
/// admitted-source inventory is not a complete namespace for this operation.
///
/// # Errors
/// Invalid configuration/shapes, unsupported paths, more than 256 specs per field,
/// more than 65,536 inventory files or exhausted pattern work budget.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // Compiler extension matching is case-sensitive.
pub fn enumerate(
    config_path: &str,
    config: &EffectiveConfig,
    inventory: &BTreeSet<String>,
) -> Result<BTreeSet<String>, &'static str> {
    if !config.configuration.valid() || config.configuration.unavailable_reason.is_some() {
        return Err("ts_project_config_unavailable");
    }
    if crate::typescript::normalize("", config_path)? != config_path {
        return Err("ts_project_path_unsupported");
    }
    if inventory.len() > 65_536 {
        return Err("ts_project_inventory_limit");
    }
    let fields = &config.configuration.fields;
    let options = fields
        .get("compilerOptions")
        .map(|value| value.as_object().ok_or("ts_project_option_shape"))
        .transpose()?;
    let option = |key: &str| options.and_then(|map| map.get(key));
    for key in ["module", "moduleResolution"] {
        if option(key).is_some_and(|value| !value.is_string()) {
            return Err("ts_project_option_shape");
        }
    }
    let boolean = |key| {
        option(key)
            .map(|value| value.as_bool().ok_or("ts_project_option_shape"))
            .transpose()
    };
    let jsconfig = config_path.ends_with("/jsconfig.json") || config_path == "jsconfig.json";
    let allow_js = boolean("allowJs")?.unwrap_or(jsconfig || boolean("checkJs")?.unwrap_or(false));
    let module = option("module")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let resolution = option("moduleResolution")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let json = boolean("resolveJsonModule")?.unwrap_or(
        matches!(module.as_str(), "nodenext" | "node20")
            || resolution == "bundler"
            || (resolution.is_empty()
                && !matches!(
                    module.as_str(),
                    "none" | "amd" | "umd" | "system" | "node16" | "node18"
                )),
    );
    let specs = |key: &str| -> Result<Option<Vec<&str>>, &'static str> {
        fields
            .get(key)
            .map(|value| {
                let values = value.as_array().ok_or("ts_project_membership_shape")?;
                if values.len() > 256 {
                    return Err("ts_project_spec_limit");
                }
                values
                    .iter()
                    .map(|value| value.as_str().ok_or("ts_project_membership_shape"))
                    .collect()
            })
            .transpose()
    };
    let origin = |key: &str| {
        config
            .field_origins
            .get(key)
            .map_or(config_path, String::as_str)
    };
    let mut literals = BTreeSet::new();
    let files = specs("files")?;
    for name in files.iter().flatten() {
        if name.contains("${") {
            return Err("ts_project_pattern_unsupported");
        }
        literals.insert(crate::typescript::normalize(
            directory(origin("files")),
            name,
        )?);
    }
    let includes = specs("include")?.unwrap_or_else(|| {
        if files.is_some() {
            vec![]
        } else {
            vec!["**/*"]
        }
    });
    let mut exclusions = Vec::new();
    if let Some(specs) = specs("exclude")? {
        for spec in specs {
            exclusions.push(Pattern::compile(directory(origin("exclude")), spec, true)?);
        }
    } else {
        for key in ["outDir", "declarationDir"] {
            if let Some(value) = option(key) {
                let value = value.as_str().ok_or("ts_project_option_shape")?;
                let origin = config
                    .option_origins
                    .get(key)
                    .map_or(config_path, String::as_str);
                exclusions.push(Pattern::compile(directory(origin), value, true)?);
            }
        }
    }
    let mut patterns = Vec::new();
    for spec in includes {
        patterns.push((
            Pattern::compile(directory(origin("include")), spec, false)?,
            spec.ends_with(".json"),
        ));
    }
    let groups: &[&[&str]] = if allow_js {
        &[
            &[".ts", ".tsx", ".d.ts", ".js", ".jsx"],
            &[".cts", ".d.cts", ".cjs"],
            &[".mts", ".d.mts", ".mjs"],
        ]
    } else {
        &[
            &[".ts", ".tsx", ".d.ts"],
            &[".cts", ".d.cts"],
            &[".mts", ".d.mts"],
        ]
    };
    let mut candidates = Vec::new();
    let mut budget = Budget::default();
    for path in inventory {
        if crate::typescript::normalize("", path)? != *path {
            return Err("ts_project_path_unsupported");
        }
        let is_json = path.ends_with(".json");
        let group = groups
            .iter()
            .find(|group| group.iter().any(|ext| path.ends_with(ext)))
            .copied();
        if group.is_none() && !(json && is_json) {
            continue;
        }
        let mut excluded = false;
        for pattern in &exclusions {
            if pattern.matches(path, &mut budget)? {
                excluded = true;
                break;
            }
        }
        if excluded {
            continue;
        }
        for (index, (pattern, json_pattern)) in patterns.iter().enumerate() {
            if (!is_json || *json_pattern) && pattern.matches(path, &mut budget)? {
                candidates.push((index, path, group));
                break;
            }
        }
    }
    // Include order first; within an include, the compiler visits directory files
    // before subdirectories. Root ordering itself is not exposed by this API.
    candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| visit_order(a.1, b.1)));
    let mut wildcard = BTreeSet::new();
    for (_, path, group) in candidates {
        if let Some(group) = group {
            let stem = stem(path);
            let mut suppressed = false;
            for ext in group {
                if path.ends_with(ext) && (*ext != ".ts" || !path.ends_with(".d.ts")) {
                    break;
                }
                let higher = format!("{stem}{ext}");
                if (literals.contains(&higher) || wildcard.contains(&higher))
                    && !(*ext == ".d.ts" && (path.ends_with(".js") || path.ends_with(".jsx")))
                {
                    suppressed = true;
                    break;
                }
            }
            if suppressed {
                continue;
            }
            for ext in group.iter().rev() {
                if path.ends_with(ext) {
                    break;
                }
                wildcard.remove(&format!("{stem}{ext}"));
            }
        }
        if !literals.contains(path) {
            wildcard.insert(path.clone());
        }
    }
    literals.extend(wildcard);
    Ok(literals)
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}
fn stem(path: &str) -> &str {
    for ext in [
        ".d.ts", ".d.cts", ".d.mts", ".tsx", ".jsx", ".cts", ".mts", ".cjs", ".mjs", ".ts", ".js",
    ] {
        if let Some(stem) = path.strip_suffix(ext) {
            return stem;
        }
    }
    path
}
fn visit_order(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a = a.split('/');
    let mut b = b.split('/');
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x == y => {}
            (Some(x), Some(y)) => {
                return a
                    .clone()
                    .next()
                    .is_some()
                    .cmp(&b.clone().next().is_some())
                    .then_with(|| x.cmp(y));
            }
            _ => return std::cmp::Ordering::Equal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(value: Value) -> EffectiveConfig {
        let Value::Object(fields) = value else {
            panic!("object fixture required");
        };
        EffectiveConfig {
            configuration: graph_search_types::typescript::TypeScriptConfig {
                fields: fields.into_iter().collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn paths(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).into()).collect()
    }

    #[test]
    fn files_are_literal_roots_and_exclusions_only_filter_wildcards() {
        let config = config(
            json!({"files":["missing.ts","src/a.js"],"include":["src"],"exclude":["**/a.*"]}),
        );
        assert_eq!(
            enumerate(
                "tsconfig.json",
                &config,
                &paths(&["src/a.ts", "src/a.js", "src/b.ts"])
            )
            .unwrap(),
            paths(&["missing.ts", "src/a.js", "src/b.ts"])
        );
        assert!(
            enumerate(
                "tsconfig.json",
                &self::config(json!({"files":[]})),
                &paths(&["src/b.ts"])
            )
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn inherited_membership_and_output_paths_use_their_declaring_config() {
        let mut config =
            config(json!({"include":["src"],"compilerOptions":{"outDir":"src/generated"}}));
        config
            .field_origins
            .insert("include".into(), "base/config.json".into());
        config
            .option_origins
            .insert("outDir".into(), "base/options.json".into());
        let inventory = paths(&["base/src/a.ts", "base/src/generated/b.ts", "app/src/c.ts"]);
        assert_eq!(
            enumerate("app/tsconfig.json", &config, &inventory).unwrap(),
            paths(&["base/src/a.ts"])
        );
        config
            .configuration
            .fields
            .insert("exclude".into(), json!([]));
        assert_eq!(
            enumerate("app/tsconfig.json", &config, &inventory).unwrap(),
            paths(&["base/src/a.ts", "base/src/generated/b.ts"])
        );
    }

    #[test]
    fn source_priority_retains_explicit_files_and_declaration_js_coexistence() {
        let config =
            config(json!({"files":["a.js"],"include":["**/*"],"compilerOptions":{"allowJs":true}}));
        assert_eq!(
            enumerate(
                "tsconfig.json",
                &config,
                &paths(&[
                    "a.ts", "a.js", "a.d.ts", "b.d.ts", "b.js", "c.cts", "c.d.cts", "c.cjs"
                ])
            )
            .unwrap(),
            paths(&["a.ts", "a.js", "b.d.ts", "b.js", "c.cts", "c.d.cts"])
        );
    }

    #[test]
    fn unsupported_fields_paths_templates_and_limits_fail_without_partial_roots() {
        for value in [
            json!({"files":"x.ts"}),
            json!({"include":["../outside"]}),
            json!({"include":["${configDir}/src"]}),
            json!({"files":["${configDir}/src/a.ts"]}),
            json!({"compilerOptions":{"allowJs":"true"}}),
            json!({"compilerOptions":{"module":1}}),
        ] {
            assert!(enumerate("tsconfig.json", &config(value), &paths(&["x.ts"])).is_err());
        }
        assert_eq!(
            enumerate(
                "tsconfig.json",
                &config(json!({"include":vec!["src";257]})),
                &BTreeSet::new()
            ),
            Err("ts_project_spec_limit")
        );
        let inventory = (0..65_537).map(|i| format!("{i}.ts")).collect();
        assert_eq!(
            enumerate("tsconfig.json", &config(json!({})), &inventory),
            Err("ts_project_inventory_limit")
        );
    }
}
