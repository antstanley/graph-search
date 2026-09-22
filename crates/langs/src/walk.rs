//! Small tree-walking helpers shared by the extractors.

/// Depth-first walk over the named tree, starting at `root`.
///
/// The visitor returns whether to descend into the node it was shown; `false`
/// prunes the subtree. Anonymous tokens are skipped: extraction speaks the
/// grammar's named nodes.
pub fn walk<F>(root: tree_sitter::Node<'_>, visit: &mut F)
where
    F: FnMut(tree_sitter::Node<'_>) -> bool,
{
    if root.child_count() == 0 {
        let _ = visit(root);
        return;
    }
    if !visit(root) {
        return;
    }
    let mut cursor = root.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.child_count() > 0 || visit(child) {
                walk(child, visit);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// The `field_name`-addressed child of a node, when present.
#[must_use]
pub fn field<'a>(node: tree_sitter::Node<'a>, name: &str) -> Option<tree_sitter::Node<'a>> {
    node.child_by_field_name(name)
}

/// The node's text, borrowed from `source`.
#[must_use]
pub fn text<'a>(node: tree_sitter::Node<'a>, source: &'a str) -> &'a str {
    let start = node.start_byte().min(source.len());
    let end = node.end_byte().min(source.len());
    source.get(start..end).unwrap_or_default()
}

/// A [`Span`] for a tree node, with saturating positions.
#[must_use]
pub fn span_of(node: tree_sitter::Node<'_>) -> graph_search_types::node::Span {
    graph_search_types::node::Span::new(
        line_of(node.start_position().row),
        line_of(node.end_position().row),
        u32::try_from(node.start_byte()).unwrap_or(u32::MAX),
        u32::try_from(node.end_byte()).unwrap_or(u32::MAX),
    )
}

/// A declaration's fact key and qualified name under its immediate parent
/// (that parent's key and qualified name), joining qualified segments with
/// `separator`.
#[must_use]
pub fn qualify(
    parent: Option<(&str, &str)>,
    name: &str,
    kind: graph_search_types::kind::NodeKind,
    separator: &str,
) -> (String, String) {
    let mut qualified = parent.map_or_else(String::new, |(_, qualified)| qualified.to_owned());
    if !qualified.is_empty() {
        qualified.push_str(separator);
    }
    qualified.push_str(name);
    let mut key = format!("{}:{qualified}", kind.as_str());
    // The immediate parent's key already contains its full ancestry.
    // Prepending every ancestor again makes nested keys grow exponentially.
    if let Some((parent_key, _)) = parent {
        key = format!("{parent_key}>{key}");
    }
    (key, qualified)
}

/// The 1-based line of a 0-based row.
#[must_use]
pub fn line_of(row: usize) -> u32 {
    u32::try_from(row).unwrap_or(u32::MAX).saturating_add(1)
}
