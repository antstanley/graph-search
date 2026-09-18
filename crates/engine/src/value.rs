//! Conversion between the port's records and Grafeo's LPG values.
//!
//! Every field that crosses becomes a property with a reserved prefix or a
//! plain name; `attr.`-prefixed properties carry the extractor attributes
//! (`SPEC.md` §5.4, §7.3). These spellings are part of the on-disk format.

use grafeo::Value;
use graph_search_types::kind::{EdgeKind, Language, NodeKind, Visibility};
use graph_search_types::node::{Edge, Node, Span};
use graph_search_types::{EdgeId, NodeId};
use std::collections::BTreeMap;

/// The read-side property map: property names to values, collected from the
/// store's own map so no vendor map type crosses into these helpers.
pub type Props = BTreeMap<String, Value>;

/// The node label every stored node carries, so `all`-node scans are cheap.
pub const LABEL: &str = "node";

/// The property that holds the stable [`NodeId`] string.
pub const PROP_ID: &str = "id";

/// Prefix for extractor attributes (`id`, `classes`, `selector`, ...).
pub const ATTR_PREFIX: &str = "attr.";

/// Turns a node kind into its label spelling.
#[must_use]
pub fn kind_label(kind: NodeKind) -> &'static str {
    kind.as_str()
}

/// The properties of one node, ready for the store.
#[must_use]
pub fn node_to_props(node: &Node) -> Vec<(String, Value)> {
    let mut props: Vec<(String, Value)> = vec![
        (PROP_ID.to_owned(), Value::from(node.id.as_str())),
        (String::from("kind"), Value::from(node.kind.as_str())),
        (String::from("path"), Value::from(node.path.as_str())),
    ];
    if let Some(name) = &node.name {
        props.push((String::from("name"), Value::from(name.as_str())));
    }
    if let Some(qualified) = &node.qualified_name {
        props.push((
            String::from("qualified_name"),
            Value::from(qualified.as_str()),
        ));
    }
    if let Some(signature) = &node.signature {
        props.push((String::from("signature"), Value::from(signature.as_str())));
    }
    if let Some(span) = node.span {
        props.push((
            String::from("start_line"),
            Value::from(i64::from(span.start_line)),
        ));
        props.push((
            String::from("end_line"),
            Value::from(i64::from(span.end_line)),
        ));
        props.push((
            String::from("start_byte"),
            Value::from(i64::from(span.start_byte)),
        ));
        props.push((
            String::from("end_byte"),
            Value::from(i64::from(span.end_byte)),
        ));
    }
    if let Some(language) = node.language {
        props.push((String::from("language"), Value::from(language.as_str())));
    }
    if let Some(bytes) = node.bytes {
        props.push((
            String::from("bytes"),
            Value::from(i64::try_from(bytes).unwrap_or(i64::MAX)),
        ));
    }
    if let Some(lines) = node.lines {
        props.push((String::from("lines"), Value::from(i64::from(lines))));
    }
    if let Some(hash) = &node.content_hash {
        props.push((String::from("content_hash"), Value::from(hash.as_str())));
    }
    if let Some(version) = node.parser_version {
        props.push((
            String::from("parser_version"),
            Value::from(i64::from(version)),
        ));
    }
    if let Some(visibility) = node.visibility {
        let spelling = match visibility {
            Visibility::Public => "public",
            Visibility::Crate => "crate",
            Visibility::Super => "super",
            Visibility::Private => "private",
        };
        props.push((String::from("visibility"), Value::from(spelling)));
    }
    if node.is_async {
        props.push((String::from("is_async"), Value::from(true)));
    }
    if let Some(parent) = &node.parent {
        props.push((String::from("parent"), Value::from(parent.as_str())));
    }
    for (key, value) in &node.attributes {
        props.push((format!("{ATTR_PREFIX}{key}"), Value::from(value.as_str())));
    }
    props
}

fn prop_str<'a>(properties: &'a Props, key: &str) -> Option<&'a str> {
    properties.get(key).and_then(Value::as_str)
}

fn prop_i64(properties: &Props, key: &str) -> Option<u32> {
    properties
        .get(key)
        .and_then(Value::as_int64)
        .and_then(|v| u32::try_from(v).ok())
}

fn prop_u64(properties: &Props, key: &str) -> Option<u64> {
    properties
        .get(key)
        .and_then(Value::as_int64)
        .and_then(|v| u64::try_from(v).ok())
}

/// Rebuilds a [`Node`] from a stored node's labels and properties. `labels`
/// is anything with `as_str()` per label (the store's own label type).
#[must_use]
pub fn node_from_props<'a, L: AsRef<str> + 'a + ?Sized>(
    labels: impl IntoIterator<Item = &'a L>,
    properties: &Props,
) -> Option<Node> {
    let id = prop_str(properties, PROP_ID)?;
    let kind = labels
        .into_iter()
        .map(std::convert::AsRef::as_ref)
        .find(|label| *label != LABEL)
        .and_then(NodeKind::parse)?;
    let mut attributes = BTreeMap::new();
    for (key, value) in properties {
        if let Some(attr) = key.strip_prefix(ATTR_PREFIX)
            && let Some(text) = value.as_str()
        {
            attributes.insert(attr.to_owned(), text.to_owned());
        }
    }
    Some(Node {
        id: NodeId::new(id),
        kind,
        path: prop_str(properties, "path").unwrap_or_default().to_owned(),
        name: prop_str(properties, "name").map(str::to_owned),
        qualified_name: prop_str(properties, "qualified_name").map(str::to_owned),
        signature: prop_str(properties, "signature").map(str::to_owned),
        span: prop_i64(properties, "start_line").map(|start_line| Span {
            start_line,
            end_line: prop_i64(properties, "end_line").unwrap_or(start_line),
            start_byte: prop_i64(properties, "start_byte").unwrap_or(0),
            end_byte: prop_i64(properties, "end_byte").unwrap_or(0),
        }),
        language: prop_str(properties, "language").and_then(Language::parse),
        bytes: prop_u64(properties, "bytes"),
        lines: prop_i64(properties, "lines"),
        content_hash: prop_str(properties, "content_hash").map(str::to_owned),
        parser_version: prop_i64(properties, "parser_version"),
        visibility: prop_str(properties, "visibility")
            .map(str::to_owned)
            .and_then(|v| match v.as_str() {
                "public" => Some(Visibility::Public),
                "crate" => Some(Visibility::Crate),
                "super" => Some(Visibility::Super),
                "private" => Some(Visibility::Private),
                _ => None,
            }),
        is_async: properties
            .get("is_async")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        parent: prop_str(properties, "parent").map(NodeId::new),
        attributes,
    })
}

/// The properties of one edge.
#[must_use]
pub fn edge_to_props(edge: &Edge) -> Vec<(String, Value)> {
    let mut props = vec![(String::from("to_name"), Value::from(edge.to_name.as_str()))];
    if let Some(path) = &edge.path {
        props.push((String::from("ref_path"), Value::from(path.as_str())));
    }
    if let Some(line) = edge.line {
        props.push((String::from("ref_line"), Value::from(i64::from(line))));
    }
    props
}

/// Rebuilds an [`Edge`] from a stored edge.
#[must_use]
pub fn edge_from_stored(
    from_gid: grafeo::NodeId,
    to_gid: grafeo::NodeId,
    edge_type: &str,
    properties: &Props,
    resolve: &dyn Fn(grafeo::NodeId) -> Option<String>,
) -> Option<Edge> {
    let from = NodeId::new(resolve(from_gid)?);
    let to = NodeId::new(resolve(to_gid)?);
    let kind = EdgeKind::parse(edge_type)?;
    Some(Edge {
        id: EdgeId::of(&from, kind, to.as_str()),
        from,
        kind,
        to: Some(to),
        to_name: prop_str(properties, "to_name")
            .unwrap_or_default()
            .to_owned(),
        resolved: true,
        path: prop_str(properties, "ref_path").map(str::to_owned),
        line: prop_i64(properties, "ref_line"),
    })
}
