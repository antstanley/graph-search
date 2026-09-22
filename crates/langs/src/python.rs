//! The Python extractor (`SPEC.md` §7.5).
//!
//! Emits classes, functions, methods, class fields and module variables, with
//! `import`/`from ... import` edges, calls, inheritance and annotation type
//! uses. Python binds names dynamically, so this adapter deliberately models
//! what the syntax states and leaves plain identifier references and local
//! bindings to the generic same-file/qualified/unique-name resolution rules
//! (`SPEC.md` §7.4). Every construct consumes its own subtree, so nothing is
//! visited twice.

use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use graph_search_types::node::Span;
use std::path::Path;
use tree_sitter::Node;

/// The Python extractor (`.py`, `.pyi`, `.pyw`).
#[derive(Debug, Default)]
pub struct PythonExtractor;

impl LanguageExtractor for PythonExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::Python
    }

    fn supports(&self, path: &Path) -> bool {
        matches!(
            path.extension().and_then(std::ffi::OsStr::to_str),
            Some("py" | "pyi" | "pyw")
        )
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("python grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the python parser produced no tree"));
        };
        let mut extractor = Extractor {
            source: file.text,
            extraction: Extraction::default(),
            scope: Vec::new(),
        };
        extractor.walk_node(tree.root_node());
        Ok(extractor.extraction)
    }
}

/// What a lexical scope holds, so a `def` can tell a method from a function and
/// an assignment can tell a field from a module variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    /// The file body.
    Module,
    /// A class body.
    Class,
    /// A function or method body.
    Function,
}

/// One enclosing item on the lexical stack.
struct Scope {
    /// The enclosing symbol's fact key.
    key: String,
    /// The enclosing symbol's dotted qualified name.
    qualified: String,
    /// What the scope body holds.
    context: Context,
}

struct Extractor<'a> {
    source: &'a str,
    extraction: Extraction,
    scope: Vec<Scope>,
}

impl Extractor<'_> {
    // ------------------------------------------------------------------
    // Text and naming helpers
    // ------------------------------------------------------------------

    fn text(&self, node: Node<'_>) -> &str {
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
            .map_or_else(String::new, |scope| scope.qualified.clone());
        if !qualified.is_empty() {
            qualified.push('.');
        }
        qualified.push_str(name);
        let mut key = format!("{}:{qualified}", kind.as_str());
        for scope in self.scope.iter().rev() {
            key = format!("{}>{}", scope.key, key);
        }
        (key, qualified)
    }

    /// Pushes a symbol and its scope; the caller pops after walking children.
    fn emit(&mut self, node: Node<'_>, kind: NodeKind, name: String, signature: String) {
        let (key, qualified) = self.qualify(&name, kind);
        let parent = self.scope.last().map(|scope| scope.key.clone());
        let mut fact = SymbolFact::new(key.clone(), kind, name, qualified.clone(), Self::span(node))
            .with_signature(signature);
        if let Some(parent) = parent {
            fact = fact.with_parent(parent);
        }
        self.extraction.symbols.push(fact);
        let context = match kind {
            NodeKind::Class => Context::Class,
            NodeKind::Function | NodeKind::Method => Context::Function,
            _ => Context::Module,
        };
        self.scope.push(Scope {
            key,
            qualified,
            context,
        });
    }

    fn current_context(&self) -> Context {
        self.scope.last().map_or(Context::Module, |scope| scope.context)
    }

    /// A reference owned by the enclosing symbol, or the file when top level.
    fn reference(&mut self, kind: EdgeKind, name: String, node: Node<'_>) {
        let owner = self.scope.last().map(|scope| scope.key.clone());
        let fact = match owner {
            Some(key) => ReferenceFact::from_symbol(key, kind, name, Self::line(node)),
            None => ReferenceFact::file_level(kind, name, Self::line(node)),
        };
        self.extraction.references.push(fact.at(Self::span(node)));
    }

    /// A call, owned by the innermost function scope and preserving the raw
    /// callee spelling for lexical analysis.
    fn reference_call(&mut self, name: &str, raw: &str, node: Node<'_>) {
        let owner = self
            .scope
            .iter()
            .rev()
            .find(|scope| scope.context == Context::Function);
        let mut fact = match owner {
            Some(scope) => ReferenceFact::from_symbol(
                scope.key.clone(),
                EdgeKind::Calls,
                name.to_owned(),
                Self::line(node),
            ),
            None => ReferenceFact::file_level(EdgeKind::Calls, name.to_owned(), Self::line(node)),
        };
        fact.raw_name = Some(raw.to_owned());
        self.extraction.references.push(fact.at(Self::span(node)));
    }

    fn is_async(&self, node: Node<'_>) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|child| child.kind() == "async")
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

    // ------------------------------------------------------------------
    // The dispatch
    // ------------------------------------------------------------------

    fn walk_node(&mut self, node: Node<'_>) {
        match node.kind() {
            "class_definition" => self.class(node),
            "function_definition" => self.function(node),
            "decorated_definition" => self.decorated(node),
            "type_alias_statement" => self.type_alias(node),
            "import_statement" => self.import(node),
            "import_from_statement" => self.import_from(node),
            "call" => self.call(node),
            "assignment" => self.assignment(node),
            _ => self.walk_children(node),
        }
    }

    fn class(&mut self, node: Node<'_>) {
        let name = node.child_by_field_name("name").map_or_else(
            || String::from("(anonymous)"),
            |name| self.text(name).to_owned(),
        );
        self.emit(node, NodeKind::Class, name, self.first_line(node));
        if let Some(superclasses) = node.child_by_field_name("superclasses") {
            self.heritage(superclasses);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    /// `class C(Base, other.Mod)`: each named base is an `extends` edge. A
    /// keyword argument (`metaclass=...`) is not a base.
    fn heritage(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let bases: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| matches!(child.kind(), "identifier" | "attribute" | "dotted_name"))
            .collect();
        for base in bases {
            let name = self.text(base).to_owned();
            if !name.is_empty() {
                self.reference(EdgeKind::Extends, name, base);
            }
        }
    }

    fn function(&mut self, node: Node<'_>) {
        let kind = if self.current_context() == Context::Class {
            NodeKind::Method
        } else {
            NodeKind::Function
        };
        let Some(name_node) = node.child_by_field_name("name") else {
            self.walk_children(node);
            return;
        };
        let name = self.text(name_node).to_owned();
        let is_async = self.is_async(node);
        self.emit(node, kind, name, self.first_line(node));
        if is_async
            && let Some(symbol) = self.extraction.symbols.last_mut()
        {
            symbol.is_async = true;
        }
        self.annotations(node);
        if let Some(parameters) = node.child_by_field_name("parameters") {
            self.walk_children(parameters);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    /// Parameter and return annotations become `type_uses` edges.
    fn annotations(&mut self, node: Node<'_>) {
        if let Some(parameters) = node.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            let typed: Vec<Node<'_>> = parameters.named_children(&mut cursor).collect();
            for parameter in typed {
                if let Some(annotation) = parameter.child_by_field_name("type") {
                    self.type_uses(annotation);
                }
            }
        }
        if let Some(return_type) = node.child_by_field_name("return_type") {
            self.type_uses(return_type);
        }
    }

    fn decorated(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let decorators: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "decorator")
            .collect();
        for decorator in decorators {
            self.walk_children(decorator);
        }
        if let Some(definition) = node.child_by_field_name("definition") {
            self.walk_node(definition);
        }
    }

    fn type_alias(&mut self, node: Node<'_>) {
        let Some(left) = node.child_by_field_name("left") else {
            self.walk_children(node);
            return;
        };
        let name = self.text(left).to_owned();
        if name.is_empty() {
            self.walk_children(node);
            return;
        }
        self.emit(node, NodeKind::TypeAlias, name, self.first_line(node));
        if let Some(right) = node.child_by_field_name("right") {
            self.type_uses(right);
        }
        self.scope.pop();
    }

    /// `import a.b`, `import a.b as c`, `import a, b`.
    fn import(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let names: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
            .collect();
        for child in names {
            let target = if child.kind() == "aliased_import" {
                child.child_by_field_name("name")
            } else {
                Some(child)
            };
            let Some(target) = target else { continue };
            let name = self.text(target).to_owned();
            if !name.is_empty() {
                self.reference(EdgeKind::Imports, name, child);
            }
        }
    }

    /// `from m import a`, `from .m import b as c`, `from m import *`.
    fn import_from(&mut self, node: Node<'_>) {
        let module = node.child_by_field_name("module_name");
        let Some(module) = module else {
            self.walk_children(node);
            return;
        };
        let specifier = self.text(module).to_owned();
        if specifier.is_empty() {
            self.walk_children(node);
            return;
        }
        // The module edge itself: file -> file.
        self.reference(EdgeKind::Imports, specifier.clone(), node);
        let mut cursor = node.walk();
        let imported: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| child.id() != module.id())
            .filter(|child| matches!(child.kind(), "dotted_name" | "aliased_import"))
            .collect();
        for child in imported {
            let target = if child.kind() == "aliased_import" {
                child.child_by_field_name("name")
            } else {
                Some(child)
            };
            let Some(target) = target else { continue };
            let name = self.text(target).to_owned();
            if !name.is_empty() {
                self.imported(&name, &specifier, child);
            }
        }
    }

    /// One explicit import binding, resolved through its module specifier
    /// (`SPEC.md` §7.4 rule 2).
    fn imported(&mut self, name: &str, specifier: &str, node: Node<'_>) {
        let owner = self.scope.last().map(|scope| scope.key.clone());
        let fact = match owner {
            Some(key) => ReferenceFact::from_symbol(key, EdgeKind::Imports, name, Self::line(node)),
            None => ReferenceFact::file_level(EdgeKind::Imports, name, Self::line(node)),
        };
        self.extraction
            .references
            .push(fact.at(Self::span(node)).via_import(specifier));
    }

    fn call(&mut self, node: Node<'_>) {
        let function = node.child_by_field_name("function");
        if let Some(function) = function {
            let callee = self.text(function).trim().to_owned();
            if !callee.is_empty() {
                match self.self_member_target(&callee) {
                    Some(target) => self.reference_call(&target, &callee, node),
                    None => self.reference_call(&callee, &callee, node),
                }
            }
        }
        // Walk the receiver (an `attribute`'s object) so nested calls are seen;
        // the member identifier itself is not a use.
        match function {
            Some(function) if function.kind() == "attribute" => {
                if let Some(object) = function.child_by_field_name("object") {
                    self.walk_node(object);
                }
            }
            Some(function) => self.walk_node(function),
            None => {}
        }
        if let Some(arguments) = node.child_by_field_name("arguments") {
            self.walk_children(arguments);
        }
    }

    /// `self.method()` written directly in a class method resolves to the
    /// enclosing class's qualified member, as Rust rewrites `self.method()`.
    fn self_member_target(&self, callee: &str) -> Option<String> {
        let member = callee.strip_prefix("self.")?;
        if member.is_empty()
            || !member
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_')
        {
            return None;
        }
        let index = self
            .scope
            .iter()
            .rposition(|scope| scope.context == Context::Function)?;
        let class = self.scope.get(index.checked_sub(1)?)?;
        if class.context != Context::Class {
            return None;
        }
        Some(format!("{}.{}", class.qualified, member))
    }

    fn assignment(&mut self, node: Node<'_>) {
        // Only module- and class-body assignments are declarations; a local
        // assignment is not a symbol this adapter models.
        if self
            .scope
            .last()
            .is_none_or(|scope| scope.context != Context::Function)
            && let Some(left) = node.child_by_field_name("left")
            && left.kind() == "identifier"
        {
            let name = self.text(left).to_owned();
            if !name.is_empty() {
                let kind = if self.current_context() == Context::Class {
                    NodeKind::Field
                } else {
                    NodeKind::Variable
                };
                self.emit(node, kind, name, self.first_line(node));
                self.scope.pop();
            }
        }
        if let Some(annotation) = node.child_by_field_name("type") {
            self.type_uses(annotation);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.walk_node(right);
        }
    }

    fn type_uses(&mut self, node: Node<'_>) {
        for name in self.type_names_in(node) {
            self.reference(EdgeKind::TypeUses, name, node);
        }
    }

    /// The named types a node (or its subtree, for an annotation) refers to.
    fn type_names_in(&self, node: Node<'_>) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            match current.kind() {
                "identifier" => {
                    let name = self.text(current);
                    if !is_builtin_type(name) {
                        names.push(name.to_owned());
                    }
                }
                "attribute" | "dotted_name" => {
                    let name = self.text(current);
                    if !name.is_empty() {
                        names.push(name.to_owned());
                    }
                }
                _ => {
                    let mut cursor = current.walk();
                    let mut children: Vec<Node<'_>> = current.named_children(&mut cursor).collect();
                    children.reverse();
                    stack.extend(children);
                }
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

/// Python builtin names that are not workspace symbols; emitting them would
/// only add dangling `type_uses` noise.
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "str"
            | "float"
            | "bool"
            | "bytes"
            | "bytearray"
            | "complex"
            | "object"
            | "list"
            | "dict"
            | "set"
            | "frozenset"
            | "tuple"
            | "type"
            | "range"
            | "self"
            | "cls"
    )
}
