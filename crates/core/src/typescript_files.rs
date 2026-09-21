//! Native modern TypeScript file loading over one generation's admitted paths.
use crate::typescript::EffectiveConfig;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum distinct presence checks across all substitutions of one resolution.
pub const MAX_PROBES: usize = 256;
/// Maximum authored module suffixes.
pub const MAX_SUFFIXES: usize = 32;

/// Explicitly selected modern resolution mode. Project/module classification is
/// the caller's responsibility; this loader never guesses it from a filename.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// TypeScript's bundler file and directory behavior.
    Bundler,
    /// `Node16`/`NodeNext` imports resolved in ESM mode.
    NodeEsm,
    /// `Node16`/`NodeNext` imports resolved in `CommonJS` mode.
    NodeCommonJs,
}

/// Compiled options that affect native file probing.
#[derive(Clone, Debug)]
pub struct Options {
    mode: Mode,
    suffixes: Vec<String>,
    json: bool,
}

impl Options {
    /// Compile the supported modern file options from a selected configuration.
    /// JSON defaults follow TypeScript 6: bundler, `NodeNext` and `Node20` enable it.
    ///
    /// # Errors
    /// Unavailable facts, unsupported resolution mode, invalid option shapes or bounds.
    pub fn compile(config: &EffectiveConfig, mode: Mode) -> Result<Self, &'static str> {
        if !config.configuration.valid() || config.configuration.unavailable_reason.is_some() {
            return Err("ts_module_config_unavailable");
        }
        let options = config
            .configuration
            .fields
            .get("compilerOptions")
            .and_then(Value::as_object)
            .ok_or("ts_module_options_shape")?;
        let resolution = options
            .get("moduleResolution")
            .and_then(Value::as_str)
            .ok_or("ts_module_resolution_unmodeled")?
            .to_ascii_lowercase();
        if !matches!(
            (mode, resolution.as_str()),
            (Mode::Bundler, "bundler")
                | (Mode::NodeEsm | Mode::NodeCommonJs, "node16" | "nodenext")
        ) {
            return Err("ts_module_resolution_unmodeled");
        }
        // This compiler API-only switch is not a supported authored config option.
        if options.contains_key("noDtsResolution") {
            return Err("ts_module_option_unmodeled");
        }
        let module = options
            .get("module")
            .map(|value| value.as_str().ok_or("ts_module_options_shape"))
            .transpose()?
            .unwrap_or("")
            .to_ascii_lowercase();
        let json = options
            .get("resolveJsonModule")
            .map(|value| value.as_bool().ok_or("ts_module_options_shape"))
            .transpose()?
            .unwrap_or(mode == Mode::Bundler || matches!(module.as_str(), "nodenext" | "node20"));
        let mut suffixes = Vec::new();
        if let Some(values) = options.get("moduleSuffixes") {
            let values = values.as_array().ok_or("ts_module_suffixes_shape")?;
            if values.len() > MAX_SUFFIXES {
                return Err("ts_module_suffix_limit");
            }
            for value in values {
                let suffix = value.as_str().ok_or("ts_module_suffixes_shape")?;
                if suffix.len() > 128 || suffix.contains(['/', '\\', ':', '\0']) {
                    return Err("ts_module_suffix_unsupported");
                }
                suffixes.push(suffix.into());
            }
        }
        if suffixes.is_empty() {
            suffixes.push(String::new());
        }
        Ok(Self {
            mode,
            suffixes,
            json,
        })
    }
}

/// Availability of a candidate. Lack of an admitted source record alone never
/// establishes absence: the presence provider must distinguish these states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Source is admitted in the selected generation.
    Admitted,
    /// Candidate is proven not to be a regular file in the captured namespace.
    Absent,
    /// Candidate exists or is opaque, but is not an admitted source.
    Unavailable,
    /// No reliable presence decision is available.
    Unknown,
}

impl Availability {
    /// Stable observation name for diagnostics and evidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Absent => "absent",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

/// A distinct path observed during resolution, including negative dependencies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
    /// Workspace-relative path.
    pub path: String,
    /// Admission or qualified absence, including unavailable/unknown candidates.
    pub availability: Availability,
    /// A package boundary was present, even if its source was unavailable.
    pub package_boundary: bool,
}

/// One resolution's bounded lookup state; reuse across alias substitutions.
/// All facts must describe the same generation. Package boundaries must include
/// unavailable manifests so their entry rules cannot be bypassed with index files.
pub struct Lookup<'a> {
    options: &'a Options,
    files: &'a BTreeSet<String>,
    packages: &'a BTreeSet<String>,
    presence: &'a mut dyn FnMut(&str) -> Result<Availability, &'static str>,
    cache: BTreeMap<String, Result<Availability, &'static str>>,
    probes: Vec<Probe>,
}

impl<'a> Lookup<'a> {
    /// Start a single resolution. `presence` qualifies candidates not admitted in
    /// the supplied source set; it must not return `Admitted`. Unknown/unavailable
    /// candidates stop fallback. The caller owns capture/validation of namespace
    /// facts; core performs no direct filesystem access.
    #[must_use]
    pub fn new(
        options: &'a Options,
        files: &'a BTreeSet<String>,
        packages: &'a BTreeSet<String>,
        presence: &'a mut dyn FnMut(&str) -> Result<Availability, &'static str>,
    ) -> Self {
        Self {
            options,
            files,
            packages,
            presence,
            cache: BTreeMap::new(),
            probes: Vec::new(),
        }
    }

    /// Distinct presence observations in first-lookup order. Missing files and
    /// package boundaries are dependencies, not merely successful targets.
    #[must_use]
    pub fn probes(&self) -> &[Probe] {
        &self.probes
    }

    /// Load a normalized workspace-relative candidate. `substitution` is the
    /// unexpanded paths value supplied by the alias dispatcher, or `None` for an
    /// ordinary file/baseUrl lookup. Explicitly authored supported extensions get
    /// an exact-file attempt before replacement; wildcard-introduced ones do not.
    ///
    /// # Errors
    /// Invalid paths, exhausted probe budget or unmodeled directory-package rules.
    pub fn load(
        &mut self,
        candidate: &str,
        substitution: Option<&str>,
    ) -> Result<Option<String>, &'static str> {
        valid_path(candidate)?;
        if substitution.is_some_and(|text| known_extension(text).is_some())
            && let Some(path) = self.suffixed(candidate)?
        {
            return Ok(Some(path));
        }
        if !candidate.is_empty()
            && !candidate.ends_with('/')
            && let Some(path) = self.file(candidate)?
        {
            return Ok(Some(path));
        }
        if self.options.mode == Mode::NodeEsm {
            return Ok(None);
        }
        let directory = candidate.trim_end_matches('/');
        let package = in_directory(directory, "package.json");
        match self.observe(&package)? {
            Availability::Admitted | Availability::Unavailable => {
                return Err("ts_module_package_directory_unmodeled");
            }
            Availability::Unknown => return Err("ts_module_presence_unknown"),
            Availability::Absent => {}
        }
        self.file(&in_directory(directory, "index"))
    }

    fn file(&mut self, candidate: &str) -> Result<Option<String>, &'static str> {
        let name = candidate.rsplit('/').next().unwrap_or(candidate);
        if name.contains('.') {
            let extension = known_extension(candidate)
                .or_else(|| name.rfind('.').map(|position| &name[position..]));
            if let Some(extension) = extension {
                let stem = candidate
                    .strip_suffix(extension)
                    .ok_or("ts_module_path_unsupported")?;
                if let Some(path) = self.extensions(stem, extension)? {
                    return Ok(Some(path));
                }
            }
        }
        if self.options.mode == Mode::NodeEsm {
            return Ok(None);
        }
        self.extensions(candidate, "")
    }

    fn extensions(&mut self, stem: &str, original: &str) -> Result<Option<String>, &'static str> {
        let extensions: &[&str] = match original {
            ".mjs" | ".mts" | ".d.mts" => &[".mts", ".d.mts", ".mjs"],
            ".cjs" | ".cts" | ".d.cts" => &[".cts", ".d.cts", ".cjs"],
            ".tsx" | ".jsx" => &[".tsx", ".ts", ".d.ts", ".jsx", ".js"],
            ".ts" | ".d.ts" | ".js" | "" => &[".ts", ".tsx", ".d.ts", ".js", ".jsx"],
            ".json" if self.options.json => &[".d.json.ts", ".json"],
            ".json" => &[".d.json.ts"],
            other => return self.suffixed(&format!("{stem}.d{other}.ts")),
        };
        for extension in extensions {
            if let Some(path) = self.suffixed(&format!("{stem}{extension}"))? {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn suffixed(&mut self, candidate: &str) -> Result<Option<String>, &'static str> {
        let extension = known_extension(candidate).unwrap_or("");
        let stem = candidate
            .strip_suffix(extension)
            .ok_or("ts_module_path_unsupported")?;
        for suffix in &self.options.suffixes {
            let path = format!("{stem}{suffix}{extension}");
            match self.observe(&path)? {
                Availability::Admitted => return Ok(Some(path)),
                Availability::Absent => {}
                Availability::Unavailable => return Err("ts_module_target_unavailable"),
                Availability::Unknown => return Err("ts_module_presence_unknown"),
            }
        }
        Ok(None)
    }

    fn observe(&mut self, path: &str) -> Result<Availability, &'static str> {
        valid_path(path)?;
        if let Some(present) = self.cache.get(path) {
            return *present;
        }
        if self.probes.len() >= MAX_PROBES {
            return Err("ts_module_probe_limit");
        }
        let availability = if self.files.contains(path) {
            Ok(Availability::Admitted)
        } else if self.packages.contains(path) {
            Ok(Availability::Unavailable)
        } else {
            (self.presence)(path).and_then(|state| {
                if state == Availability::Admitted {
                    Err("ts_module_presence_inconsistent")
                } else {
                    Ok(state)
                }
            })
        };
        self.cache.insert(path.into(), availability);
        self.probes.push(Probe {
            path: path.into(),
            availability: availability.unwrap_or(Availability::Unknown),
            package_boundary: self.packages.contains(path),
        });
        availability
    }
}

fn valid_path(path: &str) -> Result<(), &'static str> {
    if path.len() > 4096
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || (!path.is_empty()
            && path
                .trim_end_matches('/')
                .split('/')
                .any(|part| matches!(part, "" | "." | "..")))
    {
        return Err("ts_module_path_unsupported");
    }
    Ok(())
}

fn in_directory(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        name.into()
    } else {
        format!("{directory}/{name}")
    }
}

// Literal, case-sensitive suffixes follow the compiler's precedence, including
// declaration suffixes and dotfiles; Path::extension is insufficient here.
#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn known_extension(path: &str) -> Option<&'static str> {
    [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ]
    .into_iter()
    .find(|extension| path.ends_with(extension))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unavailable_and_unknown_preferred_candidates_never_select_a_lower_priority_file() {
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = files(&["src/x.js"]);
        let packages = BTreeSet::new();
        for (state, reason) in [
            (Availability::Unavailable, "ts_module_target_unavailable"),
            (Availability::Unknown, "ts_module_presence_unknown"),
        ] {
            let mut presence = |_: &str| Ok(state);
            let mut lookup = Lookup::new(&options, &known, &packages, &mut presence);
            assert_eq!(lookup.load("src/x.js", None), Err(reason));
            assert_eq!(lookup.probes().len(), 1);
            assert_eq!(lookup.probes()[0].path, "src/x.ts");
            assert_eq!(lookup.probes()[0].availability, state);
        }
    }

    #[test]
    fn presence_errors_are_recorded_cached_and_cannot_invent_admission() {
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = files(&["x.js"]);
        let packages = BTreeSet::new();
        let mut calls = 0;
        let mut presence = |_: &str| {
            calls += 1;
            Err("ts_module_presence_limit")
        };
        let mut lookup = Lookup::new(&options, &known, &packages, &mut presence);
        assert_eq!(lookup.load("x.js", None), Err("ts_module_presence_limit"));
        assert_eq!(lookup.load("x.js", None), Err("ts_module_presence_limit"));
        assert_eq!(lookup.probes()[0].availability, Availability::Unknown);
        assert_eq!(calls, 1);
        let mut inconsistent = |_: &str| Ok(Availability::Admitted);
        let mut lookup = Lookup::new(&options, &known, &packages, &mut inconsistent);
        assert_eq!(
            lookup.load("x.js", None),
            Err("ts_module_presence_inconsistent")
        );
        assert_eq!(lookup.probes()[0].availability, Availability::Unknown);
    }

    fn compile_options(mode: Mode, extra: Value) -> Result<Options, &'static str> {
        let Value::Object(mut values) = extra else {
            panic!("object fixture required");
        };
        values.entry("moduleResolution").or_insert_with(|| {
            json!(if mode == Mode::Bundler {
                "bundler"
            } else {
                "nodenext"
            })
        });
        let config = EffectiveConfig {
            configuration: graph_search_types::typescript::TypeScriptConfig {
                fields: BTreeMap::from([("compilerOptions".into(), Value::Object(values))]),
                ..Default::default()
            },
            ..Default::default()
        };
        Options::compile(&config, mode)
    }

    fn files(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).into()).collect()
    }

    #[test]
    fn authored_extensions_and_wildcard_introduced_extensions_have_different_priority() {
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = files(&["src/x.js", "src/x.ts", "src/x.tsx"]);
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
        assert_eq!(
            lookup.load("src/x.js", Some("./src/x.js")).unwrap(),
            Some("src/x.js".into())
        );
        assert_eq!(
            lookup.load("src/x.js", Some("./src/*")).unwrap(),
            Some("src/x.ts".into())
        );
        assert_eq!(
            lookup.load("src/x.jsx", None).unwrap(),
            Some("src/x.tsx".into())
        );
        assert_eq!(lookup.probes().len(), 3);
    }

    #[test]
    fn modes_separate_extensionless_and_index_loading_from_esm() {
        let known = files(&["src/file.ts", "src/dir/index.ts", "src/dir.ts"]);
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        for mode in [Mode::Bundler, Mode::NodeCommonJs, Mode::NodeEsm] {
            let options = compile_options(mode, json!({})).unwrap();
            let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
            assert_eq!(
                lookup.load("src/file.js", None).unwrap(),
                Some("src/file.ts".into())
            );
            assert_eq!(
                lookup.load("src/file", None).unwrap(),
                (mode != Mode::NodeEsm).then(|| "src/file.ts".into())
            );
            assert_eq!(
                lookup.load("src/dir/", None).unwrap(),
                (mode != Mode::NodeEsm).then(|| "src/dir/index.ts".into())
            );
        }
    }

    #[test]
    fn suffix_order_is_inside_extension_order_and_omitted_empty_suffix_does_not_fall_back() {
        let known = files(&["x.ts", "x.tsx", "x.native.tsx", "x.native.d.ts"]);
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        let options =
            compile_options(Mode::Bundler, json!({"moduleSuffixes":[".native", ""]})).unwrap();
        let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
        assert_eq!(lookup.load("x", None).unwrap(), Some("x.ts".into()));
        assert_eq!(
            lookup
                .probes()
                .iter()
                .map(|probe| probe.path.as_str())
                .collect::<Vec<_>>(),
            ["x.native.ts", "x.ts"]
        );
        let only = compile_options(Mode::Bundler, json!({"moduleSuffixes":[".native"]})).unwrap();
        assert_eq!(
            Lookup::new(&only, &known, &packages, &mut absent)
                .load("x", None)
                .unwrap(),
            Some("x.native.tsx".into())
        );
        assert_eq!(
            Lookup::new(&only, &known, &packages, &mut absent)
                .load("x.d.ts", Some("x.d.ts"))
                .unwrap(),
            Some("x.native.d.ts".into())
        );
    }

    #[test]
    fn json_declaration_wrappers_runtime_families_and_custom_extensions_are_distinct() {
        let known = files(&[
            "x.d.mts",
            "y.cts",
            "data.json",
            "style.d.css.ts",
            "double.js.ts",
        ]);
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        let options = compile_options(Mode::Bundler, json!({"resolveJsonModule":false})).unwrap();
        let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
        for (candidate, expected) in [
            ("x.mjs", "x.d.mts"),
            ("y.cjs", "y.cts"),
            ("style.css", "style.d.css.ts"),
            ("double.js", "double.js.ts"),
        ] {
            assert_eq!(lookup.load(candidate, None).unwrap(), Some(expected.into()));
        }
        assert_eq!(lookup.load("data.json", None).unwrap(), None);
        assert_eq!(
            lookup.load("data.json", Some("data.json")).unwrap(),
            Some("data.json".into())
        );
    }

    #[test]
    fn package_boundaries_cannot_be_bypassed_or_mistaken_for_admitted_source() {
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = files(&["pkg/index.ts"]);
        let packages = files(&["pkg/package.json"]);
        let mut absent = |_: &str| Ok(Availability::Absent);
        let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
        assert_eq!(
            lookup.load("pkg", None),
            Err("ts_module_package_directory_unmodeled")
        );
        let boundary = lookup
            .probes()
            .iter()
            .find(|probe| probe.path == "pkg/package.json")
            .unwrap();
        assert!(boundary.availability == Availability::Unavailable && boundary.package_boundary);
        assert_eq!(
            lookup.load("pkg/package.json", Some("pkg/package.json")),
            Err("ts_module_target_unavailable")
        );
    }

    #[test]
    fn negative_observations_are_deduplicated_and_budget_is_shared_across_substitutions() {
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = BTreeSet::new();
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
        assert_eq!(lookup.load("missing", None).unwrap(), None);
        let count = lookup.probes().len();
        assert_eq!(lookup.load("missing", None).unwrap(), None);
        assert_eq!(lookup.probes().len(), count);
        let mut exhausted = false;
        for index in 0..MAX_PROBES {
            if lookup.load(&format!("missing{index}"), None) == Err("ts_module_probe_limit") {
                exhausted = true;
                break;
            }
        }
        assert!(exhausted);
        assert_eq!(lookup.probes().len(), MAX_PROBES);
        assert!(
            lookup
                .probes()
                .iter()
                .all(|probe| probe.availability == Availability::Absent && !probe.package_boundary)
        );
    }

    #[test]
    fn unsupported_options_and_noncanonical_candidates_fail_explicitly() {
        for extra in [
            json!({"moduleResolution":"classic"}),
            json!({"moduleResolution":"node10"}),
            json!({"resolveJsonModule":"yes"}),
            json!({"moduleSuffixes":[7]}),
            json!({"moduleSuffixes":["/escape"]}),
            json!({"moduleSuffixes":vec![""; MAX_SUFFIXES + 1]}),
            json!({"noDtsResolution":true}),
        ] {
            assert!(compile_options(Mode::Bundler, extra).is_err());
        }
        let options = compile_options(Mode::Bundler, json!({})).unwrap();
        let known = BTreeSet::new();
        let packages = BTreeSet::new();
        let mut absent = |_: &str| Ok(Availability::Absent);
        for candidate in ["../x", "/absolute", "a/../x", "a//x", "a\\x"] {
            let mut lookup = Lookup::new(&options, &known, &packages, &mut absent);
            assert_eq!(
                lookup.load(candidate, None),
                Err("ts_module_path_unsupported")
            );
            assert!(lookup.probes().is_empty());
        }
    }
}
