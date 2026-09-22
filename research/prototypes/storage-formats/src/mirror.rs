//! A zero-copy (`rkyv`) mirror of one `SourceFileUnits` record, used only to
//! measure the "skip decoding entirely" ceiling. The product types carry no
//! `rkyv` derives, so this mirrors them: inline strings, fixed-width `u32` lines,
//! no dictionary, and the rare Markdown/documentation fields as JSON bytes.

use graph_search_types::source::SourceFileUnits;

/// One record in the zero-copy mirror.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RFile {
    pub source_hash: String,
    pub version: u32,
    pub truncated: bool,
    pub units: Vec<RUnit>,
}

/// One region in the zero-copy mirror.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct RUnit {
    pub span: [u32; 4],
    pub kind: String,
    pub owner: Option<String>,
    pub extras: Vec<u8>,
    pub terms: Vec<(String, Vec<u32>)>,
    pub identifiers: Vec<(String, Vec<u32>)>,
}

/// Builds the mirror of one record.
pub fn mirror(file: &SourceFileUnits) -> RFile {
    RFile {
        source_hash: file.source_hash.clone(),
        version: file.version,
        truncated: file.truncated,
        units: file
            .units
            .iter()
            .map(|u| {
                let extras = serde_json::json!({
                    "documentation": u.documentation,
                    "headings": u.headings,
                    "fence": u.fence,
                    "table": u.table,
                    "block": u.block,
                    "links": u.links,
                    "links_truncated": u.links_truncated,
                });
                RUnit {
                    span: [u.span.start_line, u.span.end_line, u.span.start_byte, u.span.end_byte],
                    kind: serde_json::to_string(&u.kind).unwrap_or_default(),
                    owner: u.owner.as_ref().map(|o| o.as_str().to_owned()),
                    extras: serde_json::to_vec(&extras).unwrap_or_default(),
                    terms: u.terms.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    identifiers: u
                        .identifiers
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                }
            })
            .collect(),
    }
}
