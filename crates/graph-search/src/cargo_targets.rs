//! Bounded authored Cargo target facts; no root discovery or filesystem reads.
use graph_search_types::limits::{
    MAX_CARGO_TARGET_FEATURES, MAX_CARGO_TARGET_PATH_BYTES, MAX_CARGO_TARGETS,
    MAX_PACKAGE_NAME_BYTES,
};
use graph_search_types::package::{
    CargoBuildScript, CargoTarget, CargoTargetKind, CargoTargetMetadata,
};
use serde_json::Value;

type Result<T> = std::result::Result<T, &'static str>;

pub(crate) fn extract(manifest: &Value, package: &Value) -> CargoTargetMetadata {
    parse(manifest, package).unwrap_or_else(|reason| CargoTargetMetadata {
        unavailable_reason: Some(reason.into()),
        ..CargoTargetMetadata::default()
    })
}

fn text(value: Option<&Value>, limit: usize) -> Result<Option<String>> {
    match value {
        None => Ok(None),
        Some(Value::String(value))
            if !value.is_empty() && value.len() <= limit && !value.contains('\0') =>
        {
            Ok(Some(value.clone()))
        }
        _ => Err("invalid_target_string"),
    }
}

fn target(kind: CargoTargetKind, value: &Value) -> Result<CargoTarget> {
    let table = value.as_object().ok_or("invalid_target_table")?;
    let name = text(table.get("name"), MAX_PACKAGE_NAME_BYTES)?;
    let path = text(table.get("path"), MAX_CARGO_TARGET_PATH_BYTES)?;
    let mut required_features = Vec::new();
    if let Some(features) = table.get("required-features") {
        let features = features.as_array().ok_or("invalid_target_features")?;
        if features.len() > MAX_CARGO_TARGET_FEATURES {
            return Err("target_feature_limit");
        }
        for feature in features {
            required_features.push(
                text(Some(feature), MAX_PACKAGE_NAME_BYTES)?.ok_or("invalid_target_features")?,
            );
        }
    }
    Ok(CargoTarget {
        kind,
        name,
        path,
        required_features,
    })
}

fn parse(manifest: &Value, package: &Value) -> Result<CargoTargetMetadata> {
    let build_script = match package.get("build") {
        None => None,
        Some(Value::Bool(false)) => Some(CargoBuildScript::Disabled),
        Some(Value::Bool(true)) => Some(CargoBuildScript::Default),
        value => Some(CargoBuildScript::Path(
            text(value, MAX_CARGO_TARGET_PATH_BYTES)?.ok_or("invalid_build_script")?,
        )),
    };
    let mut result = CargoTargetMetadata {
        build_script,
        ..Default::default()
    };
    match package.get("edition") {
        Some(Value::Object(edition))
            if edition.len() == 1 && edition.get("workspace") == Some(&Value::Bool(true)) =>
        {
            result.edition_workspace = true;
        }
        edition => result.edition = text(edition, 32)?,
    }
    for (kind, key, auto) in [
        (CargoTargetKind::Lib, "lib", "autolib"),
        (CargoTargetKind::Bin, "bin", "autobins"),
        (CargoTargetKind::Example, "example", "autoexamples"),
        (CargoTargetKind::Test, "test", "autotests"),
        (CargoTargetKind::Bench, "bench", "autobenches"),
    ] {
        if let Some(value) = package.get(auto) {
            result
                .auto_discovery
                .insert(kind, value.as_bool().ok_or("invalid_target_auto_flag")?);
        }
        let Some(value) = manifest.get(key) else {
            continue;
        };
        if kind == CargoTargetKind::Lib {
            result.targets.push(target(kind, value)?);
        } else {
            let values = value.as_array().ok_or("invalid_target_array")?;
            if values.is_empty() {
                result.empty_target_tables.insert(kind);
            }
            if result.targets.len().saturating_add(values.len()) > MAX_CARGO_TARGETS {
                return Err("target_count_limit");
            }
            for value in values {
                result.targets.push(target(kind, value)?);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authored_targets_preserve_omissions_paths_flags_and_workspace_edition() {
        let manifest = json!({"package":{"edition":{"workspace":true},"autobins":false},
            "lib":{"path":"custom/é.rs"}, "bin":[{"name":"worker"},{"path":"../shared/entry.rs","required-features":["runtime"]}],
            "example":[{"name":"demo"}], "test":[{"name":"integration"}], "bench":[{"name":"speed"}]});
        let result = extract(&manifest, &manifest["package"]);
        assert!(result.unavailable_reason.is_none());
        assert!(result.edition_workspace);
        assert_eq!(result.edition, None);
        assert_eq!(
            result.auto_discovery.into_iter().collect::<Vec<_>>(),
            [(CargoTargetKind::Bin, false)]
        );
        assert_eq!(result.targets.len(), 6);
        assert_eq!(result.targets[0].path.as_deref(), Some("custom/é.rs"));
        assert_eq!(result.targets[1].path, None);
        assert_eq!(
            result.targets[2].path.as_deref(),
            Some("../shared/entry.rs")
        );
        assert_eq!(result.targets[2].required_features, ["runtime"]);
        assert_eq!(
            extract(&json!({}), &json!({})),
            CargoTargetMetadata::default()
        );
    }

    #[test]
    fn explicit_empty_arrays_and_build_script_settings_are_distinct_from_omission() {
        for (value, expected) in [
            (json!(false), CargoBuildScript::Disabled),
            (json!(true), CargoBuildScript::Default),
            (
                json!("custom/setup.rs"),
                CargoBuildScript::Path("custom/setup.rs".into()),
            ),
        ] {
            let result = extract(&json!({"bin":[]}), &json!({"build":value}));
            assert_eq!(result.build_script, Some(expected));
            assert_eq!(
                result.empty_target_tables,
                std::collections::BTreeSet::from([CargoTargetKind::Bin])
            );
            assert!(result.targets.is_empty());
            assert!(result.unavailable_reason.is_none());
        }
        for value in [
            json!(12),
            json!(""),
            json!("x".repeat(MAX_CARGO_TARGET_PATH_BYTES + 1)),
        ] {
            let result = extract(&json!({"bin":[]}), &json!({"build":value}));
            assert!(result.unavailable_reason.is_some());
            assert!(result.empty_target_tables.is_empty());
            assert!(result.build_script.is_none());
        }
    }

    #[test]
    fn invalid_or_over_budget_targets_never_leave_a_partial_projection() {
        for manifest in [
            json!({"lib":{"path":42}}),
            json!({"bin":{}}),
            json!({"bin":[{"required-features":"x"}]}),
            json!({"bin":[{"required-features":vec!["x";MAX_CARGO_TARGET_FEATURES+1]}]}),
            json!({"bin":vec![json!({"name":"x"});MAX_CARGO_TARGETS+1]}),
            json!({"lib":{"path":"x".repeat(MAX_CARGO_TARGET_PATH_BYTES+1)}}),
            json!({"lib":{"name":"x".repeat(MAX_PACKAGE_NAME_BYTES+1)}}),
            json!({"lib":{"path":"a\0b"}}),
        ] {
            let result = extract(&manifest, &json!({"edition":"2024","autolib":false}));
            assert!(result.unavailable_reason.is_some());
            assert_eq!(
                result,
                CargoTargetMetadata {
                    unavailable_reason: result.unavailable_reason.clone(),
                    ..CargoTargetMetadata::default()
                }
            );
        }
        for package in [
            json!({"edition":42}),
            json!({"edition":{"workspace":false}}),
            json!({"autobins":"false"}),
        ] {
            assert!(
                extract(&json!({"lib":{}}), &package)
                    .unavailable_reason
                    .is_some()
            );
        }
        let exact = json!({"bin":vec![json!({"path":"x".repeat(MAX_CARGO_TARGET_PATH_BYTES),"required-features":vec!["x";MAX_CARGO_TARGET_FEATURES]});MAX_CARGO_TARGETS]});
        assert!(extract(&exact, &json!({})).unavailable_reason.is_none());
    }
}
