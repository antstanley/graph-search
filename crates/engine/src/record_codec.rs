//! Native encodings of individually hashed pack records.
//!
//! Source facts (`SourceFileUnits`) are almost entirely per-region term maps:
//! `term -> [line, ...]`. As JSON every map repeats every term string and spells
//! every line number in decimal, so the facts were larger than the source they
//! describe. `GSR1` keeps the record boundary (each record is still hashed,
//! reused across generations and read selectively), and inside it:
//!
//! * one sorted, front-coded dictionary of the record's terms, identifiers and
//!   owners, so each string is stored once per file instead of once per region;
//! * posting maps as ascending dictionary-id deltas with delta-coded lines, all
//!   LEB128 varints (a line delta is almost always one byte);
//! * the source hash as 32 raw bytes;
//! * JSON only for the rare Markdown/documentation/package fields, so their
//!   serde contracts (defaults, optional fields) stay the single definition.
//!
//! ```text
//! "GSR1"
//! dict_len, per string (byte order): shared_prefix_len, suffix_len, suffix
//! hash_tag u8 (1: 32 raw bytes of lowercase hex, 0: len + UTF-8), version, flags u8
//! extras_len (0: none), extras JSON
//! unit_count, per unit:
//!   start_line, zigzag(end_line - start_line), start_byte, zigzag(end_byte - start_byte)
//!   kind u8, owner (0: none, else dict id + 1), extras_len, extras JSON
//!   terms, identifiers: count, per entry: id delta, line_count, line deltas
//! ```
//!
//! Dictionary order is `BTreeMap<String, _>` order, so ids ascend inside every map
//! and decoding bulk-builds each map from sorted input. Deltas use wrapping
//! arithmetic, so every `u32` sequence round-trips exactly. Decoding is bounded:
//! each counted element costs at least one byte, so a count larger than the
//! remaining bytes is rejected before allocation, and trailing bytes are errors.

use graph_search_types::NodeId;
use graph_search_types::manifest::FileEntry;
use graph_search_types::node::Span;
use graph_search_types::source::{
    DocumentationComment, MarkdownBlock, MarkdownFence, MarkdownHeading, MarkdownLink,
    MarkdownTable, SourceFileUnits, SourceUnit, SourceUnitKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io;

/// Record bytes as written into a pack. The bytes are the record's identity.
pub(crate) trait EncodeRecord {
    fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()>;
}

/// Inverse of [`EncodeRecord`] for index format 3 packs.
pub(crate) trait DecodeRecord: Sized {
    fn decode_record(bytes: &[u8]) -> io::Result<Self>;
}

// Dependency records are small sets of names and paths; they keep JSON.
impl EncodeRecord for &graph_search_core::dependencies::DependencyRecord {
    fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()> {
        serde_json::to_writer(out, self).map_err(io::Error::other)
    }
}

impl DecodeRecord for graph_search_core::dependencies::DependencyRecord {
    fn decode_record(bytes: &[u8]) -> io::Result<Self> {
        serde_json::from_slice(bytes).map_err(io::Error::other)
    }
}

// Extraction facts are already compact graph records; they keep JSON.
impl EncodeRecord for &FileEntry {
    fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()> {
        serde_json::to_writer(out, self).map_err(io::Error::other)
    }
}

impl DecodeRecord for FileEntry {
    fn decode_record(bytes: &[u8]) -> io::Result<Self> {
        serde_json::from_slice(bytes).map_err(io::Error::other)
    }
}

impl EncodeRecord for SourceFileUnits {
    fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()> {
        encode_source(self, out)
    }
}

impl DecodeRecord for SourceFileUnits {
    fn decode_record(bytes: &[u8]) -> io::Result<Self> {
        decode_source(bytes)
    }
}

// Test fixtures store plain JSON values as records.
#[cfg(test)]
macro_rules! json_records {
    ($($encode:ty),* ; $($decode:ty),*) => {
        $(impl EncodeRecord for $encode {
            fn encode_record(&self, out: &mut Vec<u8>) -> io::Result<()> {
                serde_json::to_writer(out, self).map_err(io::Error::other)
            }
        })*
        $(impl DecodeRecord for $decode {
            fn decode_record(bytes: &[u8]) -> io::Result<Self> {
                serde_json::from_slice(bytes).map_err(io::Error::other)
            }
        })*
    };
}
#[cfg(test)]
json_records!(&str, String, serde_json::Value; String, serde_json::Value);

const MAGIC: &[u8; 4] = b"GSR1";

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// ------------------------------------------------------------------ extras

/// Rare file-level fields, borrowed for encoding (no clones on the write path).
#[derive(Serialize)]
struct FileExtrasRef<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    typescript_config: Option<&'a graph_search_types::typescript::TypeScriptConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_manifest: Option<&'a graph_search_types::package::PackageManifest>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    package_scope_incomplete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    package: Option<&'a graph_search_types::package::PackageIdentity>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    documentation_truncated: bool,
    #[serde(skip_serializing_if = "is_zero")]
    embedded_regions: u32,
    #[serde(skip_serializing_if = "is_zero")]
    embedded_unextracted_regions: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    embedded_truncated: bool,
}

impl FileExtrasRef<'_> {
    fn is_empty(&self) -> bool {
        self.typescript_config.is_none()
            && self.package_manifest.is_none()
            && !self.package_scope_incomplete
            && self.package.is_none()
            && !self.documentation_truncated
            && self.embedded_regions == 0
            && self.embedded_unextracted_regions == 0
            && !self.embedded_truncated
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileExtras {
    typescript_config: Option<graph_search_types::typescript::TypeScriptConfig>,
    package_manifest: Option<graph_search_types::package::PackageManifest>,
    package_scope_incomplete: bool,
    package: Option<graph_search_types::package::PackageIdentity>,
    documentation_truncated: bool,
    embedded_regions: u32,
    embedded_unextracted_regions: u32,
    embedded_truncated: bool,
}

/// Rare region fields (Markdown structure, documentation association).
#[derive(Serialize)]
struct UnitExtrasRef<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation: Option<&'a DocumentationComment>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    headings: &'a [MarkdownHeading],
    #[serde(skip_serializing_if = "Option::is_none")]
    fence: Option<&'a MarkdownFence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<&'a MarkdownTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<&'a MarkdownBlock>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    links: &'a [MarkdownLink],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    links_truncated: bool,
}

impl UnitExtrasRef<'_> {
    fn is_empty(&self) -> bool {
        self.documentation.is_none()
            && self.headings.is_empty()
            && self.fence.is_none()
            && self.table.is_none()
            && self.block.is_none()
            && self.links.is_empty()
            && !self.links_truncated
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UnitExtras {
    documentation: Option<DocumentationComment>,
    headings: Vec<MarkdownHeading>,
    fence: Option<MarkdownFence>,
    table: Option<MarkdownTable>,
    block: Option<MarkdownBlock>,
    links: Vec<MarkdownLink>,
    links_truncated: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde predicate signature
fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn kind_tag(kind: SourceUnitKind) -> u8 {
    match kind {
        SourceUnitKind::Code => 0,
        SourceUnitKind::DocumentationComment => 1,
        SourceUnitKind::Markdown => 2,
        SourceUnitKind::MarkdownCodeFence => 3,
        SourceUnitKind::MarkdownFrontmatter => 4,
        SourceUnitKind::MarkdownTable => 5,
        SourceUnitKind::MarkdownParagraph => 6,
        SourceUnitKind::MarkdownListItem => 7,
        SourceUnitKind::MarkdownOpaque => 8,
        SourceUnitKind::Configuration => 9,
        SourceUnitKind::Text => 10,
    }
}

fn kind_of(tag: u8) -> io::Result<SourceUnitKind> {
    Ok(match tag {
        0 => SourceUnitKind::Code,
        1 => SourceUnitKind::DocumentationComment,
        2 => SourceUnitKind::Markdown,
        3 => SourceUnitKind::MarkdownCodeFence,
        4 => SourceUnitKind::MarkdownFrontmatter,
        5 => SourceUnitKind::MarkdownTable,
        6 => SourceUnitKind::MarkdownParagraph,
        7 => SourceUnitKind::MarkdownListItem,
        8 => SourceUnitKind::MarkdownOpaque,
        9 => SourceUnitKind::Configuration,
        10 => SourceUnitKind::Text,
        _ => return Err(invalid("unknown source unit kind")),
    })
}

// ------------------------------------------------------------------ encoding

fn put(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let low = value.to_le_bytes()[0] & 0x7f;
        value = value.wrapping_shr(7);
        if value == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

fn put_len(out: &mut Vec<u8>, len: usize) -> io::Result<()> {
    put(out, u64::try_from(len).map_err(io::Error::other)?);
    Ok(())
}

fn put_zigzag(out: &mut Vec<u8>, value: i64) {
    put(
        out,
        (value.wrapping_shl(1) ^ value.wrapping_shr(63)).cast_unsigned(),
    );
}

fn put_json(out: &mut Vec<u8>, empty: bool, value: &impl Serialize) -> io::Result<()> {
    if empty {
        put(out, 0);
        return Ok(());
    }
    let json = serde_json::to_vec(value).map_err(io::Error::other)?;
    put_len(out, json.len())?;
    out.extend_from_slice(&json);
    Ok(())
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(c.wrapping_sub(b'a').wrapping_add(10)),
        _ => None,
    }
}

fn hex32(hash: &str) -> Option<[u8; 32]> {
    let bytes = hash.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    let (pairs, _) = bytes.as_chunks::<2>();
    for (slot, [high, low]) in out.iter_mut().zip(pairs) {
        *slot = (nibble(*high)?.wrapping_shl(4)) | nibble(*low)?;
    }
    Some(out)
}

/// Sorted, deduplicated strings of one record (ids are positions), plus the
/// final id of every string occurrence in encoding order.
///
/// Each occurrence is hashed once (std `HashMap`: randomly keyed `SipHash`, so
/// repository content cannot force collisions) and given a provisional id in
/// first-seen order; only the distinct keys are sorted. The encoder then reads
/// ids sequentially instead of searching the dictionary for every term.
struct Dictionary<'a> {
    keys: Vec<&'a str>,
    ids: std::vec::IntoIter<usize>,
}

impl<'a> Dictionary<'a> {
    /// Visits strings in exactly the order `encode_source` writes them:
    /// per unit, the owner, then term keys, then identifier keys.
    fn of(file: &'a SourceFileUnits) -> Self {
        let total = file.units.iter().fold(0usize, |total, unit| {
            total
                .saturating_add(unit.terms.len())
                .saturating_add(unit.identifiers.len())
                .saturating_add(usize::from(unit.owner.is_some()))
        });
        let mut provisional: HashMap<&'a str, usize> = HashMap::with_capacity(total);
        let mut distinct: Vec<&'a str> = Vec::with_capacity(total);
        let mut occurrences: Vec<usize> = Vec::with_capacity(total);
        for unit in &file.units {
            let owner = unit.owner.as_ref().map(NodeId::as_str);
            let keys = owner.into_iter().chain(
                unit.terms
                    .keys()
                    .chain(unit.identifiers.keys())
                    .map(String::as_str),
            );
            for key in keys {
                let next = distinct.len();
                let id = *provisional.entry(key).or_insert_with(|| {
                    distinct.push(key);
                    next
                });
                occurrences.push(id);
            }
        }
        let mut order: Vec<usize> = (0..distinct.len()).collect();
        order.sort_unstable_by(|&a, &b| distinct[a].cmp(distinct[b]));
        let mut rank = vec![0usize; distinct.len()];
        for (sorted, &id) in order.iter().enumerate() {
            rank[id] = sorted;
        }
        for id in &mut occurrences {
            *id = rank[*id];
        }
        Self {
            keys: order.iter().map(|&id| distinct[id]).collect(),
            ids: occurrences.into_iter(),
        }
    }

    /// The final id of the next string occurrence in encoding order.
    fn next(&mut self, key: &str) -> io::Result<usize> {
        let id = self
            .ids
            .next()
            .ok_or_else(|| invalid("dictionary traversal out of step"))?;
        // Equal strings compare in full, so release builds check only the
        // length (enough to catch a traversal slip); tests check the bytes.
        debug_assert_eq!(self.keys.get(id), Some(&key));
        if self.keys.get(id).map(|stored| stored.len()) == Some(key.len()) {
            Ok(id)
        } else {
            Err(invalid("dictionary traversal out of step"))
        }
    }
}

fn put_postings(
    out: &mut Vec<u8>,
    map: &BTreeMap<String, Vec<u32>>,
    dict: &mut Dictionary<'_>,
) -> io::Result<()> {
    put_len(out, map.len())?;
    let mut previous = 0usize;
    for (term, lines) in map {
        let id = dict.next(term)?;
        put_len(out, id.wrapping_sub(previous))?;
        previous = id;
        put_len(out, lines.len())?;
        let mut previous_line = 0u32;
        for &line in lines {
            put(out, u64::from(line.wrapping_sub(previous_line)));
            previous_line = line;
        }
    }
    Ok(())
}

fn encode_source(file: &SourceFileUnits, out: &mut Vec<u8>) -> io::Result<()> {
    let mut dict = Dictionary::of(file);
    out.extend_from_slice(MAGIC);
    put_len(out, dict.keys.len())?;
    let mut previous: &[u8] = &[];
    for key in &dict.keys {
        let bytes = key.as_bytes();
        let shared = previous
            .iter()
            .zip(bytes)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = bytes.get(shared..).unwrap_or_default();
        put_len(out, shared)?;
        put_len(out, suffix.len())?;
        out.extend_from_slice(suffix);
        previous = bytes;
    }
    if let Some(raw) = hex32(&file.source_hash) {
        out.push(1);
        out.extend_from_slice(&raw);
    } else {
        out.push(0);
        put_len(out, file.source_hash.len())?;
        out.extend_from_slice(file.source_hash.as_bytes());
    }
    put(out, u64::from(file.version));
    out.push(u8::from(file.truncated));
    let extras = FileExtrasRef {
        typescript_config: file.typescript_config.as_ref(),
        package_manifest: file.package_manifest.as_ref(),
        package_scope_incomplete: file.package_scope_incomplete,
        package: file.package.as_ref(),
        documentation_truncated: file.documentation_truncated,
        embedded_regions: file.embedded_regions,
        embedded_unextracted_regions: file.embedded_unextracted_regions,
        embedded_truncated: file.embedded_truncated,
    };
    put_json(out, extras.is_empty(), &extras)?;
    put_len(out, file.units.len())?;
    for unit in &file.units {
        let span = unit.span;
        put(out, u64::from(span.start_line));
        put_zigzag(
            out,
            i64::from(span.end_line).wrapping_sub(i64::from(span.start_line)),
        );
        put(out, u64::from(span.start_byte));
        put_zigzag(
            out,
            i64::from(span.end_byte).wrapping_sub(i64::from(span.start_byte)),
        );
        out.push(kind_tag(unit.kind));
        match &unit.owner {
            Some(owner) => put_len(out, dict.next(owner.as_str())?.wrapping_add(1))?,
            None => put(out, 0),
        }
        let extras = UnitExtrasRef {
            documentation: unit.documentation.as_ref(),
            headings: &unit.headings,
            fence: unit.fence.as_ref(),
            table: unit.table.as_ref(),
            block: unit.block.as_ref(),
            links: &unit.links,
            links_truncated: unit.links_truncated,
        };
        put_json(out, extras.is_empty(), &extras)?;
        put_postings(out, &unit.terms, &mut dict)?;
        put_postings(out, &unit.identifiers, &mut dict)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ decoding

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> io::Result<u8> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| invalid("truncated record"))?;
        self.at = self.at.wrapping_add(1);
        Ok(byte)
    }

    fn var(&mut self) -> io::Result<u64> {
        let mut value = 0u64;
        for shift in (0..64u32).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f).wrapping_shl(shift);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(invalid("varint overflow"))
    }

    fn var32(&mut self) -> io::Result<u32> {
        u32::try_from(self.var()?).map_err(|_| invalid("u32 overflow"))
    }

    /// A count of elements that each occupy at least one more byte.
    fn count(&mut self) -> io::Result<usize> {
        let n = usize::try_from(self.var()?).map_err(|_| invalid("count overflow"))?;
        if n > self.bytes.len().saturating_sub(self.at) {
            return Err(invalid("count exceeds record"));
        }
        Ok(n)
    }

    /// A dictionary index or prefix length: bounded by the dictionary or the
    /// previous key by its caller, not by the bytes remaining.
    fn index(&mut self) -> io::Result<usize> {
        usize::try_from(self.var()?).map_err(|_| invalid("index overflow"))
    }

    fn zigzag(&mut self) -> io::Result<i64> {
        let v = self.var()?;
        Ok(v.wrapping_shr(1).cast_signed() ^ (v & 1).cast_signed().wrapping_neg())
    }

    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| invalid("truncated record"))?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or_else(|| invalid("truncated record"))?;
        self.at = end;
        Ok(slice)
    }

    fn json<T: for<'de> Deserialize<'de> + Default>(&mut self) -> io::Result<T> {
        let n = self.count()?;
        if n == 0 {
            return Ok(T::default());
        }
        serde_json::from_slice(self.take(n)?).map_err(io::Error::other)
    }

    fn span(&mut self) -> io::Result<Span> {
        let start_line = self.var32()?;
        let end_line = i64::from(start_line)
            .checked_add(self.zigzag()?)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid("span line"))?;
        let start_byte = self.var32()?;
        let end_byte = i64::from(start_byte)
            .checked_add(self.zigzag()?)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| invalid("span byte"))?;
        Ok(Span {
            start_line,
            end_line,
            start_byte,
            end_byte,
        })
    }

    fn postings(&mut self, dict: &[String]) -> io::Result<BTreeMap<String, Vec<u32>>> {
        let n = self.count()?;
        let mut entries = Vec::with_capacity(n);
        let mut id = 0usize;
        for i in 0..n {
            let delta = self.index()?;
            // Ids strictly ascend after the first entry, so maps stay canonical.
            if i > 0 && delta == 0 {
                return Err(invalid("non-ascending posting ids"));
            }
            id = id
                .checked_add(delta)
                .ok_or_else(|| invalid("posting id overflow"))?;
            let term = dict
                .get(id)
                .ok_or_else(|| invalid("posting id out of range"))?;
            let count = self.count()?;
            let mut lines = Vec::with_capacity(count);
            let mut line = 0u32;
            for _ in 0..count {
                line = line.wrapping_add(self.var32()?);
                lines.push(line);
            }
            entries.push((term.clone(), lines));
        }
        Ok(entries.into_iter().collect())
    }
}

fn decode_source(bytes: &[u8]) -> io::Result<SourceFileUnits> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err(invalid("not a GSR1 source record"));
    }
    let n = r.count()?;
    let mut dict: Vec<String> = Vec::with_capacity(n);
    for _ in 0..n {
        let shared = r.index()?;
        let suffix_len = r.count()?;
        let suffix = r.take(suffix_len)?;
        let prefix = match dict.last() {
            Some(previous) => previous
                .as_bytes()
                .get(..shared)
                .ok_or_else(|| invalid("dictionary prefix"))?,
            None if shared == 0 => &[],
            None => return Err(invalid("dictionary prefix")),
        };
        let mut key = Vec::with_capacity(prefix.len().saturating_add(suffix.len()));
        key.extend_from_slice(prefix);
        key.extend_from_slice(suffix);
        let key = String::from_utf8(key).map_err(|_| invalid("dictionary UTF-8"))?;
        if dict.last().is_some_and(|previous| *previous >= key) {
            return Err(invalid("dictionary not strictly sorted"));
        }
        dict.push(key);
    }
    let source_hash = match r.byte()? {
        1 => {
            let raw = r.take(32)?;
            let mut hex = String::with_capacity(64);
            for byte in raw {
                hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
            }
            hex
        }
        0 => {
            let n = r.count()?;
            String::from_utf8(r.take(n)?.to_vec()).map_err(|_| invalid("source hash UTF-8"))?
        }
        _ => return Err(invalid("source hash tag")),
    };
    let version = r.var32()?;
    let flags = r.byte()?;
    if flags > 1 {
        return Err(invalid("record flags"));
    }
    let file: FileExtras = r.json()?;
    let count = r.count()?;
    let mut units = Vec::with_capacity(count);
    for _ in 0..count {
        let span = r.span()?;
        let kind = kind_of(r.byte()?)?;
        let owner = match r.index()? {
            0 => None,
            id => Some(NodeId::new(
                dict.get(id.wrapping_sub(1))
                    .ok_or_else(|| invalid("owner id out of range"))?
                    .clone(),
            )),
        };
        let extras: UnitExtras = r.json()?;
        let terms = r.postings(&dict)?;
        let identifiers = r.postings(&dict)?;
        units.push(SourceUnit {
            span,
            kind,
            owner,
            documentation: extras.documentation,
            identifiers,
            terms,
            headings: extras.headings,
            fence: extras.fence,
            table: extras.table,
            block: extras.block,
            links: extras.links,
            links_truncated: extras.links_truncated,
        });
    }
    if r.at != bytes.len() {
        return Err(invalid("trailing bytes after source record"));
    }
    Ok(SourceFileUnits {
        typescript_config: file.typescript_config,
        package_manifest: file.package_manifest,
        package_scope_incomplete: file.package_scope_incomplete,
        source_hash,
        package: file.package,
        version,
        truncated: flags == 1,
        documentation_truncated: file.documentation_truncated,
        embedded_regions: file.embedded_regions,
        embedded_unextracted_regions: file.embedded_unextracted_regions,
        embedded_truncated: file.embedded_truncated,
        units,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(start: u32, terms: &[(&str, &[u32])], owner: Option<&str>) -> SourceUnit {
        SourceUnit {
            span: Span {
                start_line: start,
                end_line: start.saturating_add(3),
                start_byte: start.saturating_mul(10),
                end_byte: start.saturating_mul(10).saturating_add(25),
            },
            kind: SourceUnitKind::Code,
            owner: owner.map(NodeId::new),
            documentation: None,
            identifiers: BTreeMap::from([(String::from("Worker"), vec![start])]),
            terms: terms
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
                .collect(),
            headings: Vec::new(),
            fence: None,
            table: None,
            block: None,
            links: Vec::new(),
            links_truncated: false,
        }
    }

    fn sample() -> SourceFileUnits {
        let mut markdown = unit(9, &[("zeta", &[9, 9, 12])], None);
        markdown.kind = SourceUnitKind::MarkdownParagraph;
        markdown.headings = vec![MarkdownHeading {
            level: 2,
            span: Span::default(),
            title_span: Span::default(),
        }];
        markdown.links_truncated = true;
        SourceFileUnits {
            source_hash: "ab".repeat(32),
            version: 15,
            truncated: true,
            embedded_regions: 2,
            units: vec![
                unit(
                    1,
                    &[("alpha", &[1, 2]), ("alphabet", &[3]), ("beta", &[])],
                    Some("f:a"),
                ),
                unit(4, &[("alpha", &[u32::MAX, 0])], Some("f:a")),
                markdown,
            ],
            ..SourceFileUnits::default()
        }
    }

    fn roundtrip(file: &SourceFileUnits) -> Vec<u8> {
        let mut bytes = Vec::new();
        file.encode_record(&mut bytes).unwrap();
        assert_eq!(&SourceFileUnits::decode_record(&bytes).unwrap(), file);
        bytes
    }

    #[test]
    fn source_records_round_trip_exactly_including_unsorted_lines_and_extras() {
        roundtrip(&sample());
        roundtrip(&SourceFileUnits::default());
        let mut odd_hash = sample();
        odd_hash.source_hash = String::from("not-hex");
        roundtrip(&odd_hash);
    }

    #[test]
    fn encoding_is_deterministic_and_smaller_than_json() {
        let file = sample();
        let bytes = roundtrip(&file);
        assert_eq!(bytes, roundtrip(&file.clone()));
        assert!(bytes.len() < serde_json::to_vec(&file).unwrap().len());
    }

    #[test]
    fn every_truncation_and_trailing_byte_is_rejected() {
        let bytes = roundtrip(&sample());
        for len in 0..bytes.len() {
            assert!(
                SourceFileUnits::decode_record(&bytes[..len]).is_err(),
                "prefix {len}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(SourceFileUnits::decode_record(&trailing).is_err());
        let mut magic = bytes;
        magic[0] = b'X';
        assert!(SourceFileUnits::decode_record(&magic).is_err());
    }

    #[test]
    fn late_references_to_large_dictionaries_decode() {
        // Owner ids, id deltas and prefix lengths are indexes, not counts: they
        // may exceed the bytes left in the record (the last region here).
        let many: Vec<(String, Vec<u32>)> =
            (0..300).map(|i| (format!("term{i:03}"), vec![1])).collect();
        let mut first = unit(1, &[], None);
        first.terms = many.into_iter().collect();
        let mut last = unit(5, &[], Some("zzz-owner"));
        last.identifiers.clear();
        let file = SourceFileUnits {
            source_hash: "cd".repeat(32),
            units: vec![first, last],
            ..SourceFileUnits::default()
        };
        roundtrip(&file);
    }

    #[test]
    fn absurd_counts_fail_before_allocating() {
        let mut bytes = MAGIC.to_vec();
        put(&mut bytes, u64::MAX >> 1);
        assert!(SourceFileUnits::decode_record(&bytes).is_err());
    }
}
