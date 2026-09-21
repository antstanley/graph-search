//! Native, bounded TypeScript include/exclude path patterns. These operate on
//! canonical workspace-relative paths; they neither walk nor admit source files.

/// One compiled, case-sensitive root-file pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    parts: Vec<String>,
    exclude: bool,
}

/// Shared comparison budget across one membership operation.
pub struct Budget(usize);

impl Default for Budget {
    fn default() -> Self {
        Self(2_000_000)
    }
}

impl Budget {
    fn charge(&mut self) -> Result<(), &'static str> {
        self.0 = self.0.checked_sub(1).ok_or("ts_project_pattern_limit")?;
        Ok(())
    }
}

impl Pattern {
    /// Compile relative to the configuration that declared the field. A final
    /// component without a dot or wildcard denotes an implicit recursive glob.
    ///
    /// # Errors
    /// Unsupported paths/templates, escaping the workspace, invalid recursive
    /// patterns or more than 256 normalized components.
    pub fn compile(directory: &str, spec: &str, exclude: bool) -> Result<Self, &'static str> {
        if directory.len() > 4096
            || directory.contains(['\\', ':', '\0'])
            || (!directory.is_empty()
                && directory
                    .split('/')
                    .any(|part| matches!(part, "" | "." | "..")))
        {
            return Err("ts_project_path_unsupported");
        }
        if spec.is_empty()
            || spec.len() > 4096
            || spec.starts_with('/')
            || spec.contains(['\\', ':', '\0'])
            || spec.contains("${")
        {
            return Err("ts_project_pattern_unsupported");
        }
        let mut parts: Vec<String> = directory
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let mut recursive = false;
        for part in spec.split('/') {
            match part {
                "" | "." => {}
                ".." if recursive => return Err("ts_project_pattern_unsupported"),
                ".." => {
                    parts.pop().ok_or("ts_project_workspace_escape")?;
                }
                other => {
                    recursive |= other == "**";
                    parts.push(other.into());
                }
            }
        }
        if !exclude && parts.last().is_some_and(|s| s == "**") {
            return Err("ts_project_pattern_unsupported");
        }
        if parts.last().is_none_or(|s| !s.contains(['.', '*', '?'])) {
            parts.extend(["**".into(), "*".into()]);
        }
        if parts.len() > 256 {
            return Err("ts_project_pattern_limit");
        }
        Ok(Self { parts, exclude })
    }

    /// Match one canonical file path, charging every dynamic-programming cell.
    /// Exclusion patterns also match path prefixes. Include wildcards suppress
    /// implicit hidden/package directories and implicit `.min.js` matches.
    ///
    /// # Errors
    /// Invalid candidate path or exhausted shared work budget.
    pub fn matches(&self, path: &str, budget: &mut Budget) -> Result<bool, &'static str> {
        if path.len() > 4096 || path.contains(['\\', ':', '\0']) {
            return Err("ts_project_path_unsupported");
        }
        let names: Vec<_> = path.split('/').collect();
        if names.len() > 256 || names.iter().any(|s| matches!(*s, "" | "." | "..")) {
            return Err("ts_project_path_unsupported");
        }
        let mut current = vec![false; names.len().saturating_add(1)];
        current[0] = true;
        for part in &self.parts {
            let mut next = vec![false; names.len().saturating_add(1)];
            for index in 0..=names.len() {
                budget.charge()?;
                if part == "**" {
                    next[index] |= current[index];
                    if index < names.len()
                        && next[index]
                        && (self.exclude
                            || (!names[index].starts_with('.') && !package(names[index])))
                    {
                        next[index.saturating_add(1)] = true;
                    }
                } else if index < names.len() && current[index] {
                    next[index.saturating_add(1)] = component(
                        part,
                        names[index],
                        self.exclude,
                        index.saturating_add(1) == names.len(),
                        budget,
                    )?;
                }
            }
            current = next;
        }
        Ok(if self.exclude {
            current.into_iter().any(|value| value)
        } else {
            current[names.len()]
        })
    }
}

fn package(name: &str) -> bool {
    matches!(name, "node_modules" | "bower_components" | "jspm_packages")
}

fn component(
    pattern: &str,
    name: &str,
    exclude: bool,
    final_name: bool,
    budget: &mut Budget,
) -> Result<bool, &'static str> {
    if !exclude && pattern.contains(['*', '?']) && package(name) {
        return Ok(false);
    }
    // TypeScript's wildcard regex has UTF-16 character semantics, including '?'.
    let pattern: Vec<_> = pattern.encode_utf16().collect();
    let name: Vec<_> = name.encode_utf16().collect();
    let min: Vec<_> = ".min.js".encode_utf16().collect();
    let mut current = vec![false; name.len().saturating_add(1)];
    current[0] = true;
    for (pi, token) in pattern.iter().enumerate() {
        let mut next = vec![false; name.len().saturating_add(1)];
        for ni in 0..=name.len() {
            budget.charge()?;
            if *token == u16::from(b'*') {
                next[ni] |= current[ni];
                if ni < name.len()
                    && next[ni]
                    && (exclude || pi != 0 || ni != 0 || name[ni] != u16::from(b'.'))
                    && (exclude || !final_name || name[ni..] != min)
                {
                    next[ni.saturating_add(1)] = true;
                }
            } else if ni < name.len() && current[ni] {
                next[ni.saturating_add(1)] = if *token == u16::from(b'?') {
                    exclude || pi != 0 || name[ni] != u16::from(b'.')
                } else {
                    *token == name[ni]
                };
            }
        }
        current = next;
    }
    Ok(current[name.len()])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_boundaries_hidden_paths_and_minified_files() {
        for (spec, path, expected) in [
            ("src", "src/a/b.ts", true),
            ("src", "src/.hidden/b.ts", false),
            ("src", "src/node_modules/b.ts", false),
            ("src/node_modules/*", "src/node_modules/b.ts", true),
            ("src/.hidden/*", "src/.hidden/b.ts", true),
            ("**/*", "src/a.min.js", false),
            ("**/*.min.js", "src/a.min.js", true),
            ("*.ts", ".ts", true),
            ("*.ts", ".hidden.ts", false),
            ("?.ts", "🦀.ts", false),
            ("??.ts", "🦀.ts", true),
            ("src/*", "src/deep/x.ts", false),
            ("src/**/x.ts", "src/x.ts", true),
        ] {
            assert_eq!(
                Pattern::compile("", spec, false)
                    .unwrap()
                    .matches(path, &mut Budget::default())
                    .unwrap(),
                expected,
                "{spec}: {path}"
            );
        }
    }
    #[test]
    fn excludes_match_prefixes_and_work_is_bounded() {
        assert!(
            Pattern::compile("pkg", "dist", true)
                .unwrap()
                .matches("pkg/dist/.hidden/a.ts", &mut Budget::default())
                .unwrap()
        );
        assert!(
            Pattern::compile("", "**/node_modules", true)
                .unwrap()
                .matches("pkg/node_modules/a.ts", &mut Budget::default())
                .unwrap()
        );
        assert_eq!(
            Pattern::compile("", "*", false)
                .unwrap()
                .matches("x.ts", &mut Budget(0)),
            Err("ts_project_pattern_limit")
        );
        for spec in ["../x", "src/**/../x", "src/**", "/src/*", "${configDir}/*"] {
            assert!(Pattern::compile("", spec, false).is_err());
        }
    }
}
