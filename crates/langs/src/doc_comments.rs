//! Source-owned documentation facts from existing syntax trees, never raw-text guesses.
use graph_search_types::extraction::{DocCommentFact, Extraction, SymbolFact};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use tree_sitter::Node;

fn comment(node: Node<'_>) -> bool {
    matches!(node.kind(), "comment" | "line_comment" | "block_comment")
}

fn documentation(text: &str, rust: bool) -> Option<bool> {
    if rust && (text.starts_with("//!") || text.starts_with("/*!")) {
        Some(true)
    } else if (rust && text.starts_with("///") && !text.starts_with("////"))
        || (text.starts_with("/**") && !text.starts_with("/***") && !text.starts_with("/**/"))
    {
        Some(false)
    } else {
        None
    }
}

type Symbols<'a> = BTreeMap<(usize, Reverse<usize>), Option<&'a SymbolFact>>;

fn owner(
    node: Node<'_>,
    inner: bool,
    symbols: &Symbols<'_>,
    cache: &mut BTreeMap<usize, Option<String>>,
) -> Option<String> {
    if inner {
        let mut parent = node.parent();
        while let Some(container) = parent {
            if let Some(fact) =
                symbols.get(&(container.start_byte(), Reverse(container.end_byte())))
            {
                return fact.map(|fact| fact.key.clone());
            }
            parent = container.parent();
        }
        return None;
    }
    let mut current = node;
    let mut visited = Vec::new();
    let selected = loop {
        if let Some(selected) = cache.get(&current.id()) {
            break selected.clone();
        }
        visited.push(current.id());
        let Some(candidate) = current.next_named_sibling() else {
            break None;
        };
        if comment(candidate) || candidate.kind() == "attribute_item" {
            current = candidate;
            continue;
        }
        // Restrict wrappers to declarations, never a callback in an unrelated
        // expression statement. Ambiguous multi-declaration wrappers stay unowned.
        let direct = symbols.get(&(candidate.start_byte(), Reverse(candidate.end_byte())));
        if let Some(fact) = direct {
            break fact.map(|fact| fact.key.clone());
        }
        if !matches!(
            candidate.kind(),
            "export_statement" | "lexical_declaration" | "variable_declaration"
        ) {
            break None;
        }
        let mut contained = symbols
            .range((candidate.start_byte(), Reverse(usize::MAX))..)
            .take_while(|((start, _), _)| *start < candidate.end_byte())
            .filter(|(_, fact)| {
                fact.is_none_or(|fact| fact.span.end_byte as usize <= candidate.end_byte())
            });
        let Some((_, first)) = contained.next() else {
            break None;
        };
        let Some(first) = first else { break None };
        if contained
            .any(|(_, fact)| fact.is_none_or(|fact| fact.span.start_byte >= first.span.end_byte))
        {
            break None;
        }
        break Some(first.key.clone());
    };
    for id in visited {
        cache.insert(id, selected.clone());
    }
    selected
}

pub(crate) fn enrich(root: Node<'_>, source: &str, rust: bool, extraction: &mut Extraction) {
    let mut key_counts = BTreeMap::new();
    for fact in &extraction.symbols {
        let count = key_counts.entry(fact.key.as_str()).or_insert(0usize);
        *count = count.saturating_add(1);
    }
    let mut symbols: Symbols<'_> = BTreeMap::new();
    for fact in &extraction.symbols {
        symbols
            .entry((
                fact.span.start_byte as usize,
                Reverse(fact.span.end_byte as usize),
            ))
            .and_modify(|entry| *entry = None)
            .or_insert_with(|| (key_counts[fact.key.as_str()] == 1).then_some(fact));
    }
    let mut cache = BTreeMap::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        let is_comment = comment(node);
        if is_comment {
            if let Some(inner) = documentation(crate::walk::text(node, source), rust) {
                if extraction.doc_comments.len()
                    == graph_search_types::limits::MAX_DOC_COMMENTS_PER_FILE
                {
                    extraction.doc_comments_truncated = true;
                    return;
                }
                extraction.doc_comments.push(DocCommentFact {
                    span: crate::walk::span_of(node),
                    owner_key: owner(node, inner, &symbols, &mut cache),
                    inner,
                });
            }
        } else if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use graph_search_core::ports::{LanguageExtractor, SourceFile};
    use graph_search_types::extraction::Extraction;
    use std::path::Path;

    fn extract(text: &str, rust: bool) -> Extraction {
        let file = SourceFile {
            path: Path::new(if rust { "a.rs" } else { "a.ts" }),
            text,
        };
        if rust {
            crate::RustExtractor.extract(&file)
        } else {
            crate::TypeScriptExtractor.extract(&file)
        }
        .unwrap()
    }

    #[test]
    fn rust_docs_have_raw_spans_and_conservative_owners_without_string_false_positives() {
        let text = "//! file docs\r\n/// café outer\r\n#[inline]\r\nfn first() {}\r\nmod nested {\r\n//! module docs\r\n/** second docs */\r\nfn second() { let value = r#\"/// fake\"#; }\r\n}\r\n//// ordinary\r\n/*** ordinary */\r\nconst TEXT: &str = \"/*! fake */\";\r\n";
        let facts = extract(text, true);
        assert_eq!(facts.doc_comments.len(), 4);
        let owners: Vec<_> = facts
            .doc_comments
            .iter()
            .map(|c| {
                c.owner_key.as_ref().map(|key| {
                    facts
                        .symbols
                        .iter()
                        .find(|fact| &fact.key == key)
                        .unwrap()
                        .qualified_name
                        .as_str()
                })
            })
            .collect();
        assert_eq!(
            owners,
            [None, Some("first"), Some("nested"), Some("nested::second")]
        );
        for (fact, expected) in facts.doc_comments.iter().zip([
            "//! file docs",
            "/// café outer",
            "//! module docs",
            "/** second docs */",
        ]) {
            assert_eq!(
                text[fact.span.start_byte as usize..fact.span.end_byte as usize].trim_end(),
                expected
            );
        }
        assert!(facts.doc_comments[0].inner && facts.doc_comments[2].inner);
        assert!(!facts.doc_comments_truncated);
    }

    #[test]
    fn js_docs_attach_only_to_the_next_syntax_neighbor_and_ignore_string_contents() {
        let text = "/** exported café */\r\nexport function first() {}\r\n/** separated */\r\nconsole.log('x');\r\nfunction later() {}\r\nconst text = '/** fake */';\r\nconst template = `/** fake template */`;\r\n/// not JS documentation\r\n/** multi */ const left = () => {}, right = () => {};\r\n/** destructured */ const {a,b} = value;\r\n";
        let facts = extract(text, false);
        assert_eq!(facts.doc_comments.len(), 4);
        assert_eq!(
            facts.doc_comments[0].owner_key.as_ref(),
            facts
                .symbols
                .iter()
                .find(|fact| fact.name == "first")
                .map(|fact| &fact.key)
        );
        assert!(
            facts.doc_comments[1..]
                .iter()
                .all(|fact| fact.owner_key.is_none())
        );
        let javascript = crate::JavaScriptExtractor
            .extract(&SourceFile {
                path: Path::new("a.js"),
                text,
            })
            .unwrap();
        assert_eq!(javascript.doc_comments, facts.doc_comments);
        assert_eq!(
            &text[facts.doc_comments[0].span.start_byte as usize
                ..facts.doc_comments[0].span.end_byte as usize],
            "/** exported café */"
        );
    }

    #[test]
    fn repeated_declaration_keys_cannot_select_a_different_occurrence() {
        let facts = extract(
            "/** first */ function repeated() {}\n/** second */ function repeated() {}\n",
            false,
        );
        assert_eq!(facts.doc_comments.len(), 2);
        assert!(
            facts
                .doc_comments
                .iter()
                .all(|fact| fact.owner_key.is_none())
        );
    }

    #[test]
    fn comment_cap_requires_an_actual_omitted_documentation_node() {
        let limit = graph_search_types::limits::MAX_DOC_COMMENTS_PER_FILE;
        let text = "/// docs\n".repeat(limit);
        let comments = |source: &str| {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_rust::LANGUAGE.into())
                .unwrap();
            let tree = parser.parse(source, None).unwrap();
            let mut facts = Extraction::default();
            super::enrich(tree.root_node(), source, true, &mut facts);
            facts
        };
        let complete = comments(&text);
        assert_eq!(complete.doc_comments.len(), limit);
        assert!(!complete.doc_comments_truncated);
        let partial = comments(&(text + "/// extra\n"));
        assert_eq!(partial.doc_comments.len(), limit);
        assert!(partial.doc_comments_truncated);
    }
}
