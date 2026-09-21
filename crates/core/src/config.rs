//! The walk and extraction policy as one named, configurable value
//! (`SPEC.md` §6.1, §12).
//!
//! An index that eats `target/` is worse than useless, so the policy is a
//! struct the library builds (from defaults, `config.toml`, and flags) and
//! every walker honours — not scattered calls.

use graph_search_types::Language;
use std::collections::BTreeMap;

/// Directories never walked, by name (`SPEC.md` §6.1).
pub const DEFAULT_EXCLUDES: [&str; 10] = [
    ".git",
    ".graph-search",
    "target",
    "node_modules",
    "dist",
    "build",
    "out",
    ".venv",
    "venv",
    "vendor",
];

/// Directories that hold version-control metadata and are never searched, as
/// `nanus` does.
pub const VCS_DIRS: [&str; 4] = [".git", ".hg", ".svn", ".bzr"];

/// The default extension to language table (`SPEC.md` §7).
#[must_use]
pub fn default_extensions() -> BTreeMap<String, Language> {
    BTreeMap::from([
        (String::from(".rs"), Language::Rust),
        (String::from(".ts"), Language::TypeScript),
        (String::from(".tsx"), Language::TypeScript),
        (String::from(".mts"), Language::TypeScript),
        (String::from(".cts"), Language::TypeScript),
        (String::from(".js"), Language::JavaScript),
        (String::from(".jsx"), Language::JavaScript),
        (String::from(".mjs"), Language::JavaScript),
        (String::from(".cjs"), Language::JavaScript),
        (String::from(".html"), Language::Html),
        (String::from(".htm"), Language::Html),
        (String::from(".css"), Language::Css),
    ])
}

/// How the tree is walked and which files are extracted (`SPEC.md` §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkPolicy {
    /// Maximum admitted files; clamped to the library hard ceiling.
    pub max_files: usize,
    /// Maximum entries yielded by the policy-constrained walker.
    pub max_walk_entries: usize,
    /// Directory names never walked. User values replace the defaults only
    /// when `replace_defaults` is set (`SPEC.md` §12).
    pub excludes: Vec<String>,
    /// Absolute subtrees excluded independently of hidden and ignore flags.
    /// Hosts use this for index storage; paths must be normalized like the walk root.
    pub excluded_paths: Vec<std::path::PathBuf>,
    /// Whether `include_hidden` applies; hidden entries are skipped otherwise.
    pub include_hidden: bool,
    /// Whether `.gitignore`, `.ignore`, and git excludes are honoured. The
    /// deliberate divergence from `nanus` (`SPEC.md` §6.1); `--no-ignore`
    /// turns it off.
    pub respect_ignore: bool,
    /// Files above this size are not read or searched.
    pub max_file_bytes: u64,
    /// Languages enabled for extraction. Disabling one leaves its files
    /// walkable but un-indexed (`SPEC.md` §12).
    pub languages: Vec<Language>,
    /// The extension to language bindings, including user extras.
    pub extensions: BTreeMap<String, Language>,
}

impl Default for WalkPolicy {
    fn default() -> Self {
        Self {
            max_files: graph_search_types::limits::MAX_FILES,
            max_walk_entries: graph_search_types::limits::MAX_WALK_ENTRIES,
            excludes: DEFAULT_EXCLUDES.iter().map(|s| (*s).to_owned()).collect(),
            excluded_paths: Vec::new(),
            include_hidden: false,
            respect_ignore: true,
            max_file_bytes: graph_search_types::limits::MAX_FILE_BYTES,
            languages: vec![
                Language::Rust,
                Language::TypeScript,
                Language::JavaScript,
                Language::Html,
                Language::Css,
            ],
            extensions: default_extensions(),
        }
    }
}

impl WalkPolicy {
    /// Fingerprint of all inclusion and extraction policy settings.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        crate::hash::content_hash(
            serde_json::json!({
                "hidden":self.include_hidden, "ignore":self.respect_ignore,
                "excludes":self.excludes, "max_file_bytes":self.max_file_bytes,
                "excluded_paths": self.excluded_paths.iter()
                    .map(|path| path.as_os_str().as_encoded_bytes()).collect::<Vec<_>>(),
                "max_files":self.max_files, "max_entries":self.max_walk_entries,
                "languages":self.languages, "extensions":self.extensions,
            })
            .to_string()
            .as_bytes(),
        )
    }

    /// Whether `path` names an always-excluded directory.
    #[must_use]
    pub fn is_excluded(&self, path: &std::path::Path) -> bool {
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| self.excludes.iter().any(|ex| ex == name))
    }

    /// The language a path is, by extension; `None` when unclaimed.
    #[must_use]
    pub fn language_for(&self, path: &std::path::Path) -> Option<Language> {
        let ext = path.extension()?.to_str()?;
        self.extensions.get(&format!(".{ext}")).copied()
    }

    /// Whether a file of `language` is extracted (enabled in the policy and
    /// claimed by the extension table).
    #[must_use]
    pub fn is_enabled(&self, language: Language) -> bool {
        language == Language::Unknown || self.languages.contains(&language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_to_languages() {
        let policy = WalkPolicy::default();
        assert_eq!(
            policy.language_for(std::path::Path::new("a/b.rs")),
            Some(Language::Rust)
        );
        assert_eq!(
            policy.language_for(std::path::Path::new("a/b.tsx")),
            Some(Language::TypeScript)
        );
        assert_eq!(policy.language_for(std::path::Path::new("a/b.txt")), None);
    }

    #[test]
    fn extras_are_added_and_enabled_languages_gate_extraction() {
        let mut policy = WalkPolicy::default();
        policy
            .extensions
            .insert(String::from(".mjsx"), Language::JavaScript);
        assert_eq!(
            policy.language_for(std::path::Path::new("a.mjsx")),
            Some(Language::JavaScript)
        );
        policy.languages.retain(|l| *l == Language::Rust);
        assert!(!policy.is_enabled(Language::TypeScript));
    }
}
