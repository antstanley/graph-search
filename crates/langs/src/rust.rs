//! The Rust extractor (`SPEC.md` §7.1).
//!
//! Emits functions (free and associated), methods, structs, enums, traits,
//! impls, type aliases, consts, statics, macros, fields, variants, and
//! modules, with `use`/`mod` imports, calls, type uses, implements, and
//! remaining path references. Every construct consumes its own subtree, so
//! nothing is visited twice.

use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::kind::{EdgeKind, NodeKind, Visibility};
use graph_search_types::node::Span;
use std::path::Path;
use tree_sitter::Node;

/// The Rust extractor.
#[derive(Debug, Default)]
pub struct RustExtractor;

impl LanguageExtractor for RustExtractor {
    fn language(&self) -> graph_search_types::Language {
        graph_search_types::Language::Rust
    }

    fn supports(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "rs")
    }

    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|error| ParseError::new(format!("rust grammar: {error}")))?;
        let Some(tree) = parser.parse(file.text, None) else {
            return Err(ParseError::new("the rust parser produced no tree"));
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

/// One enclosing item on the lexical stack: its fact key and qualified name.
struct Scope {
    key: String,
    qualified: String,
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
        for scope in &self.scope {
            if !qualified.is_empty() {
                qualified.push_str("::");
            }
            qualified.push_str(&scope.qualified);
        }
        if !qualified.is_empty() {
            qualified.push_str("::");
        }
        qualified.push_str(name);
        let mut key = format!("{}:{qualified}", kind.as_str());
        for scope in self.scope.iter().rev() {
            key = format!("{}>{}", scope.key, key);
        }
        (key, qualified)
    }

    fn visibility(&self, node: Node<'_>) -> Option<Visibility> {
        let child = node.child_by_field_name("visibility")?;
        match self.text(child) {
            text if text.starts_with("pub(crate)") => Some(Visibility::Crate),
            text if text.starts_with("pub(super)") => Some(Visibility::Super),
            text if text.starts_with("pub") => Some(Visibility::Public),
            _ => Some(Visibility::Private),
        }
    }

    fn is_async(&self, node: Node<'_>) -> bool {
        (0..node.child_count()).any(|i| {
            node.child(i)
                .is_some_and(|child| self.text(child) == "async")
        })
    }

    fn reference_from(&self, kind: EdgeKind, name: String, node: Node<'_>) -> ReferenceFact {
        match self.scope.last().map(|scope| scope.key.clone()) {
            Some(key) => ReferenceFact::from_symbol(key, kind, name, Self::line(node)),
            None => ReferenceFact::file_level(kind, name, Self::line(node)),
        }
    }

    // ------------------------------------------------------------------
    // The walk: every arm consumes its subtree
    // ------------------------------------------------------------------

    fn walk_node(&mut self, node: Node<'_>) {
        match node.kind() {
            "function_item" | "function_signature_item" => self.function(node),
            "struct_item" => self.item(node, NodeKind::Struct, "struct"),
            "enum_item" => self.item(node, NodeKind::Enum, "enum"),
            "trait_item" => self.item(node, NodeKind::Trait, "trait"),
            "type_item" => self.item(node, NodeKind::TypeAlias, "type"),
            "const_item" => self.item(node, NodeKind::Const, "const"),
            "static_item" => self.item(node, NodeKind::Static, "static"),
            "macro_definition" => self.item(node, NodeKind::Macro, "macro_rules!"),
            "impl_item" => self.impl_block(node),
            "mod_item" => self.module(node),
            "field_declaration" => self.field(node),
            "enum_variant" => self.variant(node),
            "use_declaration" => self.use_declaration(node),
            "call_expression" => self.call(node),
            "generic_type"
            | "scoped_type_identifier"
            | "reference_type"
            | "pointer_type"
            | "array_type"
            | "tuple_type"
            | "function_type"
            | "qualified_type"
            | "abstract_type"
            | "dynamic_type"
            | "macro_type" => self.type_use(node),
            "token_tree" | "token_repetition" => {} // macro bodies: not expanded
            _ => self.walk_children(node),
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

    // ------------------------------------------------------------------
    // Constructs
    // ------------------------------------------------------------------

    fn emit(&mut self, node: Node<'_>, kind: NodeKind, name: String, signature: String) {
        let (key, qualified) = self.qualify(&name, kind);
        let parent = self.scope.last().map(|scope| scope.key.clone());
        let mut fact =
            SymbolFact::new(key, kind, name, qualified, Self::span(node)).with_signature(signature);
        fact = fact.with_visibility(self.visibility(node), self.is_async(node));
        if let Some(parent) = parent {
            fact = fact.with_parent(parent);
        }
        self.extraction.symbols.push(fact);
    }

    /// `[visibility] kind Name <...> { ... }` — the shared item shape.
    fn item(&mut self, node: Node<'_>, kind: NodeKind, keyword: &str) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name_node).to_owned();
        self.emit(node, kind, name.clone(), format!("{keyword} {name}"));
        let (key, qualified) = self.qualify(&name, kind);
        self.scope.push(Scope {
            key: key.clone(),
            qualified,
        });
        // Fields, variants, and (for traits) members.
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    fn function(&mut self, node: Node<'_>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name_node).to_owned();
        let inside_impl = self
            .scope
            .iter()
            .any(|scope| scope.key.starts_with("impl:"));
        let kind = if inside_impl {
            NodeKind::Method
        } else {
            NodeKind::Function
        };
        let signature = self.first_line(node);
        self.emit(node, kind, name.clone(), signature);
        let (key, qualified) = self.qualify(&name, kind);
        self.scope.push(Scope {
            key: key.clone(),
            qualified,
        });
        // Parameters, return type, and the body — types become type uses,
        // calls become call references, all attached to this item.
        self.walk_children(node);
        self.scope.pop();
    }

    fn impl_block(&mut self, node: Node<'_>) {
        let type_node = node.child_by_field_name("type");
        let trait_node = node.child_by_field_name("trait");
        let type_text =
            type_node.map_or_else(|| String::from("<unknown>"), |t| self.text(t).to_owned());
        let qualified = match trait_node {
            Some(trait_node) => {
                let trait_text = self.text(trait_node).to_owned();
                // impl Trait for Type: the implements edge to the trait.
                self.extraction.references.push(self.reference_from(
                    EdgeKind::Implements,
                    trait_text.clone(),
                    node,
                ));
                format!("impl {trait_text} for {type_text}")
            }
            None => format!("impl {type_text}"),
        };
        self.emit(
            node,
            NodeKind::Impl,
            qualified.clone(),
            self.first_line(node),
        );
        let (key, _) = self.qualify(&qualified, NodeKind::Impl);
        // Methods under the impl take the TYPE's path (`Type::method`), which
        // is how callers spell them (`SPEC.md` §10.3).
        self.scope.push(Scope {
            key: key.clone(),
            qualified: type_text,
        });
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    fn module(&mut self, node: Node<'_>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name_node).to_owned();
        self.emit(node, NodeKind::Module, name.clone(), format!("mod {name}"));
        // `mod helper;` names a file: an import edge (`SPEC.md` §7.1).
        if node.child_by_field_name("body").is_none() {
            self.extraction.references.push(ReferenceFact::file_level(
                EdgeKind::Imports,
                name.clone(),
                Self::line(node),
            ));
        }
        let (key, qualified) = self.qualify(&name, NodeKind::Module);
        self.scope.push(Scope {
            key: key.clone(),
            qualified,
        });
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    fn field(&mut self, node: Node<'_>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name_node).to_owned();
        self.emit(node, NodeKind::Field, name, self.first_line(node));
        if let Some(type_node) = node.child_by_field_name("type") {
            for name in type_names(type_node, self.source) {
                self.extraction.references.push(self.reference_from(
                    EdgeKind::TypeUses,
                    name,
                    node,
                ));
            }
        }
    }

    fn variant(&mut self, node: Node<'_>) {
        let Some(name_node) = node.child_by_field_name("name") else {
            return;
        };
        let name = self.text(name_node).to_owned();
        self.emit(node, NodeKind::Variant, name, self.first_line(node));
    }

    fn use_declaration(&mut self, node: Node<'_>) {
        let Some(arg) = node.child_by_field_name("argument") else {
            return;
        };
        // `use a::b as c;` and `use a::{b, c};` — one fact per emitted path.
        for spec in use_paths(self.text(arg)) {
            self.extraction.references.push(ReferenceFact::file_level(
                EdgeKind::Imports,
                spec,
                Self::line(node),
            ));
        }
    }

    fn call(&mut self, node: Node<'_>) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        let callee = self.text(function).trim().to_owned();
        if callee.is_empty() || callee.chars().next().is_some_and(char::is_numeric) {
            // `0()`, string literals: not name references.
            return;
        }
        self.extraction
            .references
            .push(self.reference_from(EdgeKind::Calls, callee, node));
        // Arguments can call too: walk the argument list.
        if let Some(args) = node.child_by_field_name("arguments") {
            self.walk_children(args);
        }
    }

    fn type_use(&mut self, node: Node<'_>) {
        for name in type_names(node, self.source) {
            self.extraction
                .references
                .push(self.reference_from(EdgeKind::TypeUses, name, node));
        }
    }
}

/// The concrete type names a type node refers to: `Vec<u8>` yields `Vec`,
/// `std::io::Result<T>` yields `std::io::Result`.
fn type_names(node: Node<'_>, source: &str) -> Vec<String> {
    fn text_of<'a>(node: Node<'_>, source: &'a str) -> &'a str {
        let start = node.start_byte().min(source.len());
        let end = node.end_byte().min(source.len());
        source.get(start..end).unwrap_or_default()
    }

    let mut names = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        match current.kind() {
            "type_identifier" => names.push(text_of(current, source).to_owned()),
            "generic_type" => {
                if let Some(base) = current.child_by_field_name("type") {
                    names.push(text_of(base, source).to_owned());
                }
                if let Some(args) = current.child_by_field_name("arguments") {
                    stack.push(args);
                }
            }
            "scoped_type_identifier" => {
                names.push(text_of(current, source).to_owned());
                if let Some(args) = current.child_by_field_name("arguments") {
                    stack.push(args);
                }
            }
            "type_arguments" => {
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
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    names
}

/// `a::b as c` -> `a::b`; `a::{b, c}` -> one entry per path.
fn use_paths(argument: &str) -> Vec<String> {
    let mut paths = Vec::new();
    expand_use(argument.trim(), String::new(), &mut paths);
    paths
}

fn expand_use(text: &str, prefix: String, out: &mut Vec<String>) {
    let text = text.trim();
    if let Some(open) = text.find('{') {
        let head = text[..open].trim().trim_end_matches("::");
        let close = text.rfind('}').unwrap_or(text.len());
        let inner = &text[open.saturating_add(1)..close];
        let base = if head.is_empty() {
            prefix
        } else {
            join_use(&prefix, head)
        };
        for piece in inner.split(',') {
            expand_use(piece, base.clone(), out);
        }
        return;
    }
    let path = text.split(" as ").next().unwrap_or(text).trim();
    if path.is_empty() || path == "self" {
        return;
    }
    out.push(join_use(&prefix, path));
}

fn join_use(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}::{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &'static str) -> Extraction {
        let file = SourceFile {
            path: Path::new("src/t.rs"),
            text,
        };
        RustExtractor
            .extract(&file)
            .unwrap_or_else(|e| panic!("extract: {e}"))
    }

    #[test]
    fn a_free_function_and_a_call_are_extracted() {
        let extraction = extract(
            "fn main() { helper(); }
fn helper() {}
",
        );
        let names: Vec<&str> = extraction
            .symbols
            .iter()
            .map(|fact| fact.name.as_str())
            .collect();
        assert_eq!(names, vec!["main", "helper"], "{names:?}");
        let calls: Vec<&str> = extraction
            .references
            .iter()
            .filter(|fact| fact.kind == EdgeKind::Calls)
            .map(|fact| fact.name.as_str())
            .collect();
        assert_eq!(calls, vec!["helper"], "{calls:?}");
        // The call belongs to `main`, not the file.
        assert!(
            extraction
                .references
                .iter()
                .all(|fact| fact.from_key.is_some())
        );
    }

    #[test]
    fn an_impl_makes_methods_and_the_implements_edge() {
        let text = "struct Parser { source: String }
impl Read for Parser { fn parse(&self) {} }
";
        let extraction = extract(text);
        let kinds: Vec<NodeKind> = extraction.symbols.iter().map(|fact| fact.kind).collect();
        assert!(kinds.contains(&NodeKind::Struct), "{kinds:?}");
        assert!(kinds.contains(&NodeKind::Impl), "{kinds:?}");
        assert!(kinds.contains(&NodeKind::Method), "{kinds:?}");
        assert!(kinds.contains(&NodeKind::Field), "{kinds:?}");
        let implements: Vec<&str> = extraction
            .references
            .iter()
            .filter(|fact| fact.kind == EdgeKind::Implements)
            .map(|fact| fact.name.as_str())
            .collect();
        assert_eq!(implements, vec!["Read"], "{implements:?}");
    }

    #[test]
    fn use_statements_expand_groups_and_aliases() {
        let extraction = extract(
            "use std::{io, fs};
use a::b as c;
",
        );
        let imports: Vec<&str> = extraction
            .references
            .iter()
            .map(|fact| fact.name.as_str())
            .collect();
        assert_eq!(imports, vec!["std::io", "std::fs", "a::b"], "{imports:?}");
    }

    #[test]
    fn a_struct_field_type_becomes_a_type_use() {
        let extraction = extract(
            "struct S { field: Vec<u8> }
",
        );
        let type_uses: Vec<&str> = extraction
            .references
            .iter()
            .filter(|fact| fact.kind == EdgeKind::TypeUses)
            .map(|fact| fact.name.as_str())
            .collect();
        assert!(type_uses.contains(&"Vec"), "{type_uses:?}");
    }

    #[test]
    fn duplicate_impl_names_get_disambiguated_by_core() {
        let text = "impl A { fn go(&self) {} }
impl B { fn go(&self) {} }
";
        let extraction = extract(text);
        let impls: Vec<&str> = extraction
            .symbols
            .iter()
            .filter(|fact| fact.kind == NodeKind::Impl)
            .map(|fact| fact.qualified_name.as_str())
            .collect();
        assert_eq!(impls, vec!["impl A", "impl B"], "{impls:?}");
        let methods: Vec<&str> = extraction
            .symbols
            .iter()
            .filter(|fact| fact.kind == NodeKind::Method)
            .map(|fact| fact.qualified_name.as_str())
            .collect();
        // Methods take the type's path: `A::go`, how callers spell them.
        assert_eq!(methods, vec!["A::go", "B::go"], "{methods:?}");
    }
}
