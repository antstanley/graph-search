//! Delimiter-terminated top-level HTML blocks (`CommonMark` block types 1–5).
//! This only shields source boundaries; it does not parse or render HTML.

#[derive(Clone, Copy)]
pub(super) enum End {
    Raw,
    Marker(&'static str),
}

impl End {
    pub(super) fn reached(self, line: &str) -> bool {
        match self {
            Self::Marker(marker) => line.contains(marker),
            Self::Raw => ["</pre>", "</script>", "</style>", "</textarea>"]
                .iter()
                .any(|tag| {
                    line.as_bytes()
                        .windows(tag.len())
                        .any(|part| part.eq_ignore_ascii_case(tag.as_bytes()))
                }),
        }
    }
}

pub(super) fn opener(line: &str) -> Option<End> {
    let line = super::unindent(line)?;
    for (prefix, end) in [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")] {
        if line.starts_with(prefix) {
            return Some(End::Marker(end));
        }
    }
    if line
        .strip_prefix("<!")
        .and_then(|tail| tail.as_bytes().first())
        .is_some_and(u8::is_ascii_alphabetic)
    {
        return Some(End::Marker(">"));
    }
    for tag in ["<pre", "<script", "<style", "<textarea"] {
        if line
            .as_bytes()
            .get(..tag.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag.as_bytes()))
            && line
                .as_bytes()
                .get(tag.len())
                .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'>'))
        {
            return Some(End::Raw);
        }
    }
    None
}
