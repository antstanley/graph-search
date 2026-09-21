//! Inputs consulted by cross-file binding, separate from source/display changes.
//!
//! Keep this comparison aligned with `resolve`, `js_modules` and `rust_modules`/`rust_paths`.
//! The changed file is always reprojected: its own lexical coordinates, calls and
//! documentation must never be reused merely because its outward surface agrees.
use graph_search_types::{Language, Node, extraction::Extraction, js_module::JsModule};

fn binding_node(node: &Node) -> Node {
    let mut node = node.clone();
    // These affect source presentation or this file's own reference resolution,
    // not an unchanged other file's choice of a target.
    node.span = None;
    node.signature = None;
    node.is_async = false;
    node.attributes.remove("lexical_start");
    node.attributes.remove("lexical_end");
    node
}

pub(crate) fn module(facts: &Extraction) -> Option<JsModule> {
    let mut module = facts.js_module.clone()?;
    for import in &mut module.imports {
        import.span = graph_search_types::Span::default();
    }
    for export in &mut module.exports {
        export.span = graph_search_types::Span::default();
    }
    Some(module)
}

/// Compact identity of exactly the inputs compared by `unchanged`.
pub(crate) fn fingerprint<'a>(
    nodes: impl IntoIterator<Item = &'a Node>,
    facts: &Extraction,
    language: Language,
) -> Option<String> {
    if !matches!(
        language,
        Language::Rust | Language::JavaScript | Language::TypeScript
    ) || (matches!(language, Language::JavaScript | Language::TypeScript)
        && facts.js_module.is_none())
    {
        return None;
    }
    let mut nodes: Vec<_> = nodes
        .into_iter()
        .filter(|node| !node.is_file())
        .map(binding_node)
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    // Rust does not consult an ECMAScript surface.
    let module = (language != Language::Rust).then(|| module(facts));
    serde_json::to_vec(&(language, nodes, module))
        .ok()
        .map(|bytes| crate::hash::content_hash(&bytes))
}

/// Unknown or unsupported surfaces require conservative consumer repair.
/// IDs, kind, parentage, visibility, names and semantic attributes remain exact;
/// signature edits alone do not alter this resolver's kind-based target choice.
pub(crate) fn unchanged(
    old: &[&Node],
    new: &[Node],
    old_facts: &Extraction,
    new_facts: &Extraction,
    language: Language,
) -> bool {
    if !matches!(
        language,
        Language::Rust | Language::JavaScript | Language::TypeScript
    ) {
        return false;
    }
    if matches!(language, Language::JavaScript | Language::TypeScript)
        && (old_facts.js_module.is_none()
            || new_facts.js_module.is_none()
            || module(old_facts) != module(new_facts))
    {
        return false;
    }
    let mut old: Vec<_> = old
        .iter()
        .filter(|node| !node.is_file())
        .map(|node| binding_node(node))
        .collect();
    let mut new: Vec<_> = new.iter().map(binding_node).collect();
    old.sort_by(|a, b| a.id.cmp(&b.id));
    new.sort_by(|a, b| a.id.cmp(&b.id));
    old == new
}
