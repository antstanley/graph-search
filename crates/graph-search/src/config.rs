//! The `config.toml` file and its merge into the walk policy
//! (`SPEC.md` §12).
//!
//! Precedence: CLI flag > config file > built-in default. User `excludes`
//! replace the defaults only when `replace_defaults` is set.

use crate::error::{Error, Result};
use graph_search_core::config::WalkPolicy;
use graph_search_types::Language;
use serde::Deserialize;
use std::path::Path;

/// The on-disk configuration, optional at `<root>/.graph-search/config.toml`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ConfigFile {
    /// Directories and paths never walked.
    pub excludes: Option<Vec<String>>,
    /// Whether user `excludes` replace the defaults.
    pub replace_defaults: Option<bool>,
    /// Whether hidden entries are walked.
    pub include_hidden: Option<bool>,
    /// Languages enabled for extraction.
    pub languages: Option<Vec<String>>,
    /// Extra extension to language bindings.
    pub extensions: Option<std::collections::BTreeMap<String, String>>,
    /// The store directory, relative to the root.
    pub store: Option<String>,
    /// The per-file size ceiling.
    pub max_file_bytes: Option<u64>,
}

/// The resolved configuration: the walk policy plus the store path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The walk and extraction policy.
    pub policy: WalkPolicy,
    /// The store directory, relative to the root.
    pub store: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            policy: WalkPolicy::default(),
            store: String::from(".graph-search/index"),
        }
    }
}

impl Config {
    /// Reads and merges `<root>/.graph-search/config.toml`, when present.
    ///
    /// # Errors
    /// [`Error::Config`] when the file exists but cannot be parsed.
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join(".graph-search").join("config.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Self::default());
        };
        let file: ConfigFile = toml::from_str(&text)
            .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
        Ok(Self::default().merge(&file))
    }

    /// Merges one parsed file over `self`.
    #[must_use]
    pub fn merge(mut self, file: &ConfigFile) -> Self {
        if let Some(excludes) = &file.excludes {
            if file.replace_defaults.unwrap_or(false) {
                self.policy.excludes.clone_from(excludes);
            } else {
                for exclude in excludes {
                    if !self.policy.excludes.contains(exclude) {
                        self.policy.excludes.push(exclude.clone());
                    }
                }
            }
        }
        if let Some(hidden) = file.include_hidden {
            self.policy.include_hidden = hidden;
        }
        if let Some(languages) = &file.languages {
            self.policy.languages = languages
                .iter()
                .filter_map(|name| Language::parse(name))
                .collect();
        }
        if let Some(extensions) = &file.extensions {
            for (ext, language) in extensions {
                if let Some(language) = Language::parse(language) {
                    let ext = if ext.starts_with('.') {
                        ext.clone()
                    } else {
                        format!(".{ext}")
                    };
                    self.policy.extensions.insert(ext, language);
                }
            }
        }
        if let Some(store) = &file.store {
            self.store.clone_from(store);
        }
        if let Some(bytes) = file.max_file_bytes {
            self.policy.max_file_bytes = bytes;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Config {
        let file: ConfigFile = toml::from_str(text).unwrap_or_else(|e| panic!("toml: {e}"));
        Config::default().merge(&file)
    }

    #[test]
    fn the_spec_example_parses_and_merges() {
        // `store` and `max_file_bytes` are top-level keys; anything after a
        // `[table]` header would belong to that table.
        let config = parse(
            r#"
excludes = ["target", "secrets"]
replace_defaults = false
include_hidden = false
languages = ["rust", "typescript"]
store = ".cache/index"
max_file_bytes = 4096

[extensions]
".mjs" = "javascript"
"#,
        );
        // User excludes extend the defaults unless told to replace them.
        assert!(config.policy.excludes.contains(&String::from("target")));
        assert!(config.policy.excludes.contains(&String::from("secrets")));
        assert!(
            config
                .policy
                .excludes
                .contains(&String::from("node_modules"))
        );
        assert_eq!(
            config.policy.languages,
            vec![Language::Rust, Language::TypeScript]
        );
        assert_eq!(config.store, ".cache/index");
        assert_eq!(config.policy.max_file_bytes, 4096);
        assert_eq!(
            config.policy.extensions.get(".mjs"),
            Some(&Language::JavaScript)
        );
    }

    #[test]
    fn replace_defaults_replaces() {
        let config = parse(
            r#"
excludes = ["secrets"]
replace_defaults = true
"#,
        );
        assert_eq!(config.policy.excludes, vec![String::from("secrets")]);
    }

    #[test]
    fn a_missing_file_yields_the_defaults() {
        let tmp = tempfile::TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let config = Config::load(tmp.path()).unwrap_or_else(|e| panic!("load: {e}"));
        assert_eq!(config, Config::default());
    }
}
