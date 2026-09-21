//! Source-backed Rust use trees; logical target resolution is a separate phase.
use graph_search_types::EdgeKind;
use graph_search_types::extraction::{ReferenceFact, RustUseFact};
use tree_sitter::Node;

pub(crate) fn extract(
    node: Node<'_>,
    source: &str,
    visibility: Option<String>,
) -> Vec<ReferenceFact> {
    if let Some(facts) = leaves(node, source, visibility.as_deref()) {
        facts
    } else {
        let mut fact = ReferenceFact::file_level(
            EdgeKind::Imports,
            text(node, source),
            crate::walk::line_of(node.start_position().row),
        )
        .at(crate::walk::span_of(node));
        fact.raw_name = Some(text(node, source).into());
        fact.rust_use = Some(RustUseFact {
            type_only: false,
            local_name: None,
            glob: false,
            visibility,
        });
        fact.dynamic = true;
        fact.unresolved_reason = Some("rust_use_syntax_unsupported".into());
        vec![fact]
    }
}

fn leaves(node: Node<'_>, source: &str, visibility: Option<&str>) -> Option<Vec<ReferenceFact>> {
    if node.has_error() {
        return None;
    }
    let mut pending = vec![(node, String::new())];
    let mut facts = Vec::new();
    while let Some((node, prefix)) = pending.pop() {
        match node.kind() {
            "line_comment" | "block_comment" => {}
            "use_list" => {
                for i in (0..node.named_child_count()).rev() {
                    pending.push((node.named_child(u32::try_from(i).ok()?)?, prefix.clone()));
                }
            }
            "scoped_use_list" => {
                let path = if let Some(path) = node.child_by_field_name("path") {
                    path_text(path, source)?
                } else {
                    "::".into()
                };
                pending.push((node.child_by_field_name("list")?, join(&prefix, &path)));
            }
            _ => {
                let glob = node.kind() == "use_wildcard";
                let mut cursor = node.walk();
                let path_node = if node.kind() == "use_as_clause" {
                    Some(node.child_by_field_name("path")?)
                } else if glob {
                    node.named_children(&mut cursor)
                        .find(|n| !matches!(n.kind(), "line_comment" | "block_comment"))
                } else {
                    Some(node)
                };
                let leaf = if let Some(n) = path_node {
                    Some(path_text(n, source)?)
                } else {
                    None
                };
                let type_only = !glob
                    && leaf
                        .as_deref()
                        .is_some_and(|s| s == "self" || s.ends_with("::self"));
                let target = match leaf.as_deref() {
                    Some("self") if !prefix.is_empty() => prefix.clone(),
                    Some(leaf) if leaf.ends_with("::self") => {
                        join(&prefix, leaf.strip_suffix("::self")?)
                    }
                    Some(leaf) => join(&prefix, leaf),
                    None => prefix.clone(),
                };
                let local_name = if glob {
                    None
                } else if let Some(alias) = node.child_by_field_name("alias") {
                    let name = text(alias, source);
                    (name != "_").then(|| name.to_owned())
                } else {
                    target.rsplit("::").next().map(str::to_owned)
                };
                let name = if glob { join(&target, "*") } else { target };
                let mut fact = ReferenceFact::file_level(
                    EdgeKind::Imports,
                    name,
                    crate::walk::line_of(node.start_position().row),
                )
                .at(crate::walk::span_of(node));
                fact.raw_name = Some(text(node, source).into());
                fact.rust_use = Some(RustUseFact {
                    type_only,
                    local_name,
                    glob,
                    visibility: visibility.map(str::to_owned),
                });
                facts.push(fact);
            }
        }
    }
    Some(facts)
}

pub(crate) fn path_text(node: Node<'_>, source: &str) -> Option<String> {
    let mut pending = vec![node];
    let mut path = String::new();
    while let Some(node) = pending.pop() {
        match node.kind() {
            "line_comment" | "block_comment" => {}
            "identifier" | "crate" | "self" | "super" | "::" => path.push_str(text(node, source)),
            "scoped_identifier" => {
                for i in (0..node.child_count()).rev() {
                    pending.push(node.child(i)?);
                }
            }
            _ => return None,
        }
    }
    (!path.is_empty()).then_some(path)
}

fn join(prefix: &str, leaf: &str) -> String {
    if prefix.is_empty() {
        leaf.into()
    } else if prefix == "::" {
        format!("::{leaf}")
    } else {
        format!("{prefix}::{leaf}")
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}
