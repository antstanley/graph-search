//! Authored Node package decisions, using the existing JSON decoder only.
use graph_search_types::package::{NodePackageMetadata, NodePackageTarget};
use serde_json::Value;
use std::collections::BTreeMap;

fn target(value: &Value) -> NodePackageTarget {
    fn invariant(value: &Value, depth: usize) -> NodePackageTarget {
        match value {
            Value::String(path) => NodePackageTarget::Path(path.clone()),
            Value::Null => NodePackageTarget::Blocked,
            Value::Object(object)
                if depth < graph_search_types::limits::MAX_NODE_CONDITION_DEPTH
                    && object.contains_key("default")
                    && object.keys().all(|key| {
                        !key.is_empty()
                            && key.len() <= 4096
                            && !key.starts_with('.')
                            && !key.contains(',')
                            && !key.chars().all(|c| c.is_ascii_digit())
                            && !key.chars().any(char::is_control)
                    }) =>
            {
                let mut path = None;
                for value in object.values() {
                    let (NodePackageTarget::Path(candidate)
                    | NodePackageTarget::InvariantPath(candidate)) =
                        invariant(value, depth.saturating_add(1))
                    else {
                        return NodePackageTarget::Unsupported;
                    };
                    if path.as_ref().is_some_and(|path| path != &candidate) {
                        return NodePackageTarget::Unsupported;
                    }
                    path = Some(candidate);
                }
                path.map_or(
                    NodePackageTarget::Unsupported,
                    NodePackageTarget::InvariantPath,
                )
            }
            _ => NodePackageTarget::Unsupported,
        }
    }
    invariant(value, 0)
}

fn mapping(
    value: &Value,
    exports: bool,
) -> Result<BTreeMap<String, NodePackageTarget>, &'static str> {
    if exports {
        let Some(object) = value.as_object() else {
            return Ok(BTreeMap::from([(".".into(), target(value))]));
        };
        if !object.is_empty() && object.keys().all(|key| !key.starts_with('.')) {
            return Ok(BTreeMap::from([(".".into(), target(value))]));
        }
    }
    let object = value.as_object().ok_or("node_map_shape")?;
    let mut result = BTreeMap::new();
    for (key, value) in object {
        let valid = if exports {
            key == "." || key.starts_with("./")
        } else {
            key.starts_with('#') && key.len() > 1 && !key.starts_with("#/")
        };
        if !valid {
            return Err("node_map_key");
        }
        result.insert(key.clone(), target(value));
    }
    Ok(result)
}

fn project(value: &Value) -> Result<NodePackageMetadata, &'static str> {
    let mut metadata = NodePackageMetadata::default();
    for overrides in [
        value.get("overrides"),
        value.get("resolutions"),
        value.get("pnpm").and_then(|pnpm| pnpm.get("overrides")),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(object) = overrides
            .as_object()
            .filter(|object| object.values().all(Value::is_string))
        {
            metadata.workspace_overrides.extend(object.keys().cloned());
        } else {
            metadata.workspace_overrides.push("*".into());
        }
    }
    if let Some(manager) = value.get("packageManager") {
        metadata.package_manager = Some(manager.as_str().ok_or("node_package_manager")?.into());
    }
    if let Some(kind) = value.get("type") {
        metadata.module_type = Some(kind.as_str().ok_or("node_module_type")?.into());
    }
    metadata.main = value.get("main").map(|value| match value {
        Value::String(path) => NodePackageTarget::Path(path.clone()),
        Value::Null => NodePackageTarget::Blocked,
        _ => NodePackageTarget::Unsupported,
    });
    metadata.exports = value
        .get("exports")
        .map(|value| mapping(value, true))
        .transpose()?;
    metadata.imports = value
        .get("imports")
        .map(|value| mapping(value, false))
        .transpose()?;
    if let Some(workspaces) = value.get("workspaces") {
        let entries = workspaces
            .as_array()
            .or_else(|| workspaces.get("packages").and_then(Value::as_array))
            .ok_or("node_workspaces_shape")?;
        metadata.workspaces = Some(
            entries
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .map(str::to_owned)
                        .ok_or("node_workspace_pattern")
                })
                .collect::<Result<_, _>>()?,
        );
    }
    for field in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(dependencies) = value.get(field) {
            for (name, specifier) in dependencies.as_object().ok_or("node_dependencies_shape")? {
                let specifier = specifier.as_str().ok_or("node_dependency_specifier")?;
                if let Some(previous) = metadata.dependencies.insert(name.clone(), specifier.into())
                    && previous != specifier
                {
                    return Err("node_dependency_conflict");
                }
            }
        }
    }
    if !metadata.valid() {
        return Err("node_metadata_limit_or_value");
    }
    Ok(metadata)
}

pub(crate) fn extract(value: &Value) -> NodePackageMetadata {
    project(value).unwrap_or_else(|reason| NodePackageMetadata {
        unavailable_reason: Some(reason.into()),
        ..NodePackageMetadata::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_maps_preserve_absence_empty_null_and_unsupported_targets() {
        let value = serde_json::json!({"type":"module","main":"./legacy.js","exports":{".":"./index.js","./hidden":null,"./conditional":{"import":"./import.js","require":"./require.cjs"},"./array":["./a.js"],"./feature/*":"./src/*.js"},"imports":{"#local":"./local.js"},"workspaces":{"packages":["packages/*"]},"dependencies":{"@scope/api":"workspace:*"}});
        let metadata = extract(&value);
        assert!(metadata.valid());
        assert!(metadata.unavailable_reason.is_none());
        let exports = metadata.exports.unwrap();
        assert_eq!(exports["./hidden"], NodePackageTarget::Blocked);
        assert_eq!(exports["./conditional"], NodePackageTarget::Unsupported);
        assert_eq!(exports["./array"], NodePackageTarget::Unsupported);
        assert_eq!(
            exports["./feature/*"],
            NodePackageTarget::Path("./src/*.js".into())
        );
        assert!(extract(&serde_json::json!({})).exports.is_none());
        assert_eq!(
            extract(&serde_json::json!({"exports":{}})).exports,
            Some(BTreeMap::new())
        );
        assert_eq!(
            extract(&serde_json::json!({"exports":null}))
                .exports
                .unwrap()["."],
            NodePackageTarget::Blocked
        );
        assert_eq!(
            extract(&serde_json::json!({"exports":{"import":"./a.js"}}))
                .exports
                .unwrap()["."],
            NodePackageTarget::Unsupported
        );
    }
    #[test]
    fn conditions_bind_only_when_every_branch_has_the_same_path_and_a_default() {
        for exports in [
            serde_json::json!({"types":"./api.ts","default":"./api.ts"}),
            serde_json::json!({"node":{"import":"./api.ts","default":"./api.ts"},"default":"./api.ts"}),
        ] {
            assert_eq!(
                extract(&serde_json::json!({"exports":exports}))
                    .exports
                    .unwrap()["."],
                NodePackageTarget::InvariantPath("./api.ts".into())
            );
        }
        for exports in [
            serde_json::json!({"types":"./api.ts"}),
            serde_json::json!({"types":"./types.ts","default":"./api.ts"}),
            serde_json::json!({"node":null,"default":"./api.ts"}),
            serde_json::json!({"0":"./api.ts","default":"./api.ts"}),
            serde_json::json!({"node":{"import":"./api.ts"},"default":"./api.ts"}),
        ] {
            assert_eq!(
                extract(&serde_json::json!({"exports":exports}))
                    .exports
                    .unwrap()["."],
                NodePackageTarget::Unsupported
            );
        }
    }
    #[test]
    fn invalid_or_over_budget_metadata_never_leaves_partial_decisions() {
        for value in [
            serde_json::json!({"main":"./valid.js","exports":{".":"./index.js","import":"./other.js"}}),
            serde_json::json!({"imports":{"ordinary":"./index.js"}}),
            serde_json::json!({"workspaces":[3]}),
            serde_json::json!({"exports":"./a\npi.js"}),
            serde_json::json!({"dependencies":{"api":"workspace:*"},"devDependencies":{"api":"^1"}}),
            serde_json::json!({"main":"a".repeat(4097)}),
            serde_json::json!({"workspaces":vec!["packages/*";4097]}),
        ] {
            let metadata = extract(&value);
            assert!(metadata.unavailable_reason.is_some(), "{metadata:?}");
            assert!(metadata.valid());
            assert!(
                metadata.main.is_none()
                    && metadata.exports.is_none()
                    && metadata.imports.is_none()
                    && metadata.workspaces.is_none()
                    && metadata.dependencies.is_empty()
            );
        }
    }
}
