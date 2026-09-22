//! Manifest syntax adapters reuse this library's existing JSON/TOML decoders.
use graph_search_core::ports::SourceFile;
use graph_search_types::package::{PackageEcosystem, PackageManifest, PackageRole};

pub(crate) fn extract(file: &SourceFile<'_>) -> Option<PackageManifest> {
    let ecosystem = match file.path.file_name()?.to_str()? {
        "Cargo.toml" => PackageEcosystem::Cargo,
        "package.json" | "pnpm-workspace.yaml" => PackageEcosystem::Node,
        "pyproject.toml" => PackageEcosystem::Python,
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
        PackageEcosystem::Cargo | PackageEcosystem::Python => {
            toml::from_str::<toml::Value>(file.text)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok())
        }
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
    } else if ecosystem == PackageEcosystem::Python {
        // PEP 621 `[project]`, else Poetry's `[tool.poetry]`. A pyproject.toml
        // with neither (tool configuration only) is still an unnamed boundary.
        match python_project(&value) {
            Some(project) if project.is_object() => project,
            Some(_) => return Some(unavailable("unsupported_project_table")),
            None => &serde_json::Value::Null,
        }
    } else {
        &value
    };
    let name = match package.get("name") {
        Some(serde_json::Value::String(name)) if !name.is_empty() => Some(name.clone()),
        None if ecosystem != PackageEcosystem::Cargo => None,
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

/// The table naming a Python project: PEP 621 `[project]`, else `[tool.poetry]`.
fn python_project(value: &serde_json::Value) -> Option<&serde_json::Value> {
    value
        .get("project")
        .or_else(|| value.get("tool")?.get("poetry"))
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
    fn pyproject_names_come_from_pep_621_then_poetry() {
        let read = |text| {
            extract(&SourceFile {
                path: Path::new("pyproject.toml"),
                text,
            })
            .unwrap()
        };
        let pep = read("[project]\nname='alpha'\n[tool.poetry]\nname='beta'\n");
        assert_eq!(pep.ecosystem, PackageEcosystem::Python);
        assert_eq!(pep.role, PackageRole::Package);
        assert_eq!(pep.name.as_deref(), Some("alpha"));
        assert!(pep.node.is_none() && pep.cargo_targets.is_none());
        assert_eq!(
            read("[tool.poetry]\nname='beta'\n").name.as_deref(),
            Some("beta")
        );
        let unnamed = read("[tool.ruff]\nline-length=100\n");
        assert_eq!(unnamed.role, PackageRole::Package);
        assert!(unnamed.name.is_none());
        for (text, reason) in [
            ("not toml", "invalid_manifest_syntax"),
            ("project='x'", "unsupported_project_table"),
            ("[project]\nname=3", "unsupported_package_name"),
            ("[project]\nname=''", "unsupported_package_name"),
        ] {
            let fact = read(text);
            assert_eq!(fact.role, PackageRole::Unavailable, "{text}");
            assert_eq!(fact.unavailable_reason.as_deref(), Some(reason), "{text}");
        }
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
