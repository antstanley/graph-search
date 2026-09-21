//! Authored TypeScript configuration facts, before project selection or inheritance.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// A bounded projection of a JSON/JSONC file that could be selected as a config.
/// Its owning source record supplies the path and exact original-byte hash.
/// Presence does not declare a project or prove compiler-option semantics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeScriptConfig {
    /// Authored top-level resolution/membership fields. Absence, null, empty
    /// containers and array order remain distinct; paths are never normalized.
    /// Compiler options remain raw, including options not yet modeled by resolution.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
    /// Wildcard `paths` keys in authored first-property order. Exact names are
    /// independent of pattern precedence. Duplicate keys retain their first slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_patterns: Vec<String>,
    /// Fixed failure code. An unavailable projection never retains partial fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

impl TypeScriptConfig {
    /// Top-level fields retained for later native configuration interpretation.
    pub const FIELDS: [&'static str; 6] = [
        "extends",
        "compilerOptions",
        "files",
        "include",
        "exclude",
        "references",
    ];

    /// Validate bounds independently of the syntax adapter and persisted decoder.
    /// This checks representation, not TypeScript compiler-option semantics.
    #[must_use]
    pub fn valid(&self) -> bool {
        use crate::limits::{MAX_TYPESCRIPT_CONFIG_DEPTH, MAX_TYPESCRIPT_CONFIG_VALUES};
        if let Some(reason) = &self.unavailable_reason {
            return self.fields.is_empty()
                && self.path_patterns.is_empty()
                && !reason.is_empty()
                && reason.len() <= 128
                && reason
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
        }
        if self
            .fields
            .keys()
            .any(|key| !Self::FIELDS.contains(&key.as_str()))
        {
            return false;
        }
        let mut bytes = 0usize;
        let mut admit = |text: &str| {
            bytes = bytes.saturating_add(text.len());
            text.len() <= 4096 && bytes <= crate::limits::MAX_TYPESCRIPT_CONFIG_TEXT_BYTES
        };
        let mut work: Vec<_> = self.fields.values().map(|value| (value, 0usize)).collect();
        if self.path_patterns.len() > MAX_TYPESCRIPT_CONFIG_VALUES
            || self.path_patterns.iter().any(|key| !admit(key))
        {
            return false;
        }
        let mut visited = self.path_patterns.len();
        while let Some((value, depth)) = work.pop() {
            visited = visited.saturating_add(1);
            if visited > MAX_TYPESCRIPT_CONFIG_VALUES || depth > MAX_TYPESCRIPT_CONFIG_DEPTH {
                return false;
            }
            let children = match value {
                Value::Array(array) => array.len(),
                Value::Object(object) => object.len(),
                _ => 0,
            };
            if visited.saturating_add(work.len()).saturating_add(children)
                > MAX_TYPESCRIPT_CONFIG_VALUES
            {
                return false;
            }
            match value {
                Value::String(text) if !admit(text) => return false,
                Value::Array(array) => {
                    work.extend(array.iter().map(|value| (value, depth.saturating_add(1))));
                }
                Value::Object(object) => {
                    for (key, value) in object {
                        if !admit(key) {
                            return false;
                        }
                        work.push((value, depth.saturating_add(1)));
                    }
                }
                _ => {}
            }
        }
        let expected: std::collections::BTreeSet<_> = self
            .fields
            .get("compilerOptions")
            .and_then(|options| options.get("paths"))
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|paths| paths.keys())
            .filter(|key| key.contains('*'))
            .map(String::as_str)
            .collect();
        let ordered: std::collections::BTreeSet<_> =
            self.path_patterns.iter().map(String::as_str).collect();
        ordered.len() == self.path_patterns.len() && ordered == expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_representation_is_bounded_and_preserves_raw_semantics() {
        let mut config = TypeScriptConfig {
            fields: BTreeMap::from([
                (
                    "extends".into(),
                    serde_json::json!(["../first.json", "../last.json"]),
                ),
                ("files".into(), serde_json::json!([])),
                (
                    "compilerOptions".into(),
                    serde_json::json!({"paths":null,"customConditions":["source"]}),
                ),
            ]),
            unavailable_reason: None,
            path_patterns: Vec::new(),
        };
        assert!(config.valid());
        assert_eq!(
            serde_json::from_slice::<TypeScriptConfig>(&serde_json::to_vec(&config).unwrap())
                .unwrap(),
            config
        );
        config.fields.insert("unknown".into(), Value::Null);
        assert!(!config.valid());
        config.fields.remove("unknown");
        config.unavailable_reason = Some("invalid_config_syntax".into());
        assert!(!config.valid());
        config.fields.clear();
        assert!(config.valid());
        config.unavailable_reason = None;
        config.fields.insert(
            "files".into(),
            serde_json::json!(vec!["x"; crate::limits::MAX_TYPESCRIPT_CONFIG_VALUES]),
        );
        assert!(!config.valid());
        config
            .fields
            .insert("files".into(), serde_json::json!(["x".repeat(4097)]));
        assert!(!config.valid());
        config.fields.insert(
            "files".into(),
            serde_json::json!(vec!["x".repeat(4096); 33]),
        );
        assert!(!config.valid());
        let mut nested = Value::Null;
        for _ in 0..=crate::limits::MAX_TYPESCRIPT_CONFIG_DEPTH {
            nested = serde_json::json!([nested]);
        }
        config.fields.insert("files".into(), nested);
        assert!(!config.valid());
        config.fields.clear();
        config.fields.insert(
            "compilerOptions".into(),
            serde_json::json!({"paths":{"a*Z":[],"a*YZ":[]}}),
        );
        assert!(
            !config.valid(),
            "missing wildcard order cannot be reconstructed from sorted keys"
        );
        config.path_patterns = vec!["a*YZ".into(), "a*Z".into()];
        assert!(config.valid());
        config.path_patterns = vec!["a*Z".into(), "a*Z".into()];
        assert!(!config.valid());
        config.path_patterns = vec!["a*Z".into(), "foreign*".into()];
        assert!(!config.valid());
    }
}
