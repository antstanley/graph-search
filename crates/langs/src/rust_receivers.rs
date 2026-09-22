//! Syntactic Rust receiver types: what a method call's receiver is, as far as
//! the file's own syntax states it (`SPEC.md` § Rust receiver types).
//!
//! Nothing here infers types the way a compiler does. A receiver is typed only
//! when an annotation, a struct literal, a field, `self`, or the declared return
//! type of another call names it; everything else stays untyped, and an untyped
//! pattern binding shadows an outer typed one so a stale type is never reused.
use graph_search_types::extraction::ReceiverType;
use std::collections::HashMap;
use tree_sitter::Node;

/// Wrappers a method call auto-dereferences through to the type it is called on.
const POINTERS: &[&str] = &[
    "Box",
    "Rc",
    "Arc",
    "Ref",
    "RefMut",
    "MutexGuard",
    "RwLockReadGuard",
    "RwLockWriteGuard",
    "Cow",
    "Pin",
];

/// Methods that return their receiver's own type.
const SAME_TYPE: &[&str] = &["clone", "to_owned"];

/// One local binding in a function body.
#[derive(Clone, Debug)]
pub(crate) struct Local {
    pub(crate) name: String,
    /// First byte the binding is visible from.
    pub(crate) from: usize,
    /// Last byte the binding is visible to.
    pub(crate) until: usize,
    /// The binding's own type, when stated.
    pub(crate) plain: Option<ReceiverType>,
    /// Its `Option`/`Result` success type, when stated.
    pub(crate) fallible: Option<ReceiverType>,
}

/// The local visible at `at` named `name`: the latest declared one whose
/// extent contains the use.
pub(crate) fn lookup<'a>(locals: &'a [Local], name: &str, at: usize) -> Option<&'a Local> {
    locals
        .iter()
        .filter(|local| local.name == name && local.from <= at && at <= local.until)
        .max_by_key(|local| local.from)
}

fn last_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    let start = node.start_byte().min(source.len());
    let end = node.end_byte().min(source.len());
    source.get(start..end).unwrap_or_default()
}

/// The first type argument of a generic type (`Rc<T>` → `T`), skipping
/// lifetimes.
fn first_argument(node: Node<'_>) -> Option<Node<'_>> {
    let arguments = node.child_by_field_name("type_arguments")?;
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .find(|child| child.kind() != "lifetime")
}

/// The type a method call on a value of type `node` looks methods up on:
/// references, smart pointers and `dyn`/`impl` trait objects are peeled.
pub(crate) fn peel<'t>(mut node: Node<'t>, source: &str) -> Node<'t> {
    for _ in 0..16 {
        let next = match node.kind() {
            "reference_type" => node.child_by_field_name("type"),
            "generic_type" => node
                .child_by_field_name("type")
                .filter(|base| POINTERS.contains(&last_segment(text(*base, source))))
                .and_then(|_| first_argument(node)),
            "dynamic_type" | "abstract_type" => node.child_by_field_name("trait"),
            "bounded_type" => node.named_child(0),
            _ => None,
        };
        match next {
            Some(next) => node = next,
            None => break,
        }
    }
    node
}

/// The success type inside an `Option`/`Result`, peeled, when `node` is one.
pub(crate) fn fallible<'t>(node: Node<'t>, source: &str) -> Option<Node<'t>> {
    let node = peel(node, source);
    (node.kind() == "generic_type"
        && node
            .child_by_field_name("type")
            .is_some_and(|base| matches!(last_segment(text(base, source)), "Option" | "Result")))
    .then(|| first_argument(node))
    .flatten()
    .map(|inner| peel(inner, source))
}

/// The name a `type_uses` reference records for a (peeled) core type, when the
/// core is a nominal type.
pub(crate) fn core_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "type_identifier" | "scoped_type_identifier" => Some(text(node, source).to_owned()),
        "generic_type" => node
            .child_by_field_name("type")
            .map(|base| text(base, source).to_owned()),
        _ => None,
    }
}

/// Every identifier a pattern binds, for untyped shadowing bindings.
pub(crate) fn pattern_identifiers(pattern: Node<'_>) -> Vec<Node<'_>> {
    let mut found = Vec::new();
    let mut stack = vec![pattern];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" {
            found.push(node);
            continue;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    found
}

/// What the receiver expression `node` evaluates to, given the locals in
/// scope, the `type_uses` ordinals of type nodes, and the call ordinals of
/// call nodes already walked.
pub(crate) struct Context<'a> {
    pub(crate) source: &'a str,
    pub(crate) locals: &'a [Local],
    pub(crate) self_type: Option<&'a str>,
    /// `type_uses` ordinals by outermost type node id: `(name, ordinal)`.
    pub(crate) type_facts: &'a HashMap<usize, Vec<(String, usize)>>,
    /// Call ordinals by call node id.
    pub(crate) call_facts: &'a HashMap<usize, usize>,
}

impl Context<'_> {
    /// The `type_uses` ordinal naming the core of `ty` (peeled), where `outer`
    /// is the type node the references were recorded for.
    pub(crate) fn type_fact(&self, outer: Node<'_>, ty: Node<'_>) -> Option<usize> {
        let name = core_name(ty, self.source)?;
        self.type_facts
            .get(&outer.id())?
            .iter()
            .find(|(recorded, _)| *recorded == name)
            .map(|(_, ordinal)| *ordinal)
    }

    pub(crate) fn receiver(&self, node: Node<'_>) -> Option<ReceiverType> {
        match node.kind() {
            "self" => self
                .self_type
                .map(|ty| ReceiverType::SelfType(ty.to_owned())),
            "identifier" => lookup(self.locals, text(node, self.source), node.start_byte())
                .and_then(|local| local.plain.clone()),
            "field_expression" => {
                let value = node.child_by_field_name("value")?;
                let field = node.child_by_field_name("field")?;
                if field.kind() != "field_identifier" {
                    return None;
                }
                Some(ReceiverType::Field(
                    Box::new(self.receiver(value)?),
                    text(field, self.source).to_owned(),
                ))
            }
            "call_expression" => {
                let function = node.child_by_field_name("function")?;
                if function.kind() == "field_expression"
                    && let Some(method) = function.child_by_field_name("field")
                    && let Some(value) = function.child_by_field_name("value")
                {
                    let method = text(method, self.source);
                    if SAME_TYPE.contains(&method) {
                        return self.receiver(value);
                    }
                    if matches!(method, "unwrap" | "expect") {
                        return self.fallible(value);
                    }
                }
                self.call_facts
                    .get(&node.id())
                    .copied()
                    .map(ReceiverType::Return)
            }
            "try_expression" => self.fallible(node.named_child(0)?),
            "await_expression" | "parenthesized_expression" | "unary_expression" => {
                self.receiver(node.named_child(0)?)
            }
            "reference_expression" => self.receiver(node.child_by_field_name("value")?),
            "struct_expression" => {
                let name = node.child_by_field_name("name")?;
                self.type_fact(name, name).map(ReceiverType::Annotation)
            }
            _ => None,
        }
    }

    /// The success type of a fallible receiver (`x?`, `x.unwrap()`).
    pub(crate) fn fallible(&self, node: Node<'_>) -> Option<ReceiverType> {
        if node.kind() == "identifier" {
            return lookup(self.locals, text(node, self.source), node.start_byte())
                .and_then(|local| local.fallible.clone());
        }
        self.receiver(node)
            .map(|receiver| ReceiverType::Try(Box::new(receiver)))
    }
}
