//! Conservative, explicit task-prompt cleanup. Never rewrites literal routes.
use graph_search_types::{ExploreMode, QueryPolicy, RetrievalOptions};

const SUFFIXES: &[&str] = &[
    "Cite the relevant source and explain the execution path.",
    "Distinguish observed behavior from assumptions.",
    "Propose regression tests; do not edit the repository.",
    "Do not edit the repository.",
    "Do not modify files.",
    "Include file paths and line numbers.",
];

/// Returns an original prefix and omitted suffixes in source order. Callers must
/// enforce the input byte bound before this linear, fixed-vocabulary operation.
pub(crate) fn apply<'a>(query: &'a str, options: &RetrievalOptions) -> (&'a str, Vec<String>) {
    if options.query_policy != QueryPolicy::Task
        || !matches!(options.mode, ExploreMode::Auto | ExploreMode::Terms)
    {
        return (query, Vec::new());
    }
    let protected = quoted_prefixes(query);
    let mut effective = query;
    let mut omitted = Vec::new();
    loop {
        let trimmed = effective.trim_end();
        let candidate = SUFFIXES.iter().find_map(|suffix| {
            let start = trimmed.len().checked_sub(suffix.len())?;
            let (prefix, tail) = (trimmed.get(..start)?, trimmed.get(start..)?);
            let content = prefix.trim_end();
            let boundary = prefix.ends_with(char::is_whitespace)
                && (content.ends_with(['.', '?', '!']) || prefix[content.len()..].contains('\n'));
            (tail.eq_ignore_ascii_case(suffix)
                && !content.is_empty()
                && boundary
                && !protected[prefix.len()])
            .then_some((prefix, tail))
        });
        let Some((prefix, suffix)) = candidate else {
            break;
        };
        omitted.push(suffix.to_owned());
        effective = prefix.trim_end();
    }
    omitted.reverse();
    (effective, omitted)
}

// Fail closed for unfinished quoted strings/code. Apostrophes inside words are
// ordinary text. This is a query protection rule, not Markdown inline parsing.
fn quoted_prefixes(text: &str) -> Vec<bool> {
    let mut states = vec![false; text.len().saturating_add(1)];
    let mut quote = None;
    let mut ticks = 0usize;
    let mut escaped = false;
    let mut previous = None;
    let mut chars = text.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        let mut end = offset.saturating_add(ch.len_utf8());
        if escaped {
            escaped = false;
        } else if ch == '`' && quote.is_none() {
            let mut count = 1usize;
            while chars.peek().is_some_and(|(_, next)| *next == '`') {
                if let Some((at, next)) = chars.next() {
                    end = at.saturating_add(next.len_utf8());
                    count = count.saturating_add(1);
                }
            }
            if ticks == 0 {
                ticks = count;
            } else if ticks == count {
                ticks = 0;
            }
        } else if ticks > 0 {
            // Code-span contents cannot open string quotes or escape their closer.
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none()
            && (ch == '"' || (ch == '\'' && !previous.is_some_and(char::is_alphanumeric)))
        {
            quote = Some(ch);
        }
        states[end] = quote.is_some() || ticks > 0;
        previous = Some(ch);
    }
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_standalone_terminal_instructions_and_records_original_text() {
        let options = RetrievalOptions {
            query_policy: QueryPolicy::Task,
            ..Default::default()
        };
        let input = "Why does café fail with `--refresh`? Cite the relevant source and explain the execution path. DO NOT MODIFY FILES. \n";
        let (effective, omitted) = apply(input, &options);
        assert_eq!(effective, "Why does café fail with `--refresh`?");
        assert_eq!(omitted, [SUFFIXES[0], "DO NOT MODIFY FILES."]);
        for input in [
            "Do not modify files.",
            "Explain Do not modify files.",
            "The error says: `Do not modify files.",
            "The error says: ``quoted ` data. Do not modify files.",
            "The error says: ```quoted `` data. Do not modify files.",
            "The error says: \"Do not modify files.",
            "The error says: 'Do not modify files.",
            "Cite the relevant source and explain the execution path. Why does this fail?",
            "Why fail? Do not modify files!",
        ] {
            assert_eq!(apply(input, &options), (input, Vec::new()));
        }
        assert_eq!(
            apply("Why doesn't it work? Do not modify files.", &options).0,
            "Why doesn't it work?"
        );
        assert_eq!(
            apply("What is ``quoted ` data``? Do not modify files.", &options).0,
            "What is ``quoted ` data``?"
        );
        assert_eq!(
            apply("error code\nDo not modify files.", &options).0,
            "error code"
        );
    }

    #[test]
    fn navigation_positions_and_default_policy_preserve_every_byte() {
        let input = "needle. Do not modify files.";
        assert_eq!(
            apply(input, &RetrievalOptions::default()),
            (input, Vec::new())
        );
        for mode in [
            ExploreMode::ExactId,
            ExploreMode::ExactName,
            ExploreMode::NamePrefix,
            ExploreMode::PathGlob,
            ExploreMode::Phrase,
            ExploreMode::Near,
        ] {
            let options = RetrievalOptions {
                mode,
                query_policy: QueryPolicy::Task,
                ..Default::default()
            };
            assert_eq!(apply(input, &options), (input, Vec::new()));
        }
    }
}
