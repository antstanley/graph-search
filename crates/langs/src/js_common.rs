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
        Span::new(
            crate::walk::line_of(node.start_position().row),
            crate::walk::line_of(node.end_position().row),
            crate::walk::line_of(node.start_byte()),
            crate::walk::line_of(node.end_byte()),
        )
    }

    fn line(node: Node<'_>) -> u32 {
        crate::walk::line_of(node.start_position().row)
    }

    fn first_line(&self, node: Node<'_>) -> String {
        let first = self.text(node).lines().next().unwrap_or_default().trim();
        graph_search_core::text_search::truncate_line(first, 200)
    }

    fn qualify(&self, name: &str, kind: NodeKind) -> (String, String) {
        let mut qualified = String::new();
        for (prefix, _) in &self.scope {
            if !qualified.is_empty() {
                qualified.push('.');
            }
            qualified.push_str(prefix);
        }
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
        let line = Self::line(node);
        if let Some((scope_key, _)) = self.scope.last() {
            self.extraction.references.push(ReferenceFact::from_symbol(
                scope_key.clone(),
                kind,
                name,
                line,
            ));
        } else {
            self.extraction
                .references
                .push(ReferenceFact::file_level(kind, name, line));
        }
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
                        let mut pattern_names: Vec<String> = Vec::new();
                        collect_pattern_names(name_node, self.source, &mut pattern_names);
                        let value = child.child_by_field_name("value");
                        let kind = if is_const {
                            NodeKind::Const
                        } else {
                            NodeKind::Variable
                        };
                        let pattern_count = pattern_names.len();
                        for pattern_name in pattern_names {
                            self.emit(child, kind, pattern_name, self.first_line(child));
                        }
                        if let Some(value) = value {
                            self.walk_node(value);
                        }
                        for _ in 0..pattern_count {
                            self.scope.pop();
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
        self.reference(EdgeKind::Imports, specifier.clone(), node);
        // Each imported binding came in through this specifier.
        let mut bindings: Vec<String> = Vec::new();
        if let Some(clause) = (0..node.child_count())
            .map(|i| node.child(i))
            .find(|child| child.is_some_and(|c| c.kind() == "import_clause"))
            .flatten()
        {
            collect_bindings(clause, self.source, &mut bindings);
        }
        for binding in bindings {
            let mut fact = ReferenceFact::file_level(EdgeKind::Imports, binding, Self::line(node));
            fact = fact.via_import(specifier.clone());
            self.extraction.references.push(fact);
        }
    }

    fn export(&mut self, node: Node<'_>) {
        // `export { a, b }`, `export { a } from "./x"`, `export default X`,
        // `export <declaration>`.
        if let Some(clause) = node.child_by_field_name("export_clause") {
            let names = export_names(clause, self.source);
            for name in names {
                self.reference(EdgeKind::Exports, name, node);
            }
        }
        if let Some(declaration) = node.child_by_field_name("declaration") {
            let exported = match declaration.kind() {
                "function_declaration"
                | "generator_function_declaration"
                | "class_declaration"
                | "abstract_class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration" => declaration_name(declaration, self.source),
                "lexical_declaration" | "variable_declaration" => {
                    let mut names: Vec<String> = Vec::new();
                    let mut cursor = declaration.walk();
                    if cursor.goto_first_child() {
                        loop {
                            if cursor.node().kind() == "variable_declarator"
                                && let Some(name) = cursor.node().child_by_field_name("name")
                            {
                                names.push(self.text(name).to_owned());
                            }
                            if !cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                    names.into_iter().next()
                }
                _ => None,
            };
            if let Some(name) = exported {
                self.reference(EdgeKind::Exports, name, node);
            }
            self.walk_node(declaration);
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
                self.reference(EdgeKind::Calls, callee, node);
            }
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
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            if current.kind() == "string_fragment" {
                return Some(self.text(current).to_owned());
            }
            let mut cursor = current.walk();
            if cursor.goto_first_child() {
                loop {
                    stack.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
        None
    }
}

/// The declared name of an exportable declaration (the identifier after the
/// keyword; the grammar names no field on several of them).
fn declaration_name<'a>(declaration: Node<'a>, source: &'a str) -> Option<String> {
    (0..declaration.child_count())
        .map(|i| declaration.child(i))
        .find(|child| child.is_some_and(|c| matches!(c.kind(), "identifier" | "type_identifier")))
        .flatten()
        .map(|name| text_of(name, source).to_owned())
}

/// The bound names inside a destructuring pattern.
fn collect_pattern_names(pattern: Node<'_>, source: &str, out: &mut Vec<String>) {
    let mut stack = vec![pattern];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "shorthand_property_identifier_pattern" | "identifier" => {
                out.push(text_of(current, source).to_owned());
            }
            _ => {
                let mut cursor = current.walk();
                if cursor.goto_first_child() {
                    loop {
                        stack.push(cursor.node());
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// The imported binding names of an import clause.
fn collect_bindings(clause: Node<'_>, source: &str, out: &mut Vec<String>) {
    let mut stack = vec![clause];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "named_imports" => {
                let mut cursor = current.walk();
                if cursor.goto_first_child() {
                    loop {
                        let child = cursor.node();
                        if child.kind() == "import_specifier" {
                            // The grammar names no fields: the children are
                            // identifiers, the second being a local alias.
                            // Resolve by the *exported* (first) name.
                            let identifiers: Vec<&str> = (0..child.child_count())
                                .filter_map(|i| child.child(i))
                                .filter(|part| part.kind() == "identifier")
                                .map(|part| text_of(part, source))
                                .collect();
                            if let Some(exported) = identifiers.first() {
                                out.push((*exported).to_owned());
                            }
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            "namespace_import" => {
                // `import * as ns`: the alias is a local binding only.
            }
            "identifier" => {
                // Default import: the module's default export.
                out.push(text_of(current, source).to_owned());
            }
            _ => {
                let mut cursor = current.walk();
                if cursor.goto_first_child() {
                    loop {
                        stack.push(cursor.node());
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
        }
    }
}

/// The exported names of an export clause, using the *source* name
/// (`{ a as b }` exports `a`).
fn export_names(clause: Node<'_>, source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack = vec![clause];
    while let Some(current) = stack.pop() {
        if current.kind() == "export_specifier" {
            if let Some(name) = current.child_by_field_name("name") {
                names.push(text_of(name, source).to_owned());
            }
        } else {
            let mut cursor = current.walk();
            if cursor.goto_first_child() {
                loop {
                    stack.push(cursor.node());
                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
    }
    names
}

fn text_of<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source
        .get(node.start_byte()..node.end_byte())
        .unwrap_or_default()
}

fn first_string(node: Node<'_>, source: &str) -> Option<String> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "string_fragment" {
            return Some(text_of(current, source).to_owned());
        }
        let mut cursor = current.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    None
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
