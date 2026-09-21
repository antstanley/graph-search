//! Bounded file-local ESM facts from the existing JS/TS grammars.
use graph_search_types::{
    extraction::Extraction,
    js_module::{JsExport, JsImport, JsModule},
};
use tree_sitter::Node;

const MAX_RECORDS: usize = 4096;
const MAX_TEXT: usize = 8 * 1024 * 1024;

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

fn token(node: Node<'_>, kind: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == kind)
}

fn child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

pub(crate) fn name(node: Node<'_>, source: &str) -> Option<String> {
    let raw = text(node, source);
    if raw.len() > 4096 || node.has_error() {
        return None;
    }
    match node.kind() {
        "identifier" | "type_identifier" | "default" => Some(raw.into()),
        "string" if !raw.contains('\\') => raw.get(1..raw.len().checked_sub(1)?).map(str::to_owned),
        _ => None,
    }
}

/// Collect after declarations/scopes so exported declarations retain their keys.
#[allow(clippy::too_many_lines)] // one bounded branch per ESM grammar construct
pub(crate) fn enrich(root: Node<'_>, source: &str, extraction: &mut Extraction) {
    let mut module = JsModule {
        complete: true,
        ..JsModule::default()
    };
    let mut bytes = 0usize;
    let mut declarations: Vec<_> = extraction
        .symbols
        .iter()
        .filter(|symbol| symbol.parent_key.is_none())
        .collect();
    declarations.sort_by_key(|symbol| symbol.span.start_byte);
    let mut cursor = root.walk();
    'statements: for statement in root.named_children(&mut cursor) {
        if !matches!(statement.kind(), "import_statement" | "export_statement") {
            continue;
        }
        module.is_module = true;
        if statement.has_error() {
            module.complete = false;
            continue;
        }
        let source_node = statement.child_by_field_name("source");
        let specifier = source_node.and_then(|node| name(node, source));
        if source_node.is_some() && specifier.is_none() {
            module.complete = false;
            continue;
        }
        let type_only = token(statement, "type");
        if statement.kind() == "import_statement" {
            let Some(specifier) = specifier else {
                module.complete = false;
                continue;
            };
            let Some(clause) = child(statement, "import_clause") else {
                continue;
            };
            let mut stack = vec![clause];
            while let Some(node) = stack.pop() {
                let binding = match node.kind() {
                    "import_specifier" => node.child_by_field_name("name").and_then(|original| {
                        Some((
                            name(
                                node.child_by_field_name("alias").unwrap_or(original),
                                source,
                            )?,
                            name(original, source)?,
                        ))
                    }),
                    "namespace_import" => child(node, "identifier")
                        .and_then(|local| Some((name(local, source)?, "*".into()))),
                    "identifier" => name(node, source).map(|local| (local, "default".into())),
                    _ => {
                        let mut cursor = node.walk();
                        stack.extend(node.named_children(&mut cursor));
                        continue;
                    }
                };
                if let Some((local, imported)) = binding {
                    let cost = local
                        .len()
                        .saturating_add(imported.len())
                        .saturating_add(specifier.len());
                    if module.imports.len().saturating_add(module.exports.len()) >= MAX_RECORDS
                        || bytes.saturating_add(cost) > MAX_TEXT
                    {
                        module.complete = false;
                        break;
                    }
                    bytes = bytes.saturating_add(cost);
                    module.imports.push(JsImport {
                        local,
                        imported,
                        source: specifier.clone(),
                        type_only: type_only || token(node, "type"),
                        span: crate::walk::span_of(node),
                    });
                } else {
                    module.complete = false;
                }
            }
        } else if let Some(clause) = child(statement, "export_clause") {
            let mut cursor = clause.walk();
            for node in clause.named_children(&mut cursor) {
                if node.kind() == "comment" {
                    continue;
                }
                let Some(original) = node.child_by_field_name("name") else {
                    module.complete = false;
                    continue;
                };
                let Some(exported) = name(
                    node.child_by_field_name("alias").unwrap_or(original),
                    source,
                ) else {
                    module.complete = false;
                    continue;
                };
                let local = name(original, source);
                if local.is_none() {
                    module.complete = false;
                }
                if !push_export(
                    &mut module,
                    &mut bytes,
                    JsExport {
                        exported,
                        local,
                        source: specifier.clone(),
                        type_only: type_only || token(node, "type"),
                        span: crate::walk::span_of(node),
                    },
                ) {
                    break 'statements;
                }
            }
        } else if let Some(namespace) = child(statement, "namespace_export") {
            let exported = namespace.named_child(0).and_then(|node| name(node, source));
            if let Some(exported) = exported {
                if !push_export(
                    &mut module,
                    &mut bytes,
                    JsExport {
                        exported,
                        local: Some("*".into()),
                        source: specifier.clone(),
                        type_only,
                        span: crate::walk::span_of(namespace),
                    },
                ) {
                    break 'statements;
                }
            } else {
                module.complete = false;
            }
        } else if token(statement, "*") {
            if !push_export(
                &mut module,
                &mut bytes,
                JsExport {
                    exported: "*".into(),
                    local: Some("*".into()),
                    source: specifier.clone(),
                    type_only,
                    span: crate::walk::span_of(statement),
                },
            ) {
                break 'statements;
            }
        } else if token(statement, "default") {
            let local = statement
                .child_by_field_name("declaration")
                .and_then(|node| node.child_by_field_name("name"))
                .or_else(|| statement.child_by_field_name("value"))
                .and_then(|node| name(node, source));
            if !push_export(
                &mut module,
                &mut bytes,
                JsExport {
                    exported: "default".into(),
                    local,
                    source: None,
                    type_only,
                    span: crate::walk::span_of(statement),
                },
            ) {
                break 'statements;
            }
        } else if let Some(declaration) = statement.child_by_field_name("declaration") {
            let span = crate::walk::span_of(declaration);
            let start =
                declarations.partition_point(|symbol| symbol.span.start_byte < span.start_byte);
            for symbol in declarations[start..]
                .iter()
                .take_while(|symbol| symbol.span.start_byte <= span.end_byte)
            {
                if symbol.span.end_byte <= span.end_byte
                    && !push_export(
                        &mut module,
                        &mut bytes,
                        JsExport {
                            exported: symbol.name.clone(),
                            local: Some(symbol.name.clone()),
                            source: None,
                            type_only,
                            span: symbol.span,
                        },
                    )
                {
                    break 'statements;
                }
            }
        } else {
            module.complete = false;
        }
    }
    for import in &module.imports {
        let mut reference = graph_search_types::extraction::ReferenceFact::file_level(
            graph_search_types::EdgeKind::Imports,
            &import.imported,
            import.span.start_line,
        )
        .at(import.span)
        .via_import(import.source.clone());
        reference.raw_name = Some(import.local.clone());
        extraction.references.push(reference);
    }
    for export in &module.exports {
        let Some(local) = export.local.as_deref().filter(|local| *local != "*") else {
            continue;
        };
        let mut reference = graph_search_types::extraction::ReferenceFact::file_level(
            graph_search_types::EdgeKind::Exports,
            local,
            export.span.start_line,
        )
        .at(export.span);
        reference.via_import.clone_from(&export.source);
        extraction.references.push(reference);
    }
    extraction.js_module = Some(module);
}

// Admit each record immediately: a large export clause never builds an unbounded
// temporary vector or clones its module specifier for all rejected leaves.
fn push_export(module: &mut JsModule, bytes: &mut usize, export: JsExport) -> bool {
    let cost = export
        .exported
        .len()
        .saturating_add(export.local.as_ref().map_or(0, String::len))
        .saturating_add(export.source.as_ref().map_or(0, String::len));
    if module.imports.len().saturating_add(module.exports.len()) >= MAX_RECORDS
        || bytes.saturating_add(cost) > MAX_TEXT
    {
        module.complete = false;
        return false;
    }
    *bytes = bytes.saturating_add(cost);
    module.exports.push(export);
    true
}
