//! Open Knowledge Format bundles (`SPEC.md` §7.6): which `.md` files are
//! bundle members, and where a cross-link or path-valued field points.
//!
//! OKF makes `index.md` optional in every directory, so membership is a
//! declared approximation over paths alone: a `.md` file is a member when its
//! own directory or an ancestor holds an `index.md`. The bundle root is the
//! outermost such directory.

use crate::resolve::{Resolution, SymbolTable};
use graph_search_types::NodeId;
use graph_search_types::extraction::ReferenceFact;
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::occurrence::ResolutionClass;
use std::collections::BTreeSet;

/// The reserved directory listing that marks a bundle directory.
const INDEX: &str = "index.md";

/// The directories (workspace-relative, `""` for the root) holding an
/// `index.md` among `paths`.
#[must_use]
pub fn index_dirs<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    paths
        .into_iter()
        .filter_map(|path| match path.rsplit_once('/') {
            Some((dir, INDEX)) => Some(dir.to_owned()),
            None if path == INDEX => Some(String::new()),
            _ => None,
        })
        .collect()
}

/// Whether `rel` is a `.md` file inside a bundle described by `dirs`.
#[must_use]
pub fn is_member(rel: &str, dirs: &BTreeSet<String>) -> bool {
    std::path::Path::new(rel)
        .extension()
        .is_some_and(|ext| ext == "md")
        && ancestors(rel).any(|dir| dirs.contains(dir))
}

/// The bundle root of the member `rel`: its outermost ancestor-or-self
/// directory holding an `index.md` among `known_files`.
#[must_use]
pub fn bundle_root(rel: &str, known_files: &BTreeSet<String>) -> Option<String> {
    ancestors(rel)
        .filter(|dir| known_files.contains(&join(dir, INDEX)))
        .last()
        .map(str::to_owned)
}

/// The directories containing `rel`, innermost first, ending at the root `""`.
fn ancestors(rel: &str) -> impl Iterator<Item = &str> {
    let mut next = Some(rel);
    std::iter::from_fn(move || {
        let current = next?;
        let parent = current.rsplit_once('/').map_or("", |(dir, _)| dir);
        next = (!parent.is_empty()).then_some(parent);
        Some(parent)
    })
}

fn join(dir: &str, tail: &str) -> String {
    if dir.is_empty() {
        tail.to_owned()
    } else {
        format!("{dir}/{tail}")
    }
}

/// Whether a destination names something outside the workspace (a URI with a
/// scheme) or only a place in the same document (a fragment). Neither is a
/// bundle path, so neither becomes a reference.
#[must_use]
pub fn is_external(destination: &str) -> bool {
    let destination = destination.trim_start_matches('<');
    if destination.is_empty() || destination.starts_with('#') {
        return true;
    }
    let scheme_len = destination
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-')))
        .unwrap_or(destination.len());
    destination
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && destination
            .get(scheme_len..)
            .is_some_and(|rest| rest.starts_with(':'))
}

/// Resolves a bundle path to a known workspace file.
///
/// A leading `/` is bundle-relative (OKF §6.1); anything else is relative to
/// the referring file's directory. A path-valued frontmatter field
/// (`bundle_fallback`) also tries the bundle root, since producers commonly
/// spell those paths from the root without the leading `/`. A directory names
/// its `index.md`, and an extensionless path names a concept id (`x` →
/// `x.md`).
#[must_use]
pub fn resolve_path(
    from_path: &str,
    destination: &str,
    known_files: &BTreeSet<String>,
    bundle_fallback: bool,
) -> Option<String> {
    if is_external(destination) {
        return None;
    }
    let cleaned = destination
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    let cleaned = cleaned.split(['?', '#']).next().unwrap_or(cleaned);
    let cleaned = percent_decode(cleaned);
    let root = bundle_root(from_path, known_files);
    let from_dir = from_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut bases = Vec::new();
    if let Some(rest) = cleaned.strip_prefix('/') {
        bases.push(normalize(root.as_deref().unwrap_or(""), rest));
    } else {
        bases.push(normalize(from_dir, &cleaned));
        if bundle_fallback && let Some(root) = root.as_deref() {
            bases.push(normalize(root, &cleaned));
        }
    }
    bases
        .into_iter()
        .flatten()
        .flat_map(|base| candidates(&base))
        .find(|candidate| known_files.contains(candidate))
}

/// The files a normalized path may name.
fn candidates(base: &str) -> Vec<String> {
    let trimmed = base.trim_end_matches('/');
    if base.is_empty() || base.ends_with('/') {
        return vec![join(trimmed, INDEX)];
    }
    if std::path::Path::new(trimmed)
        .extension()
        .is_some_and(|ext| ext == "md")
    {
        return vec![trimmed.to_owned()];
    }
    // A dot need not start an extension: `v1.2-plan` is a concept id.
    vec![
        trimmed.to_owned(),
        format!("{trimmed}.md"),
        join(trimmed, INDEX),
    ]
}

/// Joins `tail` onto `dir`, folding `.` and `..`; `None` when `..` climbs
/// above the workspace root. A result naming a directory ends in `/`.
fn normalize(dir: &str, tail: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in tail.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    let mut joined = parts.join("/");
    // A path that names a directory itself (`/`, `.`, `..`, `x/`) is its listing.
    let names_directory = tail.is_empty()
        || tail.ends_with('/')
        || tail
            .rsplit('/')
            .next()
            .is_some_and(|last| matches!(last, "." | ".."));
    if names_directory {
        joined.push('/');
    }
    Some(joined)
}

/// Decodes `%XX` escapes; a malformed escape or non-UTF-8 result is kept as
/// written.
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_owned();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let escaped = (byte == b'%')
            .then(|| bytes.get(index.saturating_add(1)..index.saturating_add(3)))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(decoded) = escaped {
            out.push(decoded);
            index = index.saturating_add(3);
        } else {
            out.push(byte);
            index = index.saturating_add(1);
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// Resolves an OKF `links_to` or `cites` reference: the concept of the target
/// document when it has one, else the target file. A path that names no
/// walked file is dangling (OKF §6.1: broken links are not malformed).
#[must_use]
pub(crate) fn resolve(
    fact: &ReferenceFact,
    from_path: &str,
    table: &SymbolTable,
    known_files: &BTreeSet<String>,
) -> Resolution {
    let fallback = fact.kind == EdgeKind::Cites;
    let Some(target) = resolve_path(from_path, &fact.name, known_files, fallback) else {
        return Resolution {
            class: ResolutionClass::Unresolved,
            reason: Some("okf_link_target_missing".into()),
            fact: fact.clone(),
            to: None,
            to_name: crate::resolve::canonical_dangling_name(&fact.name),
        };
    };
    // Qualified names are unique per file where bare names are not: a section
    // may share the concept's title, but its qualified name is `Title > …`.
    let concept = table.by_file_qualified.get(&target).and_then(|names| {
        names
            .values()
            .filter_map(|id| table.symbols.get(id))
            .find(|node| node.kind == NodeKind::Concept)
    });
    let (to, to_name) = match concept {
        Some(node) => (
            node.id.clone(),
            node.qualified_name
                .clone()
                .unwrap_or_else(|| target.clone()),
        ),
        None => (NodeId::file(&target), target),
    };
    Resolution {
        class: ResolutionClass::ExplicitImport,
        reason: None,
        fact: fact.clone(),
        to: Some(to),
        to_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|p| (*p).to_owned()).collect()
    }

    #[test]
    fn membership_follows_the_nearest_index_up_the_tree() {
        let dirs = index_dirs(["kb/index.md", "kb/metrics/revenue.md", "README.md"]);
        assert_eq!(dirs, known(&["kb"]));
        assert!(is_member("kb/metrics/revenue.md", &dirs));
        assert!(is_member("kb/index.md", &dirs));
        assert!(!is_member("README.md", &dirs));
        assert!(!is_member("kb/notes.txt", &dirs));
        assert!(!is_member("kbx/a.md", &dirs));
        let root = index_dirs(["index.md"]);
        assert!(is_member("docs/a.md", &root));
    }

    #[test]
    fn bundle_root_is_the_outermost_index() {
        let files = known(&["kb/index.md", "kb/metrics/index.md", "kb/metrics/a.md"]);
        assert_eq!(
            bundle_root("kb/metrics/a.md", &files).as_deref(),
            Some("kb")
        );
        assert_eq!(bundle_root("other/a.md", &files), None);
    }

    #[test]
    fn paths_resolve_relative_bundle_relative_and_by_concept_id() {
        let files = known(&[
            "kb/index.md",
            "kb/metrics/revenue.md",
            "kb/metrics/gross margin.md",
            "kb/tables/index.md",
            "kb/tables/orders.md",
            "kb/policies/p.md",
        ]);
        let from = "kb/metrics/revenue.md";
        let at = |dest: &str| resolve_path(from, dest, &files, false);
        assert_eq!(
            at("/tables/orders.md").as_deref(),
            Some("kb/tables/orders.md")
        );
        assert_eq!(
            at("../tables/orders.md#schema").as_deref(),
            Some("kb/tables/orders.md")
        );
        assert_eq!(
            at("./gross%20margin.md").as_deref(),
            Some("kb/metrics/gross margin.md")
        );
        assert_eq!(at("/tables/orders").as_deref(), Some("kb/tables/orders.md"));
        assert_eq!(at("../tables/").as_deref(), Some("kb/tables/index.md"));
        assert_eq!(
            at("<../tables/orders.md>").as_deref(),
            Some("kb/tables/orders.md")
        );
        assert_eq!(at("https://example.com/x.md"), None);
        assert_eq!(at("#schema"), None);
        assert_eq!(at("../../../../x.md"), None);
        assert_eq!(at("/").as_deref(), Some("kb/index.md"));
        assert_eq!(at("..").as_deref(), Some("kb/index.md"));
        // Only path-valued fields fall back to the bundle root.
        assert_eq!(at("policies/p.md"), None);
        assert_eq!(
            resolve_path(from, "policies/p.md", &files, true).as_deref(),
            Some("kb/policies/p.md")
        );
    }

    #[test]
    fn schemes_are_external_and_paths_are_not() {
        assert!(is_external("mailto:a@b.c"));
        assert!(is_external("bigquery:project.dataset"));
        assert!(!is_external("./a.md"));
        assert!(!is_external("/a.md"));
        assert!(!is_external("a.md"));
    }
}
