//! Native top-level pipe tables; every coordinate addresses authored source.
use super::{heading, opener, unindent, unsupported_block};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Table {
    pub header_end: usize,
    pub delimiter_end: usize,
    pub columns: u32,
}

fn pipes(line: &str) -> impl Iterator<Item = usize> + '_ {
    let mut escaped = false;
    line.bytes().enumerate().filter_map(move |(at, byte)| {
        if byte == b'\\' {
            escaped = !escaped;
            None
        } else {
            let pipe = byte == b'|' && !escaped;
            escaped = false;
            pipe.then_some(at)
        }
    })
}

fn cells(line: &str) -> impl Iterator<Item = &str> {
    let mut line = line.trim_matches([' ', '\t']);
    if let Some(rest) = line.strip_prefix('|') {
        line = rest;
    }
    if line.ends_with('|') && pipes(line).last() == Some(line.len().saturating_sub(1)) {
        line = &line[..line.len().saturating_sub(1)];
    }
    let mut escaped = false;
    line.split(move |ch| {
        if ch == '\\' {
            escaped = !escaped;
            false
        } else {
            let split = ch == '|' && !escaped;
            escaped = false;
            split
        }
    })
    .map(|cell| cell.trim_matches([' ', '\t']))
}

fn delimiter(cell: &str) -> bool {
    let cell = cell.strip_prefix(':').unwrap_or(cell);
    let cell = cell.strip_suffix(':').unwrap_or(cell);
    !cell.is_empty() && cell.bytes().all(|byte| byte == b'-')
}

fn line_text(raw: &str) -> &str {
    raw.trim_end_matches(['\r', '\n'])
}

fn block_start(line: &str) -> bool {
    heading(line) || opener(line).is_some() || unsupported_block(line)
}

/// Recognizes a table header followed immediately by a compatible delimiter.
/// Container/HTML-like blocks are left to their declared fallback behavior.
pub(super) fn scan(text: &str, start: usize) -> Option<(usize, Table)> {
    let mut lines = text[start..].split_inclusive('\n');
    let header = lines.next()?;
    let separator = lines.next()?;
    let header_text = line_text(header);
    let separator_text = line_text(separator);
    if header_text.trim_matches([' ', '\t']).is_empty()
        || block_start(header_text)
        || unindent(separator_text).is_none()
        || (pipes(header_text).next().is_none() && pipes(separator_text).next().is_none())
    {
        return None;
    }
    let columns = cells(header_text).count();
    if columns != cells(separator_text).count() || !cells(separator_text).all(delimiter) {
        return None;
    }
    let header_end = start.saturating_add(header.len());
    let delimiter_end = header_end.saturating_add(separator.len());
    let mut end = delimiter_end;
    for raw in lines {
        let line = line_text(raw);
        if line.trim_matches([' ', '\t']).is_empty() || block_start(line) {
            break;
        }
        end = end.saturating_add(raw.len());
    }
    Some((
        end,
        Table {
            header_end,
            delimiter_end,
            columns: u32::try_from(columns).ok()?,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaping_and_edge_pipes_preserve_column_identity() {
        assert_eq!(
            cells(r"| a\|b | café | ").collect::<Vec<_>>(),
            [r"a\|b", "café"]
        );
        assert_eq!(cells(r"a\\|b").collect::<Vec<_>>(), [r"a\\", "b"]);
        assert_eq!(
            cells(r"| `\|` | b\|").collect::<Vec<_>>(),
            [r"`\|`", r"b\|"]
        );
        assert_eq!(cells("|||").collect::<Vec<_>>(), ["", ""]);
    }
    #[test]
    fn table_extent_accepts_raw_uneven_rows_but_stops_at_new_blocks() {
        let text = "| café | value |\r\n| :- | -: |\r\none\r\na | b | extra\r\n> outside\r\n";
        let (end, table) = scan(text, 0).unwrap();
        assert_eq!(table.columns, 2);
        assert_eq!(&text[end..], "> outside\r\n");
        assert_eq!(&text[..table.header_end], "| café | value |\r\n");
        for text in [
            "a | b\n|---|\n",
            "a | b\n:-: | x\n",
            "a\\|b\n---\n",
            "    a | b\n- | -\n",
            "# a | b\n- | -\n",
        ] {
            assert!(scan(text, 0).is_none(), "{text:?}");
        }
        assert!(scan("name\n|---|\n", 0).is_some());
        assert!(scan("|name|\n---\n", 0).is_some());
    }
}
