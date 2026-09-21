//! Coordinate-preserving composition of embedded JS/TS parser facts.
//!
//! The caller identifies a script's byte range and its separate binding domain.
//! This layer does not discover framework syntax, make template edges, or infer
//! visibility between script domains. Framework adapters must supply those rules.

use graph_search_core::extraction::Extraction;
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::{Language, Span};
use std::{ops::Range, path::Path};

/// Extracts one JavaScript or TypeScript region in original file coordinates.
///
/// `domain` is a stable, file-local identity (for example `instance` or `module`),
/// not a source qualifier. All declaration keys and key references are namespaced;
/// authored names, signatures, module specifiers and raw spelling are preserved.
/// Scope and binding ordinals remain local until [`Extraction::merge`] offsets them.
/// A merge alone does not establish cross-domain visibility or one ESM surface.
///
/// Only ordinary JS/TS grammars are selected; the containing filename cannot
/// accidentally select TSX. Byte boundaries must delimit UTF-8 and remain within
/// `file.text`. No preprocessor or generated source is executed.
///
/// # Errors
/// Rejects unsupported languages, invalid domains/ranges, or unrepresentable
/// coordinates. Domains contain 1–128 ASCII letters, digits, `_`, or `-`.
pub fn script(
    file: &SourceFile<'_>,
    range: Range<usize>,
    language: Language,
    domain: &str,
) -> Result<Extraction, ParseError> {
    if domain.is_empty()
        || domain.len() > 128
        || !domain
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(error("invalid binding domain"));
    }
    let source = file
        .text
        .get(range.clone())
        .ok_or_else(|| error("invalid UTF-8 script range"))?;
    // Even a parser-saturated local coordinate must never become a plausible
    // original coordinate. Reject files outside the representation before parse.
    let end = u32::try_from(range.end).map_err(|_| error("byte coordinate overflow"))?;
    let start = u32::try_from(range.start).map_err(|_| error("byte coordinate overflow"))?;
    let lines = file.text[..range.start]
        .split('\n')
        .count()
        .saturating_sub(1);
    let lines = u32::try_from(lines).map_err(|_| error("line coordinate overflow"))?;
    let parsed = SourceFile {
        path: Path::new("embedded.ts"),
        text: source,
    };
    let mut extraction = match language {
        Language::JavaScript => crate::JavaScriptExtractor.extract(&parsed)?,
        Language::TypeScript => crate::TypeScriptExtractor.extract(&parsed)?,
        _ => return Err(error("unsupported script language")),
    };
    relocate(&mut extraction, start, end, lines, domain)?;
    Ok(extraction)
}

fn error(reason: &str) -> ParseError {
    ParseError::new(format!("embedded script: {reason}"))
}

struct Coordinates {
    start: u32,
    end: u32,
    lines: u32,
}

impl Coordinates {
    fn byte(&self, value: u32) -> Result<u32, ParseError> {
        value
            .checked_add(self.start)
            .filter(|&value| value <= self.end)
            .ok_or_else(|| error("byte outside script range"))
    }

    fn line(&self, value: u32) -> Result<u32, ParseError> {
        value
            .checked_add(self.lines)
            .filter(|_| value > 0)
            .ok_or_else(|| error("invalid line coordinate"))
    }

    fn span(&self, span: &mut Span) -> Result<(), ParseError> {
        if span.start_byte > span.end_byte || span.start_line > span.end_line {
            return Err(error("reversed source span"));
        }
        *span = Span::new(
            self.line(span.start_line)?,
            self.line(span.end_line)?,
            self.byte(span.start_byte)?,
            self.byte(span.end_byte)?,
        );
        Ok(())
    }
}

fn prefix(value: &mut String, domain: &str) {
    *value = format!("embedded:{domain}>{value}");
}

// Every coordinate/key-bearing field is translated, including lexical bounds
// consumed by the core resolver rather than only the displayed symbol spans.
fn relocate(
    extraction: &mut Extraction,
    start: u32,
    end: u32,
    lines: u32,
    domain: &str,
) -> Result<(), ParseError> {
    let coordinates = Coordinates { start, end, lines };
    for symbol in &mut extraction.symbols {
        coordinates.span(&mut symbol.span)?;
        prefix(&mut symbol.key, domain);
        if let Some(parent) = &mut symbol.parent_key {
            prefix(parent, domain);
        }
        if let Some(key) = symbol.attributes.get_mut("lexical_key") {
            prefix(key, domain);
        }
        for name in ["lexical_start", "lexical_end"] {
            if let Some(value) = symbol.attributes.get_mut(name) {
                *value = coordinates
                    .byte(value.parse().map_err(|_| error("invalid lexical bound"))?)?
                    .to_string();
            }
        }
        symbol
            .attributes
            .insert("embedded_domain".into(), domain.into());
    }
    for reference in &mut extraction.references {
        if let Some(span) = &mut reference.span {
            coordinates.span(span)?;
        }
        reference.line = coordinates.line(reference.line)?;
        for key in [&mut reference.from_key, &mut reference.lexical_target]
            .into_iter()
            .flatten()
        {
            prefix(key, domain);
        }
    }
    for scope in &mut extraction.scopes {
        coordinates.span(&mut scope.span)?;
    }
    for binding in &mut extraction.bindings {
        coordinates.span(&mut binding.span)?;
        binding.visible_from = coordinates.byte(binding.visible_from)?;
        // Zero denotes a hoisted binding, not a source offset.
        if binding.initialized_from != 0 {
            binding.initialized_from = coordinates.byte(binding.initialized_from)?;
        }
        if let Some(key) = &mut binding.target_key {
            prefix(key, domain);
        }
    }
    for documentation in &mut extraction.doc_comments {
        coordinates.span(&mut documentation.span)?;
        if let Some(key) = &mut documentation.owner_key {
            prefix(key, domain);
        }
    }
    if let Some(module) = &mut extraction.js_module {
        for import in &mut module.imports {
            coordinates.span(&mut import.span)?;
        }
        for export in &mut module.exports {
            coordinates.span(&mut export.span)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_coordinates_before_returning_facts() {
        let file = SourceFile {
            path: Path::new("a.svelte"),
            text: "é\nfunction f() {}",
        };
        for range in [1..2, 0..100, Range { start: 5, end: 2 }] {
            assert!(script(&file, range, Language::JavaScript, "instance").is_err());
        }
        for domain in ["", "module>function:f", "é", "a.b"] {
            assert!(script(&file, 3..file.text.len(), Language::JavaScript, domain).is_err());
        }
        assert!(script(&file, 3..file.text.len(), Language::Rust, "module").is_err());
        let coordinates = Coordinates {
            start: u32::MAX.saturating_sub(1),
            end: u32::MAX,
            lines: u32::MAX,
        };
        assert!(coordinates.byte(2).is_err());
        assert!(coordinates.line(1).is_err());
        assert!(coordinates.span(&mut Span::new(1, 1, 2, 1)).is_err());
        assert!(coordinates.span(&mut Span::new(0, 0, 0, 0)).is_err());
    }
}
