//! Source-region facts published with the graph generation.

use crate::{NodeId, node::Span};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Kind of source evidence independent of graph declaration kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceUnitKind {
    /// Source covered by a supported language adapter.
    Code,
    /// Parser-recognized authored documentation comment.
    DocumentationComment,
    /// Markdown prose, partitioned at supported document boundaries.
    Markdown,
    /// A fenced Markdown code block or bounded fragment of a long block.
    MarkdownCodeFence,
    /// Authored YAML/TOML-style Markdown frontmatter, including its delimiters.
    MarkdownFrontmatter,
    /// A native pipe table or bounded group of its authored rows.
    MarkdownTable,
    /// A prose paragraph or a bounded fragment of it.
    MarkdownParagraph,
    /// A top-level list item, including its authored continuation lines.
    MarkdownListItem,
    /// An unsupported or deliberately opaque Markdown/HTML block.
    MarkdownOpaque,
    /// Configuration or structured data text.
    Configuration,
    /// Other readable UTF-8 source.
    Text,
}

/// A document heading in the same hash-bound source as its child evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownHeading {
    /// Heading depth, from one through six.
    pub level: u8,
    /// Complete authored heading, including its marker/underline.
    pub span: Span,
    /// Authored title bytes, without ATX markers or a Setext underline.
    /// Inline Markdown remains uninterpreted; an empty ATX title is valid.
    pub title_span: Span,
}

/// Original fenced block shared by its bounded retrieval fragments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownFence {
    /// Entire authored block, including its delimiters when present.
    pub span: Span,
    /// Code bytes without opener/closer lines; an empty body is valid.
    pub content_span: Span,
    /// Trimmed authored info string, without interpreting attributes or escapes.
    pub info_span: Span,
    /// First ASCII-whitespace-delimited info token, when present.
    /// This is an authored label, not a verified parser/language selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_span: Option<Span>,
    /// Whether the source includes a compatible closing delimiter.
    pub closed: bool,
}

/// Shared header context for bounded groups of original table rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownTable {
    /// Entire table in original source coordinates.
    pub span: Span,
    /// Authored header row, including its original line ending.
    pub header_span: Span,
    /// Authored delimiter/alignment row.
    pub delimiter_span: Span,
    /// Matching header/delimiter column count; data rows are not normalized.
    pub columns: u32,
}

/// Original prose/container block shared by bounded retrieval fragments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownBlock {
    /// Complete authored block, including attached trailing blank lines.
    pub span: Span,
    /// Authored list marker, without indentation or following whitespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker_span: Option<Span>,
    /// Nested or unsupported syntax is retained without claiming its structure.
    pub contains_unsupported: bool,
}

/// Authored link syntax; destinations are not resolved or fetched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkdownLinkKind {
    /// `[label](destination)` with an optional quoted title.
    Inline,
    /// `![label](destination)` with an optional quoted title.
    Image,
    /// An angle-delimited URI with a valid ASCII scheme.
    Autolink,
}

/// Coordinates of one authored Markdown link in the same source version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownLink {
    /// Complete authored syntax, including delimiters.
    pub span: Span,
    /// Label bytes without surrounding brackets; markup is not rendered.
    pub label_span: Span,
    /// Destination bytes without angle brackets, with escapes preserved.
    pub destination_span: Span,
    /// Optional title bytes without surrounding quotes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_span: Option<Span>,
    /// Recognized source syntax.
    pub kind: MarkdownLinkKind,
}

/// Original documentation comment group and its separately associated declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentationComment {
    /// Complete authored comment group, shared by bounded fragments.
    pub span: Span,
    /// Declaration documented by this comment; does not imply containment.
    pub documented_symbol: Option<NodeId>,
    /// Rust inner documentation, which documents its enclosing declaration/file.
    pub inner: bool,
}

/// A bounded region of one original source file. Terms carry absolute line
/// occurrences; phrase verification must use hash-verified original bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceUnit {
    /// Original UTF-8 byte and display-line bounds.
    pub span: Span,
    /// Evidence channel.
    pub kind: SourceUnitKind,
    /// Smallest enclosing indexed declaration, when the whole region fits it.
    pub owner: Option<NodeId>,
    /// Parser-owned documentation association, separate from lexical containment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<DocumentationComment>,
    /// Whole lexemes in original spelling, separate from normalized split terms.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub identifiers: BTreeMap<String, Vec<u32>>,
    /// Normalized terms and their one-based source-line occurrences.
    pub terms: BTreeMap<String, Vec<u32>>,
    /// Outer-to-inner heading ancestry; at most six source references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<MarkdownHeading>,
    /// Original fenced-block coordinates, shared across overlapping fragments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fence: Option<MarkdownFence>,
    /// Original table/header coordinates shared across row groups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<MarkdownTable>,
    /// Paragraph, list-item or opaque-block coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<MarkdownBlock>,
    /// Links wholly contained in this region; overlapping windows may share them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MarkdownLink>,
    /// Link metadata is partial because its scan or record allowance was exhausted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub links_truncated: bool,
}

/// Persisted retrieval facts for one source version, without a duplicate source blob.
#[allow(clippy::struct_excessive_bools)] // independent persisted coverage flags
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFileUnits {
    /// Raw candidate JSON/JSONC configuration facts; presence does not select a project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typescript_config: Option<crate::typescript::TypeScriptConfig>,
    /// Raw manifest facts, only for recognized manifest files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manifest: Option<crate::package::PackageManifest>,
    /// A nearest boundary was ambiguous or its metadata unavailable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub package_scope_incomplete: bool,
    /// Hash of the exact original bytes used to construct every region.
    pub source_hash: String,
    /// Nearest unambiguous package boundary in the selected generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<crate::package::PackageIdentity>,
    /// Native source-region representation revision.
    pub version: u32,
    /// Whether a region cap omitted eligible source.
    pub truncated: bool,
    /// Documentation metadata is partial; original body text remains eligible.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub documentation_truncated: bool,
    /// Recognized framework script regions retained for this file.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub embedded_regions: u32,
    /// Recognized framework regions the anchored adapter did not extract.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub embedded_unextracted_regions: u32,
    /// Whether the framework region scan reached an adapter bound.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub embedded_truncated: bool,
    /// Regions in source order.
    pub units: Vec<SourceUnit>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde predicate signature
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Source coordinates supporting a retrieved graph entity or file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceEvidence {
    /// The indexed file's nearest package boundary was ambiguous or unavailable.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub package_scope_incomplete: bool,
    /// Original source region, independent of the declaration's span.
    pub span: Span,
    /// Retrieval channel classification.
    pub kind: SourceUnitKind,
    /// Owning graph declaration, only for facts from the indexed generation.
    pub owner: Option<NodeId>,
    /// Parser-owned documentation association, separate from lexical containment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<DocumentationComment>,
    /// Line chosen for the excerpt from matched term occurrences.
    pub match_line: u32,
    /// Fingerprint of the source from which the match was computed.
    pub source_hash: String,
    /// Inline package identity; absent when `package_ref` uses the result table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<crate::package::PackageIdentity>,
    /// Result-local key in `ResultContext.packages`; never a global package id.
    /// Current results use either this reference or the inline identity, not both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_ref: Option<String>,
    /// True for a request-local replacement of indexed facts.
    pub live: bool,
}

impl SourceEvidence {
    /// Resolve either the inline identity or its result-local shared entry.
    /// Missing or contradictory associations return `None`, never a guessed identity.
    #[must_use]
    pub fn package_identity<'a>(
        &'a self,
        context: &'a crate::context::ResultContext,
    ) -> Option<&'a crate::package::PackageIdentity> {
        match (&self.package, &self.package_ref) {
            (Some(identity), None) => Some(identity),
            (None, Some(key)) => context.packages.get(key),
            _ => None,
        }
    }
}
