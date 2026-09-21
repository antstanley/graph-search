//! Manifest syntax adapters reuse this library's existing JSON/TOML decoders.
use graph_search_core::ports::SourceFile;
use graph_search_types::package::{PackageEcosystem, PackageManifest, PackageRole};

pub(crate) fn extract(file: &SourceFile<'_>) -> Option<PackageManifest> {
    let ecosystem = match file.path.file_name()?.to_str()? {
        "Cargo.toml" => PackageEcosystem::Cargo,
        "package.json" | "pnpm-workspace.yaml" => PackageEcosystem::Node,
        _ => return None,
    };
    let unavailable = |reason: &str| PackageManifest {
        node: None,
        cargo_targets: None,
        ecosystem,
        role: PackageRole::Unavailable,
        name: None,
        unavailable_reason: Some(reason.into()),
    };
    if file.text.len() > graph_search_types::limits::MAX_PACKAGE_MANIFEST_BYTES {
        return Some(unavailable("manifest_byte_limit"));
    }
    if file
        .path
        .file_name()
        .is_some_and(|name| name == "pnpm-workspace.yaml")
    {
        return Some(crate::pnpm_workspace::extract(file.text));
    }
    let value = match ecosystem {
        PackageEcosystem::Cargo => toml::from_str::<toml::Value>(file.text)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok()),
        PackageEcosystem::Node => serde_json::from_str::<serde_json::Value>(file.text).ok(),
    };
    let Some(value) = value.filter(serde_json::Value::is_object) else {
        return Some(unavailable("invalid_manifest_syntax"));
    };
    let package = if ecosystem == PackageEcosystem::Cargo {
        match value.get("package") {
            Some(package) if package.is_object() => package,
            None if value
                .get("workspace")
                .is_some_and(serde_json::Value::is_object) =>
            {
                return Some(PackageManifest {
                    node: None,
                    cargo_targets: None,
                    ecosystem,
                    role: PackageRole::Workspace,
                    name: None,
                    unavailable_reason: None,
                });
            }
            _ => return Some(unavailable("missing_package_or_workspace")),
        }
    } else {
        &value
    };
    let name = match package.get("name") {
        Some(serde_json::Value::String(name)) if !name.is_empty() => Some(name.clone()),
        None if ecosystem == PackageEcosystem::Node => None,
        _ => return Some(unavailable("unsupported_package_name")),
    };
    if name
        .as_ref()
        .is_some_and(|name| name.len() > graph_search_types::limits::MAX_PACKAGE_NAME_BYTES)
    {
        return Some(unavailable("package_name_limit"));
    }
    Some(PackageManifest {
        node: (ecosystem == PackageEcosystem::Node).then(|| crate::node_package::extract(&value)),
        cargo_targets: (ecosystem == PackageEcosystem::Cargo)
            .then(|| crate::cargo_targets::extract(&value, package)),
        ecosystem,
        role: PackageRole::Package,
        name,
        unavailable_reason: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn package_syntax_and_caps_do_not_guess_from_strings_or_invalid_metadata() {
        let read = |path, text| {
            extract(&SourceFile {
                path: Path::new(path),
                text,
            })
            .unwrap()
        };
        let cargo = read("Cargo.toml", "[package]\nname='native'\n[workspace]\n");
        assert_eq!(cargo.role, PackageRole::Package);
        assert_eq!(cargo.name.as_deref(), Some("native"));
        let workspace = read(
            "Cargo.toml",
            "description='''\n[package]\nname=\"fake\"\n'''\n[workspace]\n",
        );
        assert_eq!(workspace.role, PackageRole::Workspace);
        assert!(workspace.name.is_none());
        assert_eq!(read("package.json", "{}").role, PackageRole::Package);
        for (path, text) in [
            ("Cargo.toml", "[package]\nname.workspace=true"),
            ("package.json", "{\"name\":3}"),
            ("package.json", "[]"),
            ("Cargo.toml", "not toml"),
        ] {
            let fact = read(path, text);
            assert_eq!(fact.role, PackageRole::Unavailable);
            assert!(fact.unavailable_reason.is_some());
        }
        let oversized = " ".repeat(graph_search_types::limits::MAX_PACKAGE_MANIFEST_BYTES + 1);
        assert_eq!(
            read("package.json", &oversized)
                .unavailable_reason
                .as_deref(),
            Some("manifest_byte_limit")
        );
        let name = "a".repeat(graph_search_types::limits::MAX_PACKAGE_NAME_BYTES + 1);
        assert_eq!(
            read("package.json", &format!("{{\"name\":\"{name}\"}}"))
                .unavailable_reason
                .as_deref(),
            Some("package_name_limit")
        );
    }
    #[test]
    fn cargo_target_caps_preserve_package_identity_and_legacy_defaults() {
        fn read(path: &str, text: &str) -> PackageManifest {
            extract(&SourceFile {
                path: Path::new(path),
                text,
            })
            .unwrap()
        }
        for suffix in [
            "[[bin]]\nname='worker'\n".repeat(graph_search_types::limits::MAX_CARGO_TARGETS + 1),
            "[package.edition]\nworkspace=false\n".into(),
        ] {
            let fact = read("Cargo.toml", &format!("[package]\nname='native'\n{suffix}"));
            assert_eq!(fact.role, PackageRole::Package);
            assert!(fact.cargo_targets.unwrap().unavailable_reason.is_some());
        }
        let oversized = format!(
            "[package]\nname='native'\n#{}",
            "x".repeat(graph_search_types::limits::MAX_PACKAGE_MANIFEST_BYTES)
        );
        let fact = read("Cargo.toml", &oversized);
        assert_eq!(
            fact.unavailable_reason.as_deref(),
            Some("manifest_byte_limit")
        );
        assert!(fact.cargo_targets.is_none());
        for (path, text) in [("Cargo.toml", "[workspace]"), ("package.json", "{}")] {
            assert!(read(path, text).cargo_targets.is_none());
        }
        let empty = read("Cargo.toml", "bin=[]\n[package]\nname='native'\n");
        assert_eq!(
            empty.cargo_targets.unwrap().empty_target_tables,
            std::collections::BTreeSet::from([graph_search_types::package::CargoTargetKind::Bin])
        );
        let legacy: PackageManifest = serde_json::from_str(
            r#"{"ecosystem":"cargo","role":"package","name":"native","unavailable_reason":null}"#,
        )
        .unwrap();
        assert!(legacy.cargo_targets.is_none());
    }
}
