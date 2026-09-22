//! The Rust extractor (`SPEC.md` §7.1).
//!
//! Emits functions (free and associated), methods, structs, enums, traits,
//! impls, type aliases, consts, statics, macros, fields, variants, and
//! modules, with `use`/`mod` imports, calls, type uses, implements, and
//! remaining path references. Every construct consumes its own subtree, so
//! nothing is visited twice.

use crate::rust_receivers::{self, Local};
use graph_search_core::extraction::{Extraction, ReferenceFact, SymbolFact};
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::extraction::ReceiverType;
use graph_search_types::kind::{EdgeKind, NodeKind, Visibility};
use graph_search_types::node::Span;
use std::collections::HashMap;
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
            type_facts: HashMap::new(),
            call_facts: HashMap::new(),
            locals: Vec::new(),
            declared_types: Vec::new(),
        };
        extractor.walk_node(tree.root_node());
        let (root_path, root_unsupported) = module_path(tree.root_node(), file.text);
        if root_path.is_some() || root_unsupported {
            for symbol in &mut extractor.extraction.symbols {
                if symbol.kind == NodeKind::Module {
                    symbol.attributes.insert(
                        "rust_module_unavailable".into(),
                        "file_inner_attribute_unsupported".into(),
                    );
                }
            }
        }
        crate::scopes::enrich(tree.root_node(), file.text, &mut extractor.extraction);
        extractor.attach_declared_types();
        crate::doc_comments::enrich(tree.root_node(), file.text, true, &mut extractor.extraction);
        Ok(extractor.extraction)
    }
}

/// One enclosing item on the lexical stack: its fact key and qualified name.
struct Scope {
    key: String,
    qualified: String,
    /// Generic type parameters this item declares (`<T, U>`). Their bare uses in
    /// child positions are lexical bindings, not references to a workspace type,
    /// so they must not become `type_uses` edges.
    type_params: Vec<String>,
}

struct Extractor<'a> {
    source: &'a str,
    extraction: Extraction,
    scope: Vec<Scope>,
    /// `type_uses` ordinals by outermost type node id: `(name, ordinal)`.
    type_facts: HashMap<usize, Vec<(String, usize)>>,
    /// Call ordinals by call node id.
    call_facts: HashMap<usize, usize>,
    /// Local bindings of the function body being walked.
    locals: Vec<Local>,
    /// Declared field and return types to record once imports are expanded:
    /// `(symbol ordinal, attribute, type)`.
    declared_types: Vec<(usize, &'static str, DeclaredType)>,
}

/// A declared type recorded on a symbol for receiver typing.
#[derive(Clone, Copy)]
enum DeclaredType {
    /// `Self`: the enclosing impl's type.
    SelfType,
    /// The type a `type_uses` reference (by ordinal) names.
    Fact(usize),
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
        let parent = self
            .scope
            .last()
            .map(|scope| (scope.key.as_str(), scope.qualified.as_str()));
        crate::walk::qualify(parent, name, kind, "::")
    }

    fn visibility(&self, node: Node<'_>) -> Option<Visibility> {
        let child = visibility_modifier(node)?;
        if child.has_error() {
            return None;
        }
        let mut cursor = child.walk();
        let path = child
            .named_children(&mut cursor)
            .find(|n| !matches!(n.kind(), "line_comment" | "block_comment"));
        match path.map(|n| n.kind()) {
            Some("crate") => Some(Visibility::Crate),
            Some("super") => Some(Visibility::Super),
            Some("self") => Some(Visibility::Private),
            None if self.text(child).trim() == "pub" => Some(Visibility::Public),
            // Arbitrary restricted paths need module identity, not a public guess.
            _ => None,
        }
    }

    fn is_async(&self, node: Node<'_>) -> bool {
        (0..node.child_count()).any(|i| {
            node.child(i)
                .is_some_and(|child| self.text(child) == "async")
        })
    }

    fn reference_from(&self, kind: EdgeKind, name: String, node: Node<'_>) -> ReferenceFact {
        let fact = match self.scope.last().map(|scope| scope.key.clone()) {
            Some(key) => ReferenceFact::from_symbol(key, kind, name, Self::line(node)),
            None => ReferenceFact::file_level(kind, name, Self::line(node)),
        };
        fact.at(Self::span(node))
    }

    // ------------------------------------------------------------------
    // Receiver typing (`rust_receivers`)
    // ------------------------------------------------------------------

    /// The `type_uses` ordinal naming the core of `ty` (peeled), where `outer`
    /// is the type node the references were recorded for.
    fn type_fact(&self, outer: Node<'_>, ty: Node<'_>) -> Option<usize> {
        self.receiver_context().type_fact(outer, ty)
    }

    /// The declared type of `ty` (as written under `outer`), and of its
    /// `Option`/`Result` success type.
    fn declared(&self, outer: Node<'_>, ty: Node<'_>) -> [Option<DeclaredType>; 2] {
        let declared = |core: Node<'_>| {
            if self.text(core) == "Self" {
                Some(DeclaredType::SelfType)
            } else {
                self.type_fact(outer, core).map(DeclaredType::Fact)
            }
        };
        [
            declared(rust_receivers::peel(ty, self.source)),
            rust_receivers::fallible(ty, self.source).and_then(declared),
        ]
    }

    /// Records the declared type of the symbol at `symbol`.
    fn declare(
        &mut self,
        symbol: usize,
        attribute: [&'static str; 2],
        outer: Node<'_>,
        ty: Node<'_>,
    ) {
        for (attribute, declared) in attribute.into_iter().zip(self.declared(outer, ty)) {
            if let Some(declared) = declared {
                self.declared_types.push((symbol, attribute, declared));
            }
        }
    }

    /// Records declared types as attributes once scope analysis has expanded
    /// each type reference through the file's imports: `key:` a same-file
    /// declaration, `path:` a path resolvable from this file, `self` the impl
    /// type.
    fn attach_declared_types(&mut self) {
        for (symbol, attribute, declared) in std::mem::take(&mut self.declared_types) {
            let value = match declared {
                DeclaredType::SelfType => Some("self".to_owned()),
                DeclaredType::Fact(ordinal) => self
                    .extraction
                    .references
                    .get(ordinal)
                    .and_then(|fact| type_binding(&self.extraction, fact)),
            };
            if let (Some(value), Some(symbol)) = (value, self.extraction.symbols.get_mut(symbol)) {
                symbol.attributes.insert(attribute.into(), value);
            }
        }
        // A receiver's annotation names its type through the same bindings.
        let declared: Vec<Option<String>> = self
            .extraction
            .references
            .iter()
            .map(|fact| type_binding(&self.extraction, fact))
            .collect();
        for fact in &mut self.extraction.references {
            if let Some(receiver) = &mut fact.receiver {
                rebind_annotations(receiver, &declared);
            }
        }
    }

    /// The type methods on `self` are qualified by: the enclosing `impl` (or
    /// trait) path.
    fn self_type(&self) -> Option<&str> {
        self.scope
            .iter()
            .rev()
            .find(|scope| {
                let last = scope.key.rsplit('>').next().unwrap_or(&scope.key);
                last.starts_with("impl:") || last.starts_with("trait:")
            })
            .map(|scope| scope.qualified.as_str())
    }

    fn receiver(&self, node: Node<'_>) -> Option<ReceiverType> {
        self.receiver_context().receiver(node)
    }

    fn receiver_context(&self) -> rust_receivers::Context<'_> {
        rust_receivers::Context {
            source: self.source,
            locals: &self.locals,
            self_type: self.self_type(),
            type_facts: &self.type_facts,
            call_facts: &self.call_facts,
        }
    }

    /// Untyped bindings: each identifier a pattern binds shadows any outer
    /// local of that name from `from` to `until`.
    fn shadow(&mut self, pattern: Node<'_>, from: usize, until: usize) {
        for identifier in rust_receivers::pattern_identifiers(pattern) {
            let name = self.text(identifier).to_owned();
            self.locals.push(Local {
                name,
                from,
                until,
                plain: None,
                fallible: None,
            });
        }
    }

    /// `let x: T = …` / `let x = expr;`: `x` is visible from the end of the
    /// statement to the end of its block.
    fn let_declaration(&mut self, node: Node<'_>) {
        self.walk_children(node);
        let Some(pattern) = node.child_by_field_name("pattern") else {
            return;
        };
        let from = node.end_byte();
        let until = node.parent().map_or(from, |parent| parent.end_byte());
        if pattern.kind() != "identifier" {
            self.shadow(pattern, from, until);
            return;
        }
        let (plain, fallible) = if let Some(ty) = node.child_by_field_name("type") {
            let [plain, fallible] = self.declared(ty, ty);
            let typed = |declared: Option<DeclaredType>| match declared? {
                DeclaredType::SelfType => {
                    self.self_type().map(|ty| ReceiverType::SelfType(ty.into()))
                }
                DeclaredType::Fact(ordinal) => Some(ReceiverType::Annotation(ordinal)),
            };
            (typed(plain), typed(fallible))
        } else if let Some(value) = node.child_by_field_name("value") {
            let context = self.receiver_context();
            (context.receiver(value), context.fallible(value))
        } else {
            (None, None)
        };
        self.locals.push(Local {
            name: self.text(pattern).to_owned(),
            from,
            until,
            plain,
            fallible,
        });
    }

    /// Typed parameters are locals of the function body.
    fn parameters(&mut self, node: Node<'_>, body: Node<'_>) {
        let Some(parameters) = node.child_by_field_name("parameters") else {
            return;
        };
        let (from, until) = (body.start_byte(), body.end_byte());
        let mut cursor = parameters.walk();
        let parameters: Vec<Node<'_>> = parameters
            .named_children(&mut cursor)
            .filter(|parameter| parameter.kind() == "parameter")
            .collect();
        for parameter in parameters {
            let (Some(pattern), Some(ty)) = (
                parameter.child_by_field_name("pattern"),
                parameter.child_by_field_name("type"),
            ) else {
                continue;
            };
            if pattern.kind() != "identifier" {
                self.shadow(pattern, from, until);
                continue;
            }
            let [plain, fallible] = self.declared(ty, ty);
            let typed = |declared: Option<DeclaredType>| match declared? {
                DeclaredType::SelfType => {
                    self.self_type().map(|ty| ReceiverType::SelfType(ty.into()))
                }
                DeclaredType::Fact(ordinal) => Some(ReceiverType::Annotation(ordinal)),
            };
            let local = Local {
                name: self.text(pattern).to_owned(),
                from,
                until,
                plain: typed(plain),
                fallible: typed(fallible),
            };
            self.locals.push(local);
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
            "let_declaration" => self.let_declaration(node),
            "closure_expression" => {
                if let Some(parameters) = node.child_by_field_name("parameters") {
                    self.shadow(parameters, node.start_byte(), node.end_byte());
                }
                self.walk_children(node);
            }
            "match_arm" | "for_expression" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    self.shadow(pattern, node.start_byte(), node.end_byte());
                }
                self.walk_children(node);
            }
            "let_condition" => {
                if let Some(pattern) = node.child_by_field_name("pattern") {
                    let until = node
                        .parent()
                        .map_or(node.end_byte(), |parent| parent.end_byte());
                    self.shadow(pattern, node.start_byte(), until);
                }
                self.walk_children(node);
            }
            // A bare type in position (`t: Thing`, `-> Thing`, `let x: Thing`)
            // is a use in its own right; the composite type kinds below consume
            // their subtrees, so this arm is the only one that sees a lone
            // `type_identifier` (finding E5).
            "type_identifier"
            | "generic_type"
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
        if let Some(modifier) = visibility_modifier(node) {
            fact.attributes.insert(
                "rust_visibility_modifier".into(),
                self.text(modifier).into(),
            );
        }
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
            type_params: type_parameter_names(node, self.source),
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
        let symbol = self.extraction.symbols.len().checked_sub(1);
        let (key, qualified) = self.qualify(&name, kind);
        self.scope.push(Scope {
            key: key.clone(),
            qualified,
            type_params: type_parameter_names(node, self.source),
        });
        // Parameters, return type, and the body — types become type uses,
        // calls become call references, all attached to this item. The body
        // comes last, once its parameters are known locals.
        let body = node.child_by_field_name("body");
        let mut cursor = node.walk();
        let heads: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter(|child| Some(*child) != body)
            .collect();
        for child in heads {
            self.walk_node(child);
        }
        if let (Some(return_type), Some(symbol)) = (node.child_by_field_name("return_type"), symbol)
        {
            self.declare(
                symbol,
                ["rust_returns", "rust_returns_fallible"],
                return_type,
                return_type,
            );
        }
        if let Some(body) = body {
            let outer = std::mem::take(&mut self.locals);
            self.parameters(node, body);
            self.walk_node(body);
            self.locals = outer;
        }
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
        // is how callers spell them (`SPEC.md` §10.3) — never `Type<T>::method`.
        // Dropping the impl's generic arguments also lets `self.method()` resolve
        // across impl blocks that spell those generics differently
        // (`impl<T> Foo<T>` vs `impl<A> Foo<A>`).
        self.scope.push(Scope {
            key: key.clone(),
            qualified: impl_type_path(&type_text),
            type_params: type_parameter_names(node, self.source),
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
        let (module_path, unsupported) = module_path(node, self.source);
        if let Some(fact) = self.extraction.symbols.last_mut() {
            fact.attributes.insert(
                "rust_module_form".into(),
                if node.child_by_field_name("body").is_some() {
                    "inline"
                } else {
                    "external"
                }
                .into(),
            );
            if let Some(path) = module_path {
                fact.attributes.insert("rust_module_path".into(), path);
            }
            if unsupported {
                fact.attributes.insert(
                    "rust_module_unavailable".into(),
                    "attribute_unsupported".into(),
                );
            }
        }
        // `mod helper;` names a file: preserve its exact declaration context.
        if node.child_by_field_name("body").is_none() {
            let mut reference =
                ReferenceFact::file_level(EdgeKind::Imports, name.clone(), Self::line(node))
                    .at(Self::span(node));
            reference.rust_module_declaration = true;
            self.extraction.references.push(reference);
        }
        let (key, qualified) = self.qualify(&name, NodeKind::Module);
        self.scope.push(Scope {
            key: key.clone(),
            qualified,
            type_params: type_parameter_names(node, self.source),
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
        let symbol = self.extraction.symbols.len().saturating_sub(1);
        if let Some(type_node) = node.child_by_field_name("type") {
            for name in type_names(type_node, self.source) {
                if self.is_type_binder(&name) {
                    continue;
                }
                let ordinal = self.extraction.references.len();
                self.type_facts
                    .entry(type_node.id())
                    .or_default()
                    .push((name.clone(), ordinal));
                self.extraction.references.push(self.reference_from(
                    EdgeKind::TypeUses,
                    name,
                    node,
                ));
            }
            self.declare(
                symbol,
                ["rust_type", "rust_type_fallible"],
                type_node,
                type_node,
            );
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
        let visibility = visibility_modifier(node).map(|n| self.text(n).to_owned());
        let facts = crate::rust_use::extract(arg, self.source, visibility);
        for fact in &facts {
            self.reexport(fact, node);
        }
        self.extraction.references.extend(facts);
    }

    /// A public `use` republishes its target under a module-visible name. The
    /// fact is recorded as an export node so graph readers can follow it without
    /// re-hydrating unrelated raw extraction records.
    fn reexport(&mut self, fact: &ReferenceFact, node: Node<'_>) {
        let Some(import) = &fact.rust_use else {
            return;
        };
        if import.glob || import.local_name.is_none() {
            return;
        }
        let Some(visibility) = import.visibility.as_deref() else {
            return;
        };
        if !visibility.trim_start().starts_with("pub") || visibility.contains("self") {
            return;
        }
        // An edition-2018 target (`pub use tool::X`, `pub use other_crate::Y`)
        // resolves natively like an anchored one; only a global `::` path
        // names no workspace scope.
        if fact.name.starts_with("::") {
            return;
        }
        let local = import.local_name.clone().unwrap_or_default();
        let (key, qualified) = self.qualify(&local, NodeKind::Export);
        let mut symbol = SymbolFact::new(
            key,
            NodeKind::Export,
            local,
            qualified,
            fact.span.unwrap_or_else(|| Self::span(node)),
        )
        .with_signature(self.text(node).trim().chars().take(120).collect::<String>());
        if let Some(parent) = self.scope.last().map(|scope| scope.key.clone()) {
            symbol = symbol.with_parent(parent);
        }
        symbol
            .attributes
            .insert("rust_reexport".into(), fact.name.clone());
        symbol.attributes.insert(
            "rust_visibility_modifier".into(),
            visibility.trim().to_owned(),
        );
        if import.type_only {
            symbol
                .attributes
                .insert("rust_reexport_type_only".into(), "true".into());
        }
        symbol.visibility = match visibility.trim() {
            "pub" => Some(Visibility::Public),
            "pub(crate)" => Some(Visibility::Crate),
            "pub(super)" => Some(Visibility::Super),
            _ => None,
        };
        self.extraction.symbols.push(symbol);
    }

    fn call(&mut self, node: Node<'_>) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        let mut callee = crate::rust_use::path_text(function, self.source)
            .unwrap_or_else(|| self.text(function).trim().to_owned());
        if let Some(member) = callee.strip_prefix("self.")
            && !member.contains('.')
            && let Some(scope) = self.scope.last()
            && let Some((owner, _)) = scope.qualified.rsplit_once("::")
        {
            callee = format!("{owner}::{member}");
        }
        if callee.is_empty() || callee.chars().next().is_some_and(char::is_numeric) {
            // `0()`, string literals: not name references.
            return;
        }
        // The receiver may itself be a call (factory().run()); it is walked
        // first so its facts can type this call's receiver.
        self.walk_node(function);
        let method = match function.kind() {
            "generic_function" => function.child_by_field_name("function"),
            _ => Some(function),
        }
        .filter(|method| method.kind() == "field_expression");
        let receiver = method
            .filter(|_| !callee.contains("::"))
            .and_then(|method| method.child_by_field_name("value"))
            .and_then(|value| self.receiver(value));
        let mut reference = self.reference_from(EdgeKind::Calls, callee, node);
        reference.raw_name = Some(self.text(function).trim().to_owned());
        reference.receiver = receiver;
        self.call_facts
            .insert(node.id(), self.extraction.references.len());
        self.extraction.references.push(reference);
        // Arguments can call too: walk the argument list.
        if let Some(args) = node.child_by_field_name("arguments") {
            self.walk_children(args);
        }
    }

    /// Whether `name` is `Self` or a generic type parameter of an enclosing
    /// item — a lexical type binding, not a reference to a workspace type.
    fn is_type_binder(&self, name: &str) -> bool {
        name == "Self"
            || self
                .scope
                .iter()
                .any(|scope| scope.type_params.iter().any(|param| param == name))
    }

    fn type_use(&mut self, node: Node<'_>) {
        for name in type_names(node, self.source) {
            if self.is_type_binder(&name) {
                continue;
            }
            let ordinal = self.extraction.references.len();
            self.type_facts
                .entry(node.id())
                .or_default()
                .push((name.clone(), ordinal));
            self.extraction
                .references
                .push(self.reference_from(EdgeKind::TypeUses, name, node));
        }
    }
}

/// Replaces annotation ordinals with the type their binding names, where the
/// file's bindings name one.
fn rebind_annotations(receiver: &mut ReceiverType, declared: &[Option<String>]) {
    match receiver {
        ReceiverType::Annotation(ordinal) => {
            if let Some(Some(value)) = declared.get(*ordinal) {
                *receiver = ReceiverType::Declared(value.clone());
            }
        }
        ReceiverType::Field(inner, _) | ReceiverType::Try(inner) => {
            rebind_annotations(inner, declared);
        }
        _ => {}
    }
}

/// What a `type_uses` reference names through this file's own bindings, as a
/// declared-type value: `key:` a declaration in the same module, `path:` an
/// imported (expanded) or anchored path. Rust modules do not inherit their
/// parent's names, so lookup stops at a module; a glob import in the way makes
/// the name unknown rather than guessed.
fn type_binding(extraction: &Extraction, fact: &ReferenceFact) -> Option<String> {
    if fact.kind != EdgeKind::TypeUses {
        return None;
    }
    let name = fact.name.as_str();
    let (base, rest) = match name.split_once("::") {
        Some((base, rest)) => (base, Some(rest)),
        None => (name, None),
    };
    if matches!(base, "crate" | "self" | "super") {
        return Some(format!("path:{name}"));
    }
    let span = fact.span?;
    let mut scope = fact.scope;
    while let Some(id) = scope {
        let binding = extraction
            .bindings
            .iter()
            .filter(|binding| {
                binding.scope == id
                    && binding.name == base
                    && binding.visible_from <= span.start_byte
                    && matches!(binding.kind.as_str(), "rust_import" | "declaration")
            })
            .max_by_key(|binding| binding.visible_from);
        if let Some(binding) = binding {
            if binding.kind == "declaration" {
                return rest
                    .is_none()
                    .then(|| binding.target_key.as_ref().map(|key| format!("key:{key}")))
                    .flatten();
            }
            let import = extraction.references.iter().find(|reference| {
                reference.span == Some(binding.span)
                    && reference
                        .rust_use
                        .as_ref()
                        .is_some_and(|import| import.local_name.as_deref() == Some(base))
            })?;
            return Some(match rest {
                Some(rest) => format!("path:{}::{rest}", import.name),
                None => format!("path:{}", import.name),
            });
        }
        let globbed = extraction.references.iter().any(|reference| {
            reference.scope == Some(id) && reference.rust_use.as_ref().is_some_and(|u| u.glob)
        });
        let scope_fact = extraction.scopes.get(id)?;
        if globbed || scope_fact.kind == "module" {
            return None;
        }
        scope = scope_fact.parent;
    }
    // Unbound, but spelled through another crate: resolvable as a path.
    rest.is_some().then(|| format!("path:{name}"))
}

/// The concrete type names a type node refers to: `Vec<u8>` yields `Vec`,
/// `std::io::Result<T>` yields `std::io::Result`.
///
/// Every type wrapper (`&T`, `*T`, `[T; N]`, `(A, B)`, `fn(T) -> U`,
/// `<T as Trait>::Assoc`, generic argument lists) carries the referred type in a
/// named child, so the walk descends into unnamed wrapper kinds rather than
/// matching a closed list. Otherwise only direct field types produced
/// `type_uses`, and a type used solely as a parameter or return type was
/// invisible (finding E5).
/// The generic type-parameter names an item declares (`fn f<T, U: Bound>` →
/// `[T, U]`), so their bare uses in parameter/return/body/field positions are
/// not mistaken for references to a workspace type. Lifetimes and const
/// parameters are not type names and are skipped; trait bounds (`U: Bound`) are
/// left for the normal type-use walk.
fn type_parameter_names(node: Node<'_>, source: &str) -> Vec<String> {
    let Some(params) = node.child_by_field_name("type_parameters") else {
        return Vec::new();
    };
    let text_of = |node: Node<'_>| -> String {
        let start = node.start_byte().min(source.len());
        let end = node.end_byte().min(source.len());
        source.get(start..end).unwrap_or_default().to_owned()
    };
    let mut names = Vec::new();
    let mut cursor = params.walk();
    for child in params.named_children(&mut cursor) {
        match child.kind() {
            "lifetime" | "const_parameter" => {}
            "type_identifier" => names.push(text_of(child)),
            _ => {
                // constrained/optional/higher-ranked: the parameter name is the
                // first direct `type_identifier`; bounds are nested deeper.
                let mut inner = child.walk();
                if let Some(name) = child
                    .named_children(&mut inner)
                    .find(|node| node.kind() == "type_identifier")
                {
                    names.push(text_of(name));
                }
            }
        }
    }
    names
}

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
                // tree-sitter-rust names a generic's argument list
                // `type_arguments`; `Vec<Foo>` uses `Foo` too.
                if let Some(args) = current.child_by_field_name("type_arguments") {
                    stack.push(args);
                }
            }
            "scoped_type_identifier" => {
                names.push(text_of(current, source).to_owned());
                if let Some(args) = current.child_by_field_name("arguments") {
                    stack.push(args);
                }
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

// Attribute syntax is owned by the parser. Unknown macro/cfg_attr attributes
// may rewrite module paths and must not silently fall back to default filenames.
fn visibility_modifier(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "visibility_modifier")
}

/// An impl type's path without its generic arguments: `Foo<T>` → `Foo`,
/// `path::Foo<T>` → `path::Foo`, `Foo` → `Foo`. The base path is everything
/// before the first `<`.
fn impl_type_path(type_text: &str) -> String {
    type_text
        .split_once('<')
        .map_or(type_text, |(base, _)| base)
        .trim()
        .to_owned()
}

fn module_path(node: Node<'_>, source: &str) -> (Option<String>, bool) {
    let mut attributes = Vec::new();
    let mut previous = node.prev_named_sibling();
    while let Some(attribute) = previous {
        match attribute.kind() {
            "line_comment" | "block_comment" => {}
            "attribute_item" => attributes.push(attribute),
            _ => break,
        }
        previous = attribute.prev_named_sibling();
    }
    let attribute_body = if node.kind() == "source_file" {
        Some(node)
    } else {
        node.child_by_field_name("body")
    };
    if let Some(body) = attribute_body {
        let mut cursor = body.walk();
        attributes.extend(
            body.named_children(&mut cursor)
                .filter(|child| child.kind() == "inner_attribute_item"),
        );
    }
    let mut path = None;
    let mut unsupported = false;
    for attribute in attributes {
        let text = crate::walk::text(attribute, source).trim();
        let text = text
            .strip_prefix("#[")
            .or_else(|| text.strip_prefix("#!["))
            .and_then(|text| text.strip_suffix(']'))
            .unwrap_or("")
            .trim();
        let name = text
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .next()
            .unwrap_or("");
        if name == "path" {
            let value = text
                .strip_prefix("path")
                .and_then(|text| text.trim_start().strip_prefix('='))
                .and_then(|text| module_string(text.trim()));
            if path.is_some() || value.is_none() {
                unsupported = true;
            }
            path = value;
        } else if name == "cfg_attr" {
            // A conditional attribute can only move a module when it may apply
            // `path`; any other payload leaves the module's file where it is.
            if text
                .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                .any(|word| word == "path")
            {
                unsupported = true;
            }
        } else if !matches!(
            name,
            "cfg"
                | "doc"
                | "allow"
                | "warn"
                | "deny"
                | "forbid"
                | "expect"
                | "deprecated"
                | "macro_use"
                | "rustfmt"
                | "no_std"
                | "no_implicit_prelude"
                | "recursion_limit"
                | "feature"
        ) {
            unsupported = true;
        }
    }
    (path, unsupported)
}

fn module_string(text: &str) -> Option<String> {
    let value = if let Some(raw) = text.strip_prefix('r') {
        let hashes = raw.bytes().take_while(|byte| *byte == b'#').count();
        let quote = raw.get(hashes..)?.strip_prefix('"')?;
        quote.strip_suffix(&format!("\"{}", "#".repeat(hashes)))?
    } else {
        let value = text.strip_prefix('"')?.strip_suffix('"')?;
        if value.contains('\\') || value.contains('"') {
            return None;
        }
        value
    };
    (!value.is_empty()
        && !value.contains('\0')
        && value.len() <= graph_search_types::limits::MAX_CARGO_TARGET_PATH_BYTES)
        .then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(text: &str) -> Extraction {
        let file = SourceFile {
            path: Path::new("src/t.rs"),
            text,
        };
        RustExtractor
            .extract(&file)
            .unwrap_or_else(|e| panic!("extract: {e}"))
    }

    #[test]
    fn module_attributes_and_reference_spans_preserve_declaration_context() {
        let code = r##"#[path = r#"é.rs"#] mod external;
mod inline { #![path="custom"] mod child; }
#[cfg_attr(unix,path="other.rs")] mod conditional;
#[path="one.rs"] #[path="two.rs"] mod duplicate;
#[path="escaped\x2ers"] mod escaped;
"##;
        let extraction = extract(code);
        let modules: Vec<_> = extraction
            .symbols
            .iter()
            .filter(|node| node.kind == NodeKind::Module)
            .collect();
        assert_eq!(
            modules[0]
                .attributes
                .get("rust_module_path")
                .map(String::as_str),
            Some("é.rs")
        );
        assert_eq!(
            modules[1]
                .attributes
                .get("rust_module_path")
                .map(String::as_str),
            Some("custom")
        );
        for name in ["conditional", "duplicate", "escaped"] {
            assert!(
                modules
                    .iter()
                    .find(|node| node.name == name)
                    .unwrap()
                    .attributes
                    .contains_key("rust_module_unavailable")
            );
        }
        for reference in &extraction.references {
            assert!(reference.rust_module_declaration);
            let span = reference.span.unwrap();
            assert!(code[span.start_byte as usize..span.end_byte as usize].starts_with("mod "));
        }
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
    fn nested_use_groups_preserve_self_and_sibling_paths() {
        let extraction =
            extract("use crate::outer::{self, nested::{alpha, beta as renamed}, gamma};");
        let paths: Vec<_> = extraction
            .references
            .iter()
            .filter(|r| r.kind == EdgeKind::Imports)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "crate::outer",
                "crate::outer::nested::alpha",
                "crate::outer::nested::beta",
                "crate::outer::gamma"
            ]
        );
    }

    #[test]
    fn use_leaves_keep_aliases_globs_visibility_and_original_scopes() {
        let source = "// é\r\nmod inner { pub(crate) use crate /* comment */ :: outer::{self as base, nested::{alpha as renamed, beta}, r#type as _, *}; }";
        let extraction = extract(source);
        let facts: Vec<_> = extraction
            .references
            .iter()
            .filter(|r| r.rust_use.is_some())
            .collect();
        let expected = [
            ("crate::outer", Some("base"), false),
            ("crate::outer::nested::alpha", Some("renamed"), false),
            ("crate::outer::nested::beta", Some("beta"), false),
            ("crate::outer::r#type", None, false),
            ("crate::outer::*", None, true),
        ];
        assert_eq!(facts.len(), expected.len());
        for (fact, (path, local, glob)) in facts.iter().zip(expected) {
            assert_eq!(fact.name, path);
            let import = fact.rust_use.as_ref().unwrap();
            assert_eq!(import.local_name.as_deref(), local);
            assert_eq!(import.glob, glob);
            assert_eq!(import.visibility.as_deref(), Some("pub(crate)"));
            let span = fact.span.unwrap();
            assert_eq!(
                fact.raw_name.as_deref(),
                Some(&source[span.start_byte as usize..span.end_byte as usize])
            );
            assert_eq!(extraction.scopes[fact.scope.unwrap()].kind, "module");
            assert!(!fact.dynamic);
        }
        assert!(facts[0].rust_use.as_ref().unwrap().type_only);
        assert!(
            facts[1..]
                .iter()
                .all(|r| !r.rust_use.as_ref().unwrap().type_only)
        );
        let trailing = extract("use crate::api::self as api; use self::*;");
        assert_eq!(trailing.references[0].name, "crate::api");
        assert!(trailing.references[0].rust_use.as_ref().unwrap().type_only);
        assert!(!trailing.references[1].rust_use.as_ref().unwrap().type_only);
        let absolute = extract("use ::package::{item, nested::{self, leaf}};");
        assert_eq!(
            absolute
                .references
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            [
                "::package::item",
                "::package::nested",
                "::package::nested::leaf"
            ]
        );
    }

    #[test]
    fn unsupported_use_leaf_discards_partial_expansion() {
        let extraction = extract("use crate::{known, $unknown};");
        assert_eq!(extraction.references.len(), 1);
        let fact = &extraction.references[0];
        assert!(fact.dynamic);
        assert_eq!(
            fact.unresolved_reason.as_deref(),
            Some("rust_use_syntax_unsupported")
        );
        assert_eq!(fact.rust_use.as_ref().unwrap().local_name, None);
        assert!(extract("use crate::empty::{};").references.is_empty());
    }

    #[test]
    fn oversized_import_alias_expansion_stays_explicitly_unresolved() {
        let source = format!(
            "use crate::{} as relay; fn call() {{ relay(); }}",
            "x".repeat(4096)
        );
        let extraction = extract(&source);
        let call = extraction
            .references
            .iter()
            .find(|r| r.kind == EdgeKind::Calls)
            .unwrap();
        assert!(call.dynamic);
        assert_eq!(
            call.unresolved_reason.as_deref(),
            Some("rust_import_expansion_limit")
        );
        assert_eq!(call.name, "relay");
        assert_eq!(call.raw_name.as_deref(), Some("relay"));
        assert!(call.binding.is_some());
        assert!(call.via_import.is_none());
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
    fn nested_scope_keys_do_not_repeat_entire_ancestor_chains() {
        for depth in [4, 8, 12, 16] {
            let text = format!(
                "{}fn caller() {{ target(); }} fn target() {{}}{}",
                "mod layer {".repeat(depth),
                "}".repeat(depth)
            );
            let extraction = extract(&text);
            let bytes: usize = extraction.symbols.iter().map(|s| s.key.len()).sum();
            assert!(bytes < 64 * (depth + 2).pow(3), "depth={depth}: {bytes}");
            for symbol in &extraction.symbols {
                if let Some(parent) = &symbol.parent_key {
                    assert_eq!(
                        extraction
                            .symbols
                            .iter()
                            .filter(|s| &s.key == parent)
                            .count(),
                        1
                    );
                }
            }
            let caller = extraction
                .symbols
                .iter()
                .find(|s| s.name == "caller")
                .unwrap();
            let call = extraction
                .references
                .iter()
                .find(|r| r.kind == EdgeKind::Calls)
                .unwrap();
            assert_eq!(call.from_key.as_deref(), Some(caller.key.as_str()));
        }
    }

    #[test]
    fn nested_keys_preserve_duplicate_groups_and_distinct_parents() {
        let extraction = extract(
            "mod a { mod inner { fn same() {} fn same() {} } }
             mod b { mod inner { fn same() {} } }",
        );
        let same: Vec<_> = extraction
            .symbols
            .iter()
            .filter(|s| s.name == "same")
            .collect();
        assert_eq!(same.len(), 3);
        // Duplicate declarations remain a group for core's ambiguity handling.
        assert_eq!(same[0].key, same[1].key);
        assert_eq!(same[0].parent_key, same[1].parent_key);
        assert_ne!(same[0].key, same[2].key);
        assert_ne!(same[0].parent_key, same[2].parent_key);
        assert_eq!(same[0].qualified_name, "a::inner::same");
        assert_eq!(same[2].qualified_name, "b::inner::same");
    }

    #[test]
    fn explicit_visibility_uses_the_grammar_modifier_node() {
        let extraction = extract(
            "pub fn public() {} pub(crate) fn internal() {} pub(super) fn parent() {} pub(self) fn local() {} fn omitted() {} pub(in crate::area) fn restricted() {}",
        );
        let actual: Vec<_> = extraction
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.visibility))
            .collect();
        assert_eq!(
            actual,
            vec![
                ("public", Some(Visibility::Public)),
                ("internal", Some(Visibility::Crate)),
                ("parent", Some(Visibility::Super)),
                ("local", Some(Visibility::Private)),
                ("omitted", None),
                ("restricted", None)
            ]
        );
        assert_eq!(
            extraction.symbols[5]
                .attributes
                .get("rust_visibility_modifier")
                .map(String::as_str),
            Some("pub(in crate::area)")
        );
        assert!(
            !extraction.symbols[4]
                .attributes
                .contains_key("rust_visibility_modifier")
        );
        for (modifier, expected) in [
            ("pub ( crate )", Visibility::Crate),
            ("pub(/* scope */super)", Visibility::Super),
            ("pub(in self)", Visibility::Private),
            ("pub(in crate)", Visibility::Crate),
        ] {
            let source = format!("{modifier} struct Example {{ {modifier} field: u8 }}");
            let extraction = extract(&source);
            assert_eq!(extraction.symbols.len(), 2);
            for symbol in extraction.symbols {
                assert_eq!(symbol.visibility, Some(expected), "{modifier}: {symbol:?}");
                assert_eq!(
                    symbol
                        .attributes
                        .get("rust_visibility_modifier")
                        .map(String::as_str),
                    Some(modifier)
                );
            }
        }
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
