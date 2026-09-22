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
use std::collections::{BTreeMap, BTreeSet};
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
            keys: BTreeSet::new(),
            modules: module_bindings(tree.root_node(), file.text),
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
    /// The enclosing symbol's syntax, bounding what a function body declares.
    span: Span,
    /// PEP 695 type parameters this item declares (`[T, U]`). Their bare uses
    /// in child positions are lexical bindings, not references to a workspace
    /// type, so they must not become `type_uses` edges.
    type_params: Vec<String>,
}

struct Extractor<'a> {
    source: &'a str,
    extraction: Extraction,
    scope: Vec<Scope>,
    /// Every symbol fact key emitted so far, to keep keys unique.
    keys: BTreeSet<String>,
    /// Names `import` statements bind to modules (see [`module_bindings`]).
    modules: BTreeMap<String, Option<String>>,
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
        crate::walk::qualify(parent, name, kind, ".")
    }

    /// Pushes a symbol and its scope; the caller pops after walking children.
    fn emit(&mut self, node: Node<'_>, kind: NodeKind, name: String, signature: String) {
        let (mut key, qualified) = self.qualify(&name, kind);
        // Python rebinds names freely (`@x.setter`, `@overload`, conditional
        // `def`s), but a fact key must be unique within the file: a repeat takes
        // its `#line` (then byte) disambiguator.
        if self.keys.contains(&key) {
            key = format!("{key}#{}", Self::line(node));
            if self.keys.contains(&key) {
                key = format!("{key}@{}", node.start_byte());
            }
        }
        self.keys.insert(key.clone());
        let parent = self.scope.last().map(|scope| scope.key.clone());
        let mut fact =
            SymbolFact::new(key.clone(), kind, name, qualified.clone(), Self::span(node))
                .with_signature(signature);
        if let Some(parent) = parent {
            fact = fact.with_parent(parent);
        }
        // A `def` or `class` inside a function body binds a local name: it is
        // visible only within that function, never file- or workspace-wide.
        if let Some(function) = self
            .scope
            .iter()
            .rev()
            .find(|scope| scope.context == Context::Function)
        {
            fact = fact
                .with_attribute("lexical_local", "true")
                .with_attribute("lexical_start", function.span.start_byte.to_string())
                .with_attribute("lexical_end", function.span.end_byte.to_string());
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
            span: Self::span(node),
            type_params: Vec::new(),
        });
    }

    fn current_context(&self) -> Context {
        self.scope
            .last()
            .map_or(Context::Module, |scope| scope.context)
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
    /// callee spelling for lexical analysis. A module-qualified call carries
    /// the module it names as its import provenance.
    fn reference_call(&mut self, name: &str, raw: &str, module: Option<&str>, node: Node<'_>) {
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
        let mut fact = fact.at(Self::span(node));
        if let Some(module) = module {
            fact = fact.via_import(module);
        }
        self.extraction.references.push(fact);
    }

    fn is_async(node: Node<'_>) -> bool {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .any(|child| child.kind() == "async")
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
        if let Some(parameters) = node.child_by_field_name("type_parameters") {
            self.type_parameters(parameters);
        }
        if let Some(superclasses) = node.child_by_field_name("superclasses") {
            self.heritage(superclasses);
        }
        if let Some(body) = node.child_by_field_name("body") {
            self.walk_children(body);
        }
        self.scope.pop();
    }

    /// `class C(Base, other.Mod, Generic[T])`: each named base is an `extends`
    /// edge, a subscripted (generic) base naming the class it parameterizes.
    /// A keyword argument (`metaclass=...`) is not a base.
    fn heritage(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let bases: Vec<Node<'_>> = node
            .named_children(&mut cursor)
            .filter_map(|child| match child.kind() {
                "subscript" => child.child_by_field_name("value"),
                _ => Some(child),
            })
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
        let is_async = Self::is_async(node);
        self.emit(node, kind, name, self.first_line(node));
        if is_async && let Some(symbol) = self.extraction.symbols.last_mut() {
            symbol.is_async = true;
        }
        if let Some(parameters) = node.child_by_field_name("type_parameters") {
            self.type_parameters(parameters);
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

    /// `type Name = ...` and the generic `type Name[T] = ...`, whose name is
    /// the bare identifier and whose `[T]` are type parameters.
    fn type_alias(&mut self, node: Node<'_>) {
        let Some(left) = node.child_by_field_name("left") else {
            self.walk_children(node);
            return;
        };
        let generic = left
            .named_child(0)
            .filter(|child| child.kind() == "generic_type");
        let name_node = generic.map_or(left, |generic| generic.named_child(0).unwrap_or(generic));
        let name = self.text(name_node).trim().to_owned();
        if name.is_empty() {
            self.walk_children(node);
            return;
        }
        self.emit(node, NodeKind::TypeAlias, name, self.first_line(node));
        let parameters = generic.and_then(|generic| {
            let mut cursor = generic.walk();
            generic
                .named_children(&mut cursor)
                .find(|child| child.kind() == "type_parameter")
        });
        if let Some(parameters) = parameters {
            self.type_parameters(parameters);
        }
        if let Some(right) = node.child_by_field_name("right") {
            self.type_uses(right);
        }
        self.scope.pop();
    }

    /// A PEP 695 `[T, U: Bound]` list: each parameter's name is bound for the
    /// declaring item's scope, and each bound is a `type_uses` edge.
    fn type_parameters(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let parameters: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        let names: Vec<String> = parameters
            .into_iter()
            .filter_map(first_identifier)
            .map(|name| self.text(name).to_owned())
            .collect();
        if let Some(scope) = self.scope.last_mut() {
            scope.type_params.extend(names);
        }
        self.type_uses(node);
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
                if let Some(target) = self.self_member_target(&callee) {
                    self.reference_call(&target, &callee, None, node);
                } else if let Some((module, member)) = self.module_member(&callee) {
                    self.reference_call(&member, &callee, Some(&module), node);
                } else {
                    self.reference_call(&callee, &callee, None, node);
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
        if member.is_empty() || !member.chars().all(|c| c.is_alphanumeric() || c == '_') {
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

    /// `utils.helper()` after `import utils` (or `u.helper()` after `import
    /// utils as u`) calls `helper` in the module the prefix is bound to, so
    /// the call resolves through that module rather than by qualified name:
    /// Python qualified names never include the module.
    fn module_member(&self, callee: &str) -> Option<(String, String)> {
        let (prefix, member) = callee.rsplit_once('.')?;
        if member.is_empty() || !member.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let module = self.modules.get(prefix)?.as_ref()?;
        Some((module.clone(), member.to_owned()))
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
                "identifier" | "attribute" | "dotted_name" => {
                    self.push_type_name(&mut names, self.text(current));
                }
                // A string annotation is a forward reference: the names it
                // spells are the types it uses (`-> "Node"`, `"list[Node]"`).
                "string" => {
                    let mut cursor = current.walk();
                    let contents: Vec<Node<'_>> = current
                        .named_children(&mut cursor)
                        .filter(|child| child.kind() == "string_content")
                        .collect();
                    for content in contents {
                        for name in forward_reference_names(self.text(content)) {
                            self.push_type_name(&mut names, name);
                        }
                    }
                }
                // `Head[args]`: `Literal` arguments are values, not types, and
                // only `Annotated`'s first argument is a type; the rest is
                // metadata.
                "generic_type" | "subscript" => {
                    let (head, arguments) = generic_parts(current);
                    let last = head.map_or("", |head| {
                        let text = self.text(head);
                        text.rsplit('.').next().unwrap_or(text)
                    });
                    let arguments: Vec<Node<'_>> = match last {
                        "Literal" => Vec::new(),
                        "Annotated" => arguments.into_iter().take(1).collect(),
                        _ => arguments,
                    };
                    stack.extend(arguments.into_iter().rev());
                    stack.extend(head);
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

    /// Records a type name unless it names a builtin, a `typing` form or a
    /// type parameter in scope: none of those is a workspace symbol, and a
    /// `type_uses` edge to one could only dangle or bind to an unrelated
    /// workspace class of the same name.
    fn push_type_name(&self, names: &mut Vec<String>, name: &str) {
        let name = name.trim();
        if name.is_empty() || is_builtin_type(name) || is_typing_name(name) {
            return;
        }
        if self
            .scope
            .iter()
            .any(|scope| scope.type_params.iter().any(|param| param == name))
        {
            return;
        }
        names.push(name.to_owned());
    }
}

/// Names `import` statements anywhere in the file bind to modules: `import
/// a.b` binds `a` to module `a` (and spells `a.b`), `import a.b as c` binds `c`
/// to `a.b`. A name bound to two different modules is ambiguous (`None`).
/// `from m import x` is not recorded: `x` may be a symbol rather than a module.
fn module_bindings(root: Node<'_>, source: &str) -> BTreeMap<String, Option<String>> {
    let mut bindings: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut bind = |name: &str, module: &str| {
        let module = Some(module.to_owned());
        let entry = bindings.entry(name.to_owned()).or_insert(module.clone());
        if *entry != module {
            *entry = None;
        }
    };
    crate::walk::walk(root, &mut |node| {
        if node.kind() != "import_statement" {
            return true;
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "dotted_name" => {
                    let module = crate::walk::text(child, source);
                    bind(module, module);
                    if let Some((root, _)) = module.split_once('.') {
                        bind(root, root);
                    }
                }
                "aliased_import" => {
                    if let (Some(name), Some(alias)) = (
                        child.child_by_field_name("name"),
                        child.child_by_field_name("alias"),
                    ) {
                        bind(
                            crate::walk::text(alias, source),
                            crate::walk::text(name, source),
                        );
                    }
                }
                _ => {}
            }
        }
        false
    });
    bindings
}

/// A generic's head (`Optional`, `typing.Dict`) and its type arguments.
fn generic_parts(node: Node<'_>) -> (Option<Node<'_>>, Vec<Node<'_>>) {
    let mut cursor = node.walk();
    if node.kind() == "subscript" {
        let arguments = node
            .children_by_field_name("subscript", &mut cursor)
            .collect();
        return (node.child_by_field_name("value"), arguments);
    }
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    let head = children.first().copied();
    let arguments = children
        .iter()
        .filter(|child| child.kind() == "type_parameter")
        .flat_map(|parameters| {
            let mut cursor = parameters.walk();
            parameters.named_children(&mut cursor).collect::<Vec<_>>()
        })
        .collect();
    (head, arguments)
}

/// The first identifier in a subtree, in source order: a type parameter's name.
fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
    children.into_iter().find_map(first_identifier)
}

/// The dotted names a string forward reference spells.
fn forward_reference_names(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .map(|token| token.trim_matches('.'))
        .filter(|token| {
            token
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
        })
}

/// `typing` special forms and generic aliases, spelled bare or through
/// `typing`/`typing_extensions`/`collections.abc`. They are never workspace
/// symbols, and a bare `Optional` must not bind to one that happens to share
/// the name.
fn is_typing_name(name: &str) -> bool {
    let bare = ["typing.", "typing_extensions.", "collections.abc."]
        .into_iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name);
    matches!(
        bare,
        "Any"
            | "Annotated"
            | "AsyncGenerator"
            | "AsyncIterable"
            | "AsyncIterator"
            | "Awaitable"
            | "Callable"
            | "ClassVar"
            | "Collection"
            | "Concatenate"
            | "Container"
            | "Coroutine"
            | "DefaultDict"
            | "Deque"
            | "Dict"
            | "Final"
            | "FrozenSet"
            | "Generator"
            | "Generic"
            | "Hashable"
            | "Iterable"
            | "Iterator"
            | "List"
            | "Literal"
            | "LiteralString"
            | "Mapping"
            | "MutableMapping"
            | "MutableSequence"
            | "MutableSet"
            | "Never"
            | "NoReturn"
            | "NotRequired"
            | "Optional"
            | "ParamSpec"
            | "Protocol"
            | "ReadOnly"
            | "Required"
            | "Self"
            | "Sequence"
            | "Set"
            | "Sized"
            | "Tuple"
            | "Type"
            | "TypeAlias"
            | "TypeGuard"
            | "TypeIs"
            | "TypeVar"
            | "Union"
            | "Unpack"
    )
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
            | "None"
    )
}
