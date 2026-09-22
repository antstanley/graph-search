//! The extraction engine shared by the TypeScript and JavaScript dialects
//! (`SPEC.md` §7.2).
//!
//! The two grammars agree on statement and expression shapes and disagree on
//! the spelling of type positions; the dialect table captures the
//! differences.

use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_types::kind::{EdgeKind, Language, NodeKind, Visibility};
use graph_search_types::node::Span;
use tree_sitter::Node;

/// What the two dialects spell differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dialect {
    /// The language the facts claim.
    pub language: Language,
    /// Whether type positions exist (TypeScript).
    pub types: bool,
    /// Class field declarations: `public_field_definition` (TS) or
    /// `field_definition` (JS).
    pub field_kind: &'static str,
}

/// The extraction state for one file.
pub struct JsExtractor<'a> {
    /// The file's text.
    pub source: &'a str,
    /// The dialect table.
    pub dialect: Dialect,
    /// The facts accumulated so far.
    pub extraction: Extraction,
    /// The lexical scope stack: `(prefix, fact_key)`.
    pub scope: Vec<(String, String)>,
}

impl<'a> JsExtractor<'a> {
    // ------------------------------------------------------------------
    // Helpers (the same shape as the Rust extractor's)
    // ------------------------------------------------------------------

    /// The node's text.
    #[must_use]
    pub fn text(&self, node: Node<'_>) -> &'a str {
        let start = node.start_byte().min(self.source.len());
        let end = node.end_byte().min(self.source.len());
        self.source.get(start..end).unwrap_or_default()
    }

    fn span(node: Node<'_>) -> Span {
        crate::walk::span_of(node)
    }

    fn line(node: Node<'_>) -> u32 {
        crate::walk::line_of(node.start_position().row)
    }

    fn first_line(&self, node: Node<'_>) -> String {
        let first = self.text(node).lines().next().unwrap_or_default().trim();
        graph_search_core::text_search::truncate_line(first, 200)
    }

    fn qualify(&self, name: &str, kind: NodeKind) -> (String, String) {
        let mut qualified = self
            .scope
            .last()
            .map_or_else(String::new, |(name, _)| name.clone());
        if !qualified.is_empty() {
            qualified.push('.');
        }
        qualified.push_str(name);
        let mut key = format!("{}:{qualified}", kind.as_str());
        for (_, scope_key) in self.scope.iter().rev() {
            key = format!("{scope_key}>{key}");
        }
        (key, qualified)
    }

    fn emit(&mut self, node: Node<'_>, kind: NodeKind, name: String, signature: String) {
        let (key, qualified) = self.qualify(&name, kind);
        let parent = self.scope.last().map(|(_, parent)| parent.clone());
        let mut fact =
            SymbolFact::new(key.clone(), kind, name, qualified.clone(), Self::span(node))
                .with_signature(signature);
        if let Some(parent) = parent {
            fact = fact.with_parent(parent);
        }
        self.extraction.symbols.push(fact);
        self.scope.push((qualified, key));
    }

    fn reference(&mut self, kind: EdgeKind, name: String, node: Node<'_>) {
        self.reference_raw(kind, name, None, node);
    }

    /// Like [`Self::reference`], but records an explicit raw (source-spelled)
    /// name when the resolved `name` was rewritten from what the source wrote
    /// (e.g. `this.method` → `Class.method`). Preserving the original spelling
    /// keeps the lexical-binding pass seeing the real receiver.
    fn reference_raw(&mut self, kind: EdgeKind, name: String, raw: Option<String>, node: Node<'_>) {
        let owner = if kind == EdgeKind::Calls {
            self.scope.iter().rev().find(|(_, key)| {
                let local = key.rsplit('>').next().unwrap_or(key);
                local.starts_with("function:") || local.starts_with("method:")
            })
        } else {
            self.scope.last()
        };
        let fact = if let Some((_, key)) = owner {
            ReferenceFact::from_symbol(key.clone(), kind, name, Self::line(node))
        } else {
            ReferenceFact::file_level(kind, name, Self::line(node))
        };
        let mut fact = fact.at(Self::span(node));
        if kind == EdgeKind::Calls {
            fact.raw_name = Some(raw.unwrap_or_else(|| fact.name.clone()));
        }
        self.extraction.references.push(fact);
    }

    /// If `callee` is `this.<member>` written directly inside a class method,
    /// returns the enclosing class's qualified member name (`Class.member`), so
    /// the resolver can match it against the method's qualified name — mirroring
    /// the Rust `self.method()` rewrite. Arrow functions and blocks are
    /// transparent (they keep the lexical `this`), but a regular `function`
    /// expression between the call and the class rebinds `this`, so the walk
    /// refuses to guess through one.
    fn this_member_target(&self, callee: &str) -> Option<String> {
        let member = callee.strip_prefix("this.")?;
        if member.is_empty()
            || !member
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        {
            return None;
        }
        for (index, (qualified, key)) in self.scope.iter().enumerate().rev() {
            let local = key.rsplit('>').next().unwrap_or(key);
            if local.starts_with("function:") {
                return None;
            }
            if local.starts_with("method:") {
                // Only a method written directly in a class body shares the
                // class's `this`. An object-literal method (`{ run() {} }`), or
                // one nested under a field initializer or another method,
                // rebinds `this` to its own receiver, so the walk stops there.
                let parent_is_class = index
                    .checked_sub(1)
                    .and_then(|parent| self.scope.get(parent))
                    .is_some_and(|(_, parent_key)| {
                        parent_key
                            .rsplit('>')
                            .next()
                            .unwrap_or(parent_key)
                            .starts_with("class:")
                    });
                if !parent_is_class {
                    return None;
                }
            }
            if local.starts_with("class:") {
                return Some(format!("{qualified}.{member}"));
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // The walk: every arm consumes its subtree
    // ------------------------------------------------------------------

    /// Walks one node; every arm consumes its subtree.
    #[allow(clippy::too_many_lines)] // one arm per grammar construct
    pub fn walk_node(&mut self, node: Node<'_>) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| self.text(name).to_owned());
                if let Some(name) = name {
                    self.emit(node, NodeKind::Function, name, self.first_line(node));
                    self.walk_children(node);
                    self.scope.pop();
                }
            }
            "class_declaration" | "abstract_class_declaration" => {
                self.class(node);
            }
            "method_definition" => {
                let name = node
                    .child_by_field_name("name")
                    .map_or_else(String::new, |name| self.text(name).to_owned());
                self.emit(node, NodeKind::Method, name, self.first_line(node));
                let mut cursor = node.walk();
                let modifiers: Vec<_> = node
                    .children(&mut cursor)
                    .map(|child| child.kind())
                    .collect();
                if modifiers.contains(&"static")
                    && !modifiers.iter().any(|kind| matches!(*kind, "get" | "set"))
                    && let Some(method) = self.extraction.symbols.last_mut()
                {
                    method
                        .attributes
                        .insert("static_callable".into(), "true".into());
                }
                self.walk_children(node);
                self.scope.pop();
            }
            "interface_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| self.text(name).to_owned());
                if let Some(name) = name {
                    self.emit(node, NodeKind::Interface, name, self.first_line(node));
                    self.walk_children(node);
                    self.scope.pop();
                }
            }
            "type_alias_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| self.text(name).to_owned());
                if let Some(name) = name {
                    self.emit(node, NodeKind::TypeAlias, name, self.first_line(node));
                    // The aliased type is a type use.
                    if let Some(value) = node.child_by_field_name("value") {
                        self.type_use(value);
                    }
                    self.scope.pop();
                }
            }
            "enum_declaration" => {
                let name = node
                    .child_by_field_name("name")
                    .map(|name| self.text(name).to_owned());
                if let Some(name) = name {
                    self.emit(node, NodeKind::Enum, name, self.first_line(node));
                    self.walk_children(node);
                    self.scope.pop();
                }
            }
            "lexical_declaration" | "variable_declaration" => {
                self.declaration(node);
            }
            kind if kind == self.dialect.field_kind => {
                let name = node
                    .child_by_field_name("name")
                    .or_else(|| node.child_by_field_name("property"))
                    .map(|name| self.text(name).to_owned());
                if let Some(name) = name {
                    self.emit(node, NodeKind::Field, name, self.first_line(node));
                    self.walk_children(node);
                    self.scope.pop();
                }
            }
            "import_statement" => self.import(node),
            "export_statement" => self.export(node),
            "expression_statement"
            | "assignment_expression"
            | "return_statement"
            | "variable_declarator"
            | "parenthesized_expression"
            | "binary_expression"
            | "arguments"
            | "pair"
            | "array"
            | "statement_block"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "switch_statement"
            | "try_statement"
            | "lexical_declaration_unused" => self.walk_children(node),
            "call_expression" => self.call(node),
            "new_expression" => self.new_expression(node),
            "member_expression" => {
                // The object side may call or reference; the property side is
                // a member, not a use.
                if let Some(object) = node.child_by_field_name("object") {
                    self.walk_node(object);
                }
            }
            "class_heritage" => self.heritage(node),
            _ => {
                if self.dialect.types {
                    match node.kind() {
                        "type_annotation"
                        | "type_identifier"
                        | "generic_type"
                        | "nested_type_identifier"
                        | "nested_identifier" => {
                            self.type_use(node);
                        }
                        _ => self.walk_children(node),
                    }
                } else {
                    self.walk_children(node);
                }
            }
        }
    }

    fn walk_children(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.is_named() {
                    self.walk_node(child);
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn class(&mut self, node: Node<'_>) {
        let name = node.child_by_field_name("name").map_or_else(
            || String::from("(anonymous)"),
            |name| self.text(name).to_owned(),
        );
        self.emit(node, NodeKind::Class, name.clone(), self.first_line(node));
        // Heritage is a child node, unnamed by the grammar.
        if let Some(heritage) = (0..node.child_count())
            .map(|i| node.child(i))
            .find(|child| child.is_some_and(|c| c.kind() == "class_heritage"))
            .flatten()
        {
            self.heritage(heritage);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    fn heritage(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                match child.kind() {
                    "extends_clause" | "extends" => {
                        for name in self.type_names_in(child) {
                            self.reference(EdgeKind::Extends, name, child);
                        }
                    }
                    "implements_clause" | "implements" => {
                        for name in self.type_names_in(child) {
                            self.reference(EdgeKind::Implements, name, child);
                        }
                    }
                    _ => {}
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn declaration(&mut self, node: Node<'_>) {
        // `const x = ...`, `let y = ...` at any scope: a top-level binding
        // with a function value is a function; otherwise a const/variable.
        let is_const = self.text(node).starts_with("const");
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "variable_declarator"
                    && let Some(name_node) = child.child_by_field_name("name")
                {
                    // A destructured declaration binds every identifier in the
                    // pattern: `const { store } = require(...)`.
                    if name_node.kind() != "identifier" {
                        let pattern_names: Vec<_> = crate::scopes::pattern_names(name_node)
                            .into_iter()
                            .map(|name| self.text(name).to_owned())
                            .collect();
                        let value = child.child_by_field_name("value");
                        let kind = if is_const {
                            NodeKind::Const
                        } else {
                            NodeKind::Variable
                        };
                        for pattern_name in pattern_names {
                            self.emit(child, kind, pattern_name, self.first_line(child));
                            self.scope.pop();
                        }
                        // Computed keys and default values execute expressions;
                        // their identifiers are not additional declarations.
                        self.walk_node(name_node);
                        if let Some(value) = value {
                            self.walk_node(value);
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                        continue;
                    }
                    let name = self.text(name_node).to_owned();
                    let value = child.child_by_field_name("value");
                    let value_kind = value.map(|value| value.kind()).unwrap_or_default();
                    let kind = match value_kind {
                        "arrow_function"
                        | "function_expression"
                        | "generator_function_expression" => NodeKind::Function,
                        _ if is_const => NodeKind::Const,
                        _ => NodeKind::Variable,
                    };
                    // Exported const bindings often act as module values;
                    // the export pass adds the `exports` fact separately.
                    self.emit(child, kind, name, self.first_line(child));
                    if let Some(value) = value {
                        self.walk_node(value);
                    }
                    self.scope.pop();
                } else if child.is_named() && child.kind() != "variable_declarator" {
                    // `const`/`let` keywords and separators.
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fn import(&mut self, node: Node<'_>) {
        let Some(source) = node.child_by_field_name("source") else {
            return;
        };
        let specifier = self.string_text(source);
        let Some(specifier) = specifier else { return };
        // The module edge itself: file -> file.
        self.reference(EdgeKind::Imports, specifier, node);
        // Binding leaves are collected once by the native module-fact pass.
    }

    /// Attach explicit import provenance before workspace resolution.
    pub fn bind_imports(&mut self) {
        let Some(module) = &self.extraction.js_module else {
            return;
        };
        let mut bindings = std::collections::BTreeMap::new();
        for import in &module.imports {
            bindings
                .entry(import.local.as_str())
                .and_modify(|binding| *binding = None)
                .or_insert(Some(import));
        }
        let mut added_bytes = 0usize;
        for fact in &mut self.extraction.references {
            if fact.kind == EdgeKind::Imports
                || fact.dynamic
                || fact.lexical_target.is_some()
                || fact.via_import.is_some()
            {
                continue;
            }
            if !module.complete {
                fact.dynamic = true;
                fact.unresolved_reason = Some("js_module_surface_incomplete".into());
                continue;
            }
            let base = fact
                .name
                .split(['.', '[', '?'])
                .next()
                .unwrap_or(&fact.name);
            let Some(binding) = bindings.get(base) else {
                continue;
            };
            let reason = if let Some(import) = binding {
                if import.type_only && fact.kind == EdgeKind::Calls {
                    fact.dynamic = true;
                    fact.unresolved_reason = Some("js_type_only_import".into());
                    continue;
                }
                let target = if import.imported == "*" {
                    fact.name
                        .strip_prefix(import.local.as_str())
                        .and_then(|name| name.strip_prefix('.'))
                        .filter(|name| {
                            !name.is_empty()
                                && name
                                    .chars()
                                    .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                        })
                } else if fact.name == import.local {
                    Some(import.imported.as_str())
                } else {
                    None
                };
                if let Some(target) = target {
                    let cost = import.source.len().saturating_add(target.len());
                    if target.len() > 4096 || added_bytes.saturating_add(cost) > 8 * 1024 * 1024 {
                        Some("js_import_binding_limit")
                    } else {
                        added_bytes = added_bytes.saturating_add(cost);
                        fact.via_import = Some(import.source.clone());
                        fact.name = target.to_owned();
                        None
                    }
                } else {
                    Some("js_import_member_unmodeled")
                }
            } else {
                Some("js_import_binding_ambiguous")
            };
            if let Some(reason) = reason {
                fact.dynamic = true;
                fact.unresolved_reason = Some(reason.into());
            }
        }
    }

    fn export(&mut self, node: Node<'_>) {
        // `export { a, b }`, `export { a } from "./x"`, `export default X`,
        // `export <declaration>`.
        if let Some(declaration) = node.child_by_field_name("declaration") {
            self.walk_node(declaration);
        } else if let Some(value) = node.child_by_field_name("value") {
            self.walk_node(value);
        }
        if let Some(source) = node.child_by_field_name("source")
            && let Some(specifier) = self.string_text(source)
        {
            self.reference(EdgeKind::Imports, specifier, node);
        }
    }

    fn call(&mut self, node: Node<'_>) {
        if let Some(function) = node.child_by_field_name("function") {
            let callee = self.text(function).trim().to_owned();
            if callee == "require" {
                // `const x = require("./x")`: the module edge.
                if let Some(args) = node.child_by_field_name("arguments")
                    && let Some(first) = first_string(args, self.source)
                {
                    self.reference(EdgeKind::Imports, first, node);
                }
            } else if !callee.is_empty() {
                if let Some(target) = self.this_member_target(&callee) {
                    self.reference_raw(EdgeKind::Calls, target, Some(callee), node);
                } else {
                    self.reference(EdgeKind::Calls, callee, node);
                }
            }
        }
        if let Some(function) = node.child_by_field_name("function") {
            self.walk_node(function);
        }
        if let Some(args) = node.child_by_field_name("arguments") {
            self.walk_children(args);
        }
    }

    fn new_expression(&mut self, node: Node<'_>) {
        if let Some(constructor) = node.child_by_field_name("constructor") {
            let name = self.text(constructor).trim().to_owned();
            if !name.is_empty() {
                self.reference(EdgeKind::Calls, name, node);
            }
        }
        if let Some(args) = node.child_by_field_name("arguments") {
            self.walk_children(args);
        }
    }

    fn type_use(&mut self, node: Node<'_>) {
        for name in self.type_names_in(node) {
            self.reference(EdgeKind::TypeUses, name, node);
        }
    }

    /// The named types a node (or its subtree, for annotations) refers to.
    fn type_names_in(&self, node: Node<'_>) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            match current.kind() {
                "type_identifier" | "identifier" => {
                    names.push(self.text(current).to_owned());
                }
                _ => {
                    let mut cursor = current.walk();
                    if cursor.goto_first_child() {
                        loop {
                            let child = cursor.node();
                            if child.is_named() {
                                stack.push(child);
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }

    fn string_text(&self, node: Node<'_>) -> Option<String> {
        crate::js_modules::name(node, self.source)
    }
}

fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    let first = node
        .named_children(&mut cursor)
        .find(|child| child.kind() != "comment")?;
    (first.kind() == "string")
        .then(|| crate::js_modules::name(first, source))
        .flatten()
}

/// Exposes the visibility helper to the dialect wrappers; TS/JS has none, so
/// everything exported is public and everything else is private.
#[must_use]
pub const fn visibility_for_exported(exported: bool) -> Option<Visibility> {
    if exported {
        Some(Visibility::Public)
    } else {
        None
    }
}

/// The language the dialect claims, re-exposed for wrappers.
#[must_use]
pub const fn language_of(dialect: &Dialect) -> Language {
    dialect.language
}
