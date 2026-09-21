//! Deliberately bounded YAML subset for authored pnpm workspace membership.
//! No aliases, tags, flow collections, multiline scalars, or document merging.
use graph_search_types::package::{
    NodePackageMetadata, PackageEcosystem, PackageManifest, PackageRole,
};
use std::collections::BTreeSet;

fn scalar(value: &str) -> Option<String> {
    if value.starts_with('"') {
        serde_json::from_str::<String>(value).ok()
    } else if value.starts_with('\'') {
        let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
        let decoded = inner.replace("''", "");
        (!decoded.contains('\'')).then(|| inner.replace("''", "'"))
    } else {
        (!value.is_empty()
            && !value.starts_with(['!', '&', '*', '@', '`'])
            && !value.contains(": ")
            && !["- ", "? ", ": "]
                .iter()
                .any(|prefix| value.starts_with(prefix))
            && !value.contains(['[', ']', '{', '}', '|', '>', '\'', '"', '\\']))
        .then(|| value.to_owned())
    }
}

/// Strip a comment and locate a mapping colon outside complete quoted strings.
fn line_parts(line: &str) -> Option<(&str, Option<usize>)> {
    let mut quote = None;
    let mut escaped = false;
    let mut colon = None;
    let mut previous_space = true;
    for (offset, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '#' && previous_space {
            return Some((line[..offset].trim_end(), colon));
        } else if ch == ':' && colon.is_none() {
            colon = Some(offset);
        }
        previous_space = ch.is_whitespace();
    }
    (quote.is_none() && !escaped).then_some((line.trim_end(), colon))
}

fn implicit_scalar(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "null"
            | "~"
            | "true"
            | "false"
            | "yes"
            | "no"
            | "on"
            | "off"
            | ".nan"
            | ".inf"
            | "-.inf"
            | "+.inf"
    ) || value
        .trim_start_matches(['+', '-', '.'])
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
        || value.parse::<f64>().is_ok()
        || ["0x", "0o", "0b"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
}

fn header_key<'a>(key: &'a str, keys: &mut BTreeSet<&'a str>) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        && keys.insert(key)
}

fn projection(text: &str) -> Option<NodePackageMetadata> {
    let mut overrides = Vec::new();
    let mut keys = BTreeSet::new();
    let mut active = "";
    let mut block = false;
    let mut indent = None;
    let mut sequence = None;
    let mut child_keys = BTreeSet::new();
    let mut result = None;
    let mut packages_sequence = false;
    for raw in text.lines() {
        if raw.contains('\t') {
            return None;
        }
        let (line, colon) = line_parts(raw)?;
        if line.trim().is_empty() {
            continue;
        }
        let spaces = line
            .len()
            .saturating_sub(line.trim_start_matches(' ').len());
        if spaces == 0 {
            let split = colon?;
            let key = &line[..split];
            if !header_key(key, &mut keys) {
                return None;
            }
            let value = line[split.saturating_add(1)..].trim();
            active = key;
            block = value.is_empty();
            indent = None;
            sequence = None;
            child_keys.clear();
            if key == "packages" {
                if !block && value != "[]" {
                    return None;
                }
                result = Some(Vec::new());
                packages_sequence = value == "[]";
            } else if key == "overrides" {
                if !block && value != "{}" {
                    return None;
                }
            } else if !block && value != "[]" && value != "{}" {
                scalar(value)?;
            }
        } else {
            if !block || indent.is_some_and(|expected| expected != spaces) {
                return None;
            }
            indent = Some(spaces);
            let content = &line[spaces..];
            let item = content.strip_prefix("- ");
            let is_sequence = item.is_some();
            if sequence.is_some_and(|expected| expected != is_sequence) {
                return None;
            }
            sequence = Some(is_sequence);
            let value = if let Some(item) = item {
                scalar(item.trim())?
            } else {
                let split = colon?.checked_sub(spaces)?;
                let key = scalar(content[..split].trim())?;
                if active == "overrides" {
                    overrides.push(key.clone());
                }
                if !child_keys.insert(key) {
                    return None;
                }
                scalar(content[split.saturating_add(1)..].trim())?
            };
            if active == "overrides" && is_sequence {
                return None;
            }
            if active == "packages" {
                if !is_sequence {
                    return None;
                }
                let raw_value = item?.trim();
                if !raw_value.starts_with(['\'', '"']) && implicit_scalar(raw_value) {
                    return None;
                }
                result.as_mut()?.push(value);
                packages_sequence = true;
            }
        }
    }
    packages_sequence.then_some(NodePackageMetadata {
        workspaces: Some(result?),
        workspace_overrides: overrides,
        ..NodePackageMetadata::default()
    })
}

pub(crate) fn extract(text: &str) -> PackageManifest {
    let metadata = projection(text).filter(NodePackageMetadata::valid);
    PackageManifest {
        ecosystem: PackageEcosystem::Node,
        role: if metadata.is_some() {
            PackageRole::Workspace
        } else {
            PackageRole::Unavailable
        },
        unavailable_reason: metadata
            .is_none()
            .then(|| "pnpm_workspace_syntax_unmodeled".into()),
        node: metadata,
        name: None,
        cargo_targets: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_only_complete_declared_membership_in_the_supported_yaml_subset() {
        let text = "packages:\n  - packages/*\n  - 'docs' # comment\n  - \"!packages/excluded\"\noverrides:\n  \"@scope/tool\": \"file:./tools/tool.tgz\"\nallowBuilds:\n  esbuild: false\n";
        let fact = extract(text);
        assert_eq!(
            fact.node.unwrap().workspaces.unwrap(),
            ["packages/*", "docs", "!packages/excluded"]
        );
        for text in [
            "packages:\n",
            "packages: *alias",
            "packages:\n  - packages/*\npackages: []",
            "notes: |\npackages:\n  - packages/*",
            "packages:\n - packages/*\n  - other/*",
            "packages:\n  - packages/*\nnotes: 'unclosed",
            "packages:\n  - packages/*\n---\npackages: []",
            "packages:\n  name: value",
            "packages:\n  - true\n",
            "packages:\n  - 123\n",
            "packages:\n  - 1_000\n",
            "packages:\n  - packages/*\nother: invalid: mapping\n",
            "packages:\n  - packages/*\nother:\n  key: value\n  key: again\n",
        ] {
            assert_eq!(extract(text).role, PackageRole::Unavailable, "{text}");
        }
        assert_eq!(
            extract("packages: []").node.unwrap().workspaces,
            Some(vec![])
        );
    }
}
