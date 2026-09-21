//! Direct JS/TS class-static bindings. No inheritance or property dataflow.
use crate::walk::span_of;
use graph_search_types::extraction::{BindingFact, Extraction};
use graph_search_types::{NodeKind, Span};
use std::collections::BTreeMap;
use tree_sitter::Node;

#[derive(Default)]
struct Class {
    members: BTreeMap<String, Option<(String, String)>>,
    deferred: Vec<Span>,
    declaration: Option<Span>,
}

pub(crate) struct Classes(BTreeMap<String, Class>);

impl Classes {
    pub(crate) fn new(extraction: &Extraction) -> Self {
        let mut counts = BTreeMap::new();
        for symbol in &extraction.symbols {
            let count = counts.entry(symbol.key.as_str()).or_insert(0usize);
            *count = count.saturating_add(1);
        }
        let mut classes: BTreeMap<_, _> = extraction
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == NodeKind::Class && counts[symbol.key.as_str()] == 1)
            .map(|symbol| {
                (
                    symbol.key.clone(),
                    Class {
                        declaration: Some(symbol.span),
                        ..Class::default()
                    },
                )
            })
            .collect();
        for symbol in &extraction.symbols {
            if symbol.kind != NodeKind::Method
                || symbol.attributes.get("static_callable").map(String::as_str) != Some("true")
            {
                continue;
            }
            let Some(class) = symbol
                .parent_key
                .as_ref()
                .and_then(|key| classes.get_mut(key))
            else {
                continue;
            };
            let target = (counts[symbol.key.as_str()] == 1)
                .then(|| (symbol.key.clone(), symbol.qualified_name.clone()));
            class
                .members
                .entry(symbol.name.clone())
                .and_modify(|value| *value = None)
                .or_insert(target);
        }
        Self(classes)
    }

    pub(crate) fn record_deferred(&mut self, key: &str, declaration: Node<'_>) {
        let Some(class) = self.0.get_mut(key) else {
            return;
        };
        let Some(body) = declaration.child_by_field_name("body") else {
            return;
        };
        let mut cursor = body.walk();
        for method in body
            .named_children(&mut cursor)
            .filter(|node| node.kind() == "method_definition")
        {
            // Parameters and bodies execute when the method is called. Computed
            // names, heritage and field initializers are deliberately excluded.
            for field in ["parameters", "body"] {
                if let Some(node) = method.child_by_field_name(field) {
                    class.deferred.push(span_of(node));
                }
            }
        }
        class.deferred.sort_by_key(|span| span.start_byte);
    }

    pub(crate) fn resolve(
        &self,
        binding: &BindingFact,
        suffix: &str,
        span: Span,
    ) -> Option<Member<'_>> {
        let class = binding
            .target_key
            .as_ref()
            .and_then(|key| self.0.get(key))?;
        let position = class
            .deferred
            .partition_point(|body| body.start_byte <= span.start_byte);
        let deferred = position
            .checked_sub(1)
            .is_some_and(|i| span.end_byte <= class.deferred[i].end_byte);
        let initialized = span.start_byte >= binding.initialized_from || deferred;
        let target = suffix
            .strip_prefix('.')
            .and_then(|name| class.members.get(name))
            .and_then(Option::as_ref);
        Some(if !initialized {
            Member::Unresolved(
                if class.declaration.is_some_and(|declaration| {
                    declaration.start_byte <= span.start_byte
                        && span.end_byte <= declaration.end_byte
                }) {
                    "class_initialization_context_unsupported"
                } else {
                    "binding_before_initialization"
                },
            )
        } else if let Some((key, qualified)) = target {
            Member::Target(key, qualified)
        } else {
            Member::Unresolved("class_static_member_missing_or_ambiguous")
        })
    }
}

pub(crate) enum Member<'a> {
    Target(&'a str, &'a str),
    Unresolved(&'static str),
}
