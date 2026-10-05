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
/// Whether every syntax error in `statement` lies inside a function or class
/// body. Such an error cannot change what the statement imports or exports
/// (`export function f() { db<Row[]>`..` }` still exports `f`).
fn errors_inside_bodies(statement: Node<'_>) -> bool {
    let mut stack = vec![(statement, false)];
    while let Some((node, in_body)) = stack.pop() {
        if (node.is_error() || node.is_missing()) && !in_body {
            return false;
        }
        if !node.has_error() {
            continue;
        }
        let in_body = in_body || matches!(node.kind(), "statement_block" | "class_body");
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor).map(|child| (child, in_body)));
    }
    true
}

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
        if statement.has_error() && !errors_inside_bodies(statement) {
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

/// `const { a, b: c } = await import("x")` and `const m = await import("x")`
/// bind names to another module as a static import does (the test idiom of
/// mocking first, then importing). Lexical scoping leaves those bindings
/// without a target; calls through them gain the import's provenance.
pub(crate) fn bind_dynamic_imports(root: Node<'_>, source: &str, extraction: &mut Extraction) {
    // Binding identifier span -> (imported name, specifier).
    let mut imported: std::collections::BTreeMap<(u32, u32), (String, String)> =
        std::collections::BTreeMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "variable_declarator"
            && let (Some(pattern), Some(value)) = (
                node.child_by_field_name("name"),
                node.child_by_field_name("value"),
            )
            && let Some(specifier) = dynamic_import(value, source)
        {
            let mut bind = |local: Node<'_>, name: String| {
                let span = crate::walk::span_of(local);
                imported.insert((span.start_byte, span.end_byte), (name, specifier.clone()));
            };
            match pattern.kind() {
                "identifier" => bind(pattern, "*".into()),
                "object_pattern" => {
                    let mut cursor = pattern.walk();
                    for property in pattern.named_children(&mut cursor) {
                        match property.kind() {
                            "shorthand_property_identifier_pattern" => {
                                bind(property, crate::walk::text(property, source).to_owned());
                            }
                            "pair_pattern" => {
                                if let (Some(key), Some(value)) = (
                                    property.child_by_field_name("key"),
                                    property.child_by_field_name("value"),
                                ) && value.kind() == "identifier"
                                    && key.kind() == "property_identifier"
                                {
                                    bind(value, crate::walk::text(key, source).to_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    if imported.is_empty() {
        return;
    }
    for fact in &mut extraction.references {
        if fact.kind != graph_search_types::EdgeKind::Calls || fact.via_import.is_some() {
            continue;
        }
        let Some(binding) = fact.binding.and_then(|id| extraction.bindings.get(id)) else {
            continue;
        };
        let Some((name, specifier)) =
            imported.get(&(binding.span.start_byte, binding.span.end_byte))
        else {
            continue;
        };
        let target = if name == "*" {
            fact.name
                .strip_prefix(binding.name.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
                .filter(|member| {
                    !member.is_empty()
                        && member
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                })
        } else {
            (fact.name == binding.name).then_some(name.as_str())
        };
        if let Some(target) = target {
            fact.name = target.to_owned();
            fact.via_import = Some(specifier.clone());
            fact.dynamic = false;
            fact.unresolved_reason = None;
            fact.lexical_target = None;
        }
    }
}

/// The specifier of `import("x")`, awaited or not.
fn dynamic_import(value: Node<'_>, source: &str) -> Option<String> {
    let call = if value.kind() == "await_expression" {
        value.named_child(0)?
    } else {
        value
    };
    if call.kind() != "call_expression" || call.child_by_field_name("function")?.kind() != "import"
    {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let first = arguments
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment")?;
    (first.kind() == "string")
        .then(|| name(first, source))
        .flatten()
}
