//! File-local syntactic bindings. This is lexical lookup, not alias dataflow.
use crate::walk::{span_of, text};
use graph_search_types::extraction::{BindingFact, Extraction, ScopeFact};
use graph_search_types::{EdgeKind, Span};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

pub(crate) fn enrich(root: Node<'_>, source: &str, extraction: &mut Extraction) {
    let mut index = Index {
        source,
        scopes: vec![ScopeFact {
            parent: None,
            span: span_of(root),
            kind: "file".into(),
        }],
        bindings: Vec::new(),
        locations: BTreeMap::new(),
        symbols: BTreeMap::new(),
        declarations: BTreeMap::new(),
        branch_scopes: BTreeMap::new(),
        target_bounds: BTreeMap::new(),
        namespace_targets: BTreeSet::new(),
        rust_imports: BTreeMap::new(),
        rust_globs: BTreeSet::new(),
        classes: crate::class_bindings::Classes::new(extraction),
    };
    let mut key_counts = BTreeMap::new();
    for symbol in &extraction.symbols {
        let count = key_counts.entry(symbol.key.as_str()).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    for symbol in &extraction.symbols {
        if key_counts[&symbol.key.as_str()] == 1 {
            index.target_bounds.insert(symbol.key.clone(), symbol.span);
            if matches!(
                symbol.kind,
                graph_search_types::NodeKind::Class
                    | graph_search_types::NodeKind::Struct
                    | graph_search_types::NodeKind::Enum
                    | graph_search_types::NodeKind::Module
            ) {
                index.namespace_targets.insert(symbol.key.clone());
            }
            index.symbols.insert(
                (
                    symbol.span.start_byte,
                    symbol.span.end_byte,
                    symbol.name.clone(),
                ),
                symbol.key.clone(),
            );
        }
    }
    let mut pending = vec![(root, 0)];
    while let Some((node, parent)) = pending.pop() {
        let scope = index.visit(node, parent);
        let span = span_of(node);
        index
            .locations
            .insert((span.start_byte, span.end_byte), scope);
        for i in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(u32::try_from(i).unwrap_or(u32::MAX)) {
                let child_scope = index
                    .branch_scopes
                    .get(&node.id())
                    .copied()
                    .filter(|&id| {
                        let span = index.scopes[id].span;
                        child.start_byte() >= span.start_byte as usize
                            && child.end_byte() <= span.end_byte as usize
                    })
                    .unwrap_or(scope);
                pending.push((child, child_scope));
            }
        }
    }
    index.imports(extraction);
    index.attach(extraction);
    extraction.scopes = index.scopes;
    extraction.bindings = index.bindings;
}

struct Index<'a> {
    source: &'a str,
    scopes: Vec<ScopeFact>,
    bindings: Vec<BindingFact>,
    locations: BTreeMap<(u32, u32), usize>,
    symbols: BTreeMap<(u32, u32, String), String>,
    declarations: BTreeMap<(u32, u32, String), usize>,
    branch_scopes: BTreeMap<usize, usize>,
    target_bounds: BTreeMap<String, Span>,
    namespace_targets: BTreeSet<String>,
    rust_imports: BTreeMap<usize, (String, bool)>,
    rust_globs: BTreeSet<usize>,
    classes: crate::class_bindings::Classes,
}

impl Index<'_> {
    fn imports(&mut self, extraction: &Extraction) {
        for reference in &extraction.references {
            let Some(import) = &reference.rust_use else {
                continue;
            };
            let Some(span) = reference.span else { continue };
            let Some(&scope) = self.locations.get(&(span.start_byte, span.end_byte)) else {
                continue;
            };
            if reference.dynamic {
                continue;
            }
            if import.glob {
                self.rust_globs.insert(scope);
                continue;
            }
            let Some(name) = &import.local_name else {
                continue;
            };
            let id = self.bindings.len();
            self.bindings.push(BindingFact {
                scope,
                name: name.clone(),
                span,
                visible_from: self.scopes[scope].span.start_byte,
                initialized_from: 0,
                kind: "rust_import".into(),
                target_key: None,
            });
            self.rust_imports
                .insert(id, (reference.name.clone(), import.type_only));
        }
    }

    fn scope(&mut self, parent: usize, span: Span, kind: &str) -> usize {
        let id = self.scopes.len();
        self.scopes.push(ScopeFact {
            parent: Some(parent),
            span,
            kind: kind.into(),
        });
        id
    }

    fn function_scope(&self, mut scope: usize) -> usize {
        while !matches!(self.scopes[scope].kind.as_str(), "function" | "file") {
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        scope
    }

    fn declaration(&mut self, node: Node<'_>, scope: usize) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let span = span_of(node);
        let spelling = text(name, self.source).to_owned();
        self.declarations
            .insert((span.start_byte, span.end_byte, spelling.clone()), scope);
        let key = self
            .symbols
            .get(&(span.start_byte, span.end_byte, spelling.clone()))
            .cloned();
        let class = matches!(
            node.kind(),
            "class_declaration" | "abstract_class_declaration"
        );
        if class && let Some(key) = &key {
            self.classes.record_deferred(key, node);
        }
        self.bindings.push(BindingFact {
            scope,
            name: spelling,
            span: span_of(name),
            visible_from: self.scopes[scope].span.start_byte,
            initialized_from: if class { span.end_byte } else { 0 },
            kind: "declaration".into(),
            target_key: key,
        });
    }

    fn pattern(
        &mut self,
        node: Node<'_>,
        scope: usize,
        visible: u32,
        initialized: u32,
        kind: &str,
    ) {
        for name in pattern_names(node) {
            self.bindings.push(BindingFact {
                scope,
                name: text(name, self.source).to_owned(),
                span: span_of(name),
                visible_from: visible,
                initialized_from: initialized,
                kind: kind.into(),
                target_key: None,
            });
        }
    }

    #[allow(clippy::too_many_lines)] // one rule per grammar binding construct
    fn visit(&mut self, node: Node<'_>, parent: usize) -> usize {
        let kind = node.kind();
        let span = span_of(node);
        let declaration = matches!(
            kind,
            "function_item"
                | "function_declaration"
                | "generator_function_declaration"
                | "class_declaration"
                | "abstract_class_declaration"
                | "struct_item"
                | "enum_item"
                | "mod_item"
        );
        if declaration {
            self.declaration(node, parent);
        }
        let scope_kind = match kind {
            "function_item"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "closure_expression"
            | "method_definition" => Some("function"),
            "block" | "statement_block" | "for_statement" | "for_in_statement"
            | "for_expression" | "catch_clause" | "match_arm" | "switch_body" => Some("block"),
            "class_declaration"
            | "abstract_class_declaration"
            | "class"
            | "impl_item"
            | "trait_item" => Some("class"),
            "mod_item" => Some("module"),
            _ => None,
        };
        let scope = scope_kind.map_or(parent, |kind| self.scope(parent, span, kind));
        self.condition_bindings(node, scope);
        if scope_kind == Some("function") {
            if let Some(parameters) = node
                .child_by_field_name("parameters")
                .or_else(|| node.child_by_field_name("parameter"))
            {
                self.pattern(parameters, scope, span.start_byte, 0, "parameter");
            }
            if matches!(kind, "function_expression" | "generator_function") {
                // A named expression's name is visible only inside that expression.
                self.declaration(node, scope);
            }
        }
        match kind {
            "let_declaration" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.pattern(pattern, scope, span.end_byte, span.end_byte, "value");
                }
            }
            "variable_declarator" => {
                if let Some(pattern) = node.child_by_field_name("name") {
                    let var = node
                        .parent()
                        .is_some_and(|p| p.kind() == "variable_declaration");
                    let owner = if var {
                        self.function_scope(scope)
                    } else {
                        scope
                    };
                    self.pattern(
                        pattern,
                        owner,
                        self.scopes[owner].span.start_byte,
                        span.end_byte,
                        "value",
                    );
                    self.direct_function(node, pattern, owner);
                }
            }
            "catch_clause" => {
                if let Some(pattern) = node.child_by_field_name("parameter") {
                    self.pattern(pattern, scope, span.start_byte, 0, "parameter");
                }
            }
            "for_expression" => {
                if let (Some(pattern), Some(body)) = (
                    node.child_by_field_name("pattern"),
                    node.child_by_field_name("body"),
                ) {
                    self.pattern(pattern, scope, span_of(body).start_byte, 0, "value");
                }
            }
            "for_in_statement" if node.child_by_field_name("kind").is_some() => {
                if let Some(pattern) = node.child_by_field_name("left") {
                    let var = node
                        .child_by_field_name("kind")
                        .is_some_and(|n| text(n, self.source) == "var");
                    let owner = if var {
                        self.function_scope(scope)
                    } else {
                        scope
                    };
                    self.pattern(
                        pattern,
                        owner,
                        self.scopes[owner].span.start_byte,
                        0,
                        "value",
                    );
                }
            }
            "match_arm" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.pattern(pattern, scope, span.start_byte, 0, "value");
                }
            }
            _ => {}
        }
        scope
    }

    fn direct_function(&mut self, node: Node<'_>, pattern: Node<'_>, scope: usize) {
        if pattern.kind() != "identifier"
            || !node
                .parent()
                .is_some_and(|p| text(p, self.source).trim_start().starts_with("const "))
            || !node.child_by_field_name("value").is_some_and(|value| {
                matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "generator_function_expression"
                )
            })
        {
            return;
        }
        let span = span_of(node);
        let name = text(pattern, self.source).to_owned();
        let key = (span.start_byte, span.end_byte, name);
        if let Some(binding) = self.bindings.last_mut() {
            binding.kind = "declaration".into();
            binding.target_key = self.symbols.get(&key).cloned();
            self.declarations.insert(key, scope);
        }
    }

    fn condition_bindings(&mut self, node: Node<'_>, parent: usize) {
        if !matches!(node.kind(), "if_expression" | "while_expression") {
            return;
        }
        let Some(condition) = node.child_by_field_name("condition") else {
            return;
        };
        let Some(body) = node
            .child_by_field_name("consequence")
            .or_else(|| node.child_by_field_name("body"))
        else {
            return;
        };
        let mut pending = vec![condition];
        let mut patterns = Vec::new();
        while let Some(part) = pending.pop() {
            match part.kind() {
                "let_condition" => {
                    if let Some(pattern) = part.child_by_field_name("pattern") {
                        patterns.push((pattern, span_of(part).end_byte));
                    }
                }
                "let_chain" => {
                    for i in (0..part.named_child_count()).rev() {
                        if let Some(child) = part.named_child(u32::try_from(i).unwrap_or(u32::MAX))
                        {
                            pending.push(child);
                        }
                    }
                }
                _ => {}
            }
        }
        if patterns.is_empty() {
            return;
        }
        let mut span = span_of(condition);
        span.end_byte = span_of(body).end_byte;
        span.end_line = span_of(body).end_line;
        let scope = self.scope(parent, span, "block");
        self.branch_scopes.insert(node.id(), scope);
        for (pattern, visible) in patterns {
            self.pattern(pattern, scope, visible, visible, "value");
        }
    }

    fn attach(&self, extraction: &mut Extraction) {
        let mut by_scope: Vec<BTreeMap<&str, Vec<usize>>> =
            vec![BTreeMap::new(); self.scopes.len()];
        for (id, binding) in self.bindings.iter().enumerate() {
            by_scope[binding.scope]
                .entry(&binding.name)
                .or_default()
                .push(id);
        }
        for names in &mut by_scope {
            for ids in names.values_mut() {
                ids.sort_by_key(|&id| {
                    (
                        self.bindings[id].visible_from,
                        self.bindings[id].span.start_byte,
                    )
                });
            }
        }
        let mut import_bytes = 8 * 1024 * 1024;
        for reference in &mut extraction.references {
            self.attach_reference(reference, &by_scope, &mut import_bytes);
        }
        self.attach_symbols(extraction);
    }

    #[allow(clippy::too_many_lines)] // ordered lexical/import precedence and provenance
    fn attach_reference(
        &self,
        reference: &mut graph_search_types::extraction::ReferenceFact,
        by_scope: &[BTreeMap<&str, Vec<usize>>],
        import_bytes: &mut usize,
    ) {
        let Some(span) = reference.span else { return };
        let Some(&scope) = self.locations.get(&(span.start_byte, span.end_byte)) else {
            return;
        };
        reference.scope = Some(scope);
        if reference.kind != EdgeKind::Calls {
            return;
        }
        if ["crate::", "self::", "super::"]
            .iter()
            .any(|prefix| reference.name.starts_with(prefix))
        {
            return;
        }
        let raw = reference.raw_name.as_deref().unwrap_or(&reference.name);
        let raw = if raw.contains("::") {
            reference.name.as_str()
        } else {
            raw
        };
        let base = raw.split(['.', ':']).next().unwrap_or(raw);
        let unqualified = base == raw;
        let mut current = Some(scope);
        let mut type_only_seen = false;
        while let Some(id) = current {
            let candidates = by_scope[id].get(base);
            let imports_present = candidates
                .is_some_and(|ids| ids.iter().any(|id| self.rust_imports.contains_key(id)));
            let eligible: Vec<_> = candidates
                .filter(|_| imports_present)
                .into_iter()
                .flatten()
                .filter(|&&id| {
                    if self.bindings[id].visible_from > span.start_byte {
                        return false;
                    }
                    if unqualified
                        && self
                            .rust_imports
                            .get(&id)
                            .is_some_and(|(_, type_only)| *type_only)
                    {
                        type_only_seen = true;
                        return false;
                    }
                    !(imports_present
                        && raw[base.len()..].starts_with("::")
                        && !self.rust_imports.contains_key(&id)
                        && (self.bindings[id].kind != "declaration"
                            || self.bindings[id]
                                .target_key
                                .as_ref()
                                .is_some_and(|key| !self.namespace_targets.contains(key))))
                })
                .copied()
                .collect();
            let selected = if imports_present {
                eligible.last()
            } else {
                candidates.and_then(|ids| {
                    let end = ids
                        .partition_point(|&id| self.bindings[id].visible_from <= span.start_byte);
                    end.checked_sub(1).and_then(|position| ids.get(position))
                })
            };
            if let Some(&binding_id) = selected {
                let binding = &self.bindings[binding_id];
                if imports_present
                    && matches!(binding.kind.as_str(), "rust_import" | "declaration")
                    && eligible
                        .iter()
                        .filter(|&&id| {
                            matches!(
                                self.bindings[id].kind.as_str(),
                                "rust_import" | "declaration"
                            )
                        })
                        .count()
                        > 1
                {
                    reference.dynamic = true;
                    reference.unresolved_reason = Some("rust_import_binding_ambiguous".into());
                    break;
                }
                if let Some((path, _)) = self.rust_imports.get(&binding_id) {
                    reference.binding = Some(binding_id);
                    if unqualified || raw[base.len()..].starts_with("::") {
                        if let Some(name) = expand_import(path, &raw[base.len()..], import_bytes) {
                            reference.via_import = Some(path.clone());
                            reference.name = name;
                        } else {
                            reference.dynamic = true;
                            reference.unresolved_reason =
                                Some("rust_import_expansion_limit".into());
                        }
                    } else {
                        reference.dynamic = true;
                        reference.unresolved_reason =
                            Some("rust_import_receiver_unsupported".into());
                    }
                    break;
                }
                if !unqualified
                    && raw[base.len()..].starts_with('.')
                    && let Some(member) = self.classes.resolve(binding, &raw[base.len()..], span)
                {
                    reference.binding = Some(binding_id);
                    match member {
                        crate::class_bindings::Member::Target(key, qualified) => {
                            reference.name = qualified.to_owned();
                            reference.lexical_target = Some(key.to_owned());
                        }
                        crate::class_bindings::Member::Unresolved(reason) => {
                            reference.dynamic = true;
                            reference.unresolved_reason = Some(reason.into());
                        }
                    }
                    break;
                }
                // A local callable is a value, not a namespace through
                // which an unrelated outer class/module can be reached.
                if !unqualified
                    && binding.kind == "declaration"
                    && (raw[base.len()..].starts_with("::")
                        || binding
                            .target_key
                            .as_ref()
                            .is_some_and(|key| self.namespace_targets.contains(key)))
                {
                    break;
                }
                reference.binding = Some(binding_id);
                let initialized = span.start_byte >= binding.initialized_from
                    || binding
                        .target_key
                        .as_ref()
                        .and_then(|key| self.target_bounds.get(key))
                        .is_some_and(|body| {
                            body.start_byte <= span.start_byte && span.end_byte <= body.end_byte
                        });
                if unqualified
                    && binding.kind == "declaration"
                    && binding.target_key.is_some()
                    && initialized
                {
                    reference.lexical_target.clone_from(&binding.target_key);
                } else {
                    reference.dynamic = true;
                    reference.unresolved_reason = Some(
                        if span.start_byte < binding.initialized_from {
                            "binding_before_initialization"
                        } else if !unqualified {
                            "lexical_member_target_unknown"
                        } else if binding.kind == "declaration" {
                            "lexical_declaration_without_unique_target"
                        } else {
                            "lexical_value_target_unknown"
                        }
                        .into(),
                    );
                }
                break;
            }
            if self.rust_globs.contains(&id) {
                reference.dynamic = true;
                reference.unresolved_reason = Some("rust_glob_exports_unavailable".into());
                break;
            }
            if self.scopes[id].kind == "module" {
                // Rust module items do not inherit the parent module's names.
                // Until an import binding supplies a target, keep this unresolved
                // rather than allowing core's bare-name fallback to cross it.
                reference.dynamic = true;
                reference.unresolved_reason = Some("rust_module_binding_unknown".into());
                break;
            }
            current = self.scopes[id].parent;
        }
        if type_only_seen && reference.binding.is_none() && !reference.dynamic {
            reference.dynamic = true;
            reference.unresolved_reason = Some("rust_import_value_namespace_missing".into());
        }
    }

    fn attach_symbols(&self, extraction: &mut Extraction) {
        for symbol in &mut extraction.symbols {
            // Explicit member bindings use the same identity proof as direct
            // declarations; graph methods need their original extraction key too.
            symbol
                .attributes
                .insert("lexical_key".into(), symbol.key.clone());
            let declaration = self.declarations.get(&(
                symbol.span.start_byte,
                symbol.span.end_byte,
                symbol.name.clone(),
            ));
            let enclosing = (symbol.kind != graph_search_types::NodeKind::Method)
                .then(|| {
                    self.locations
                        .get(&(symbol.span.start_byte, symbol.span.end_byte))
                })
                .flatten();
            if let Some(&scope) = declaration.or(enclosing) {
                let scope = &self.scopes[scope];
                symbol
                    .attributes
                    .insert("lexical_key".into(), symbol.key.clone());
                if matches!(scope.kind.as_str(), "block" | "function") {
                    symbol
                        .attributes
                        .insert("lexical_local".into(), "true".into());
                    symbol
                        .attributes
                        .insert("lexical_start".into(), scope.span.start_byte.to_string());
                    symbol
                        .attributes
                        .insert("lexical_end".into(), scope.span.end_byte.to_string());
                }
            }
        }
    }
}

/// Pattern-only traversal: keys, types, defaults and constructors are not bindings.
pub(crate) fn pattern_names(root: Node<'_>) -> Vec<Node<'_>> {
    let mut names = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node.kind() {
            "identifier"
            | "shorthand_property_identifier_pattern"
            | "shorthand_field_identifier" => names.push(node),
            "parameter" | "required_parameter" | "optional_parameter" => {
                if let Some(pattern) = node
                    .child_by_field_name("pattern")
                    .or_else(|| node.child_by_field_name("name"))
                {
                    pending.push(pattern);
                }
            }
            "pair_pattern" => {
                if let Some(value) = node.child_by_field_name("value") {
                    pending.push(value);
                }
            }
            "assignment_pattern" | "object_assignment_pattern" => {
                if let Some(left) = node.child_by_field_name("left") {
                    pending.push(left);
                }
            }
            "field_pattern" => {
                if let Some(pattern) = node
                    .child_by_field_name("pattern")
                    .or_else(|| node.child_by_field_name("name"))
                {
                    pending.push(pattern);
                }
            }
            "formal_parameters"
            | "closure_parameters"
            | "parameters"
            | "object_pattern"
            | "array_pattern"
            | "tuple_pattern"
            | "slice_pattern"
            | "struct_pattern"
            | "tuple_struct_pattern"
            | "reference_pattern"
            | "ref_pattern"
            | "mut_pattern"
            | "captured_pattern"
            | "or_pattern"
            | "rest_pattern"
            | "match_pattern" => {
                let excluded = node.child_by_field_name("type");
                let condition = node.child_by_field_name("condition");
                for i in (0..node.named_child_count()).rev() {
                    if let Some(child) = node.named_child(u32::try_from(i).unwrap_or(u32::MAX))
                        && Some(child) != excluded
                        && Some(child) != condition
                    {
                        pending.push(child);
                    }
                }
            }
            _ => {}
        }
    }
    names
}

// Bound added target/provenance bytes, not total parser memory. Fail before allocation.
fn expand_import(path: &str, suffix: &str, remaining: &mut usize) -> Option<String> {
    let length = path.len().checked_add(suffix.len())?;
    let cost = length.checked_add(path.len())?;
    if length > 4096 || cost > *remaining {
        return None;
    }
    *remaining = remaining.checked_sub(cost)?;
    Some(format!("{path}{suffix}"))
}

#[cfg(test)]
mod import_budget_tests {
    use super::expand_import;

    #[test]
    fn expansion_counts_utf8_provenance_and_stops_before_oversized_allocations() {
        let path = "é".repeat(2048);
        let mut budget = 8192;
        assert_eq!(
            expand_import(&path, "", &mut budget).as_deref(),
            Some(path.as_str())
        );
        assert_eq!(budget, 0);
        assert!(expand_import("a", "", &mut budget).is_none());
        let mut budget = 10_000;
        assert!(expand_import(&path, "x", &mut budget).is_none());
        assert_eq!(budget, 10_000);
        let mut budget = 8;
        assert_eq!(
            expand_import("é", "::go", &mut budget).as_deref(),
            Some("é::go")
        );
        assert_eq!(budget, 0);
    }
}
