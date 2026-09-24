//! Native Markdown boundaries reach source evidence without rewriting source.
#![allow(clippy::unwrap_used)]
use graph_search::{Index, OpenOptions};
use graph_search_types::{ExploreQuery, source::SourceUnitKind};

#[test]
fn link_metadata_caps_are_visible_after_reopen_without_hiding_body_text() {
    let root = tempfile::tempdir().unwrap();
    let source = format!(
        "{}[lastBodyNeedle](destination)\n",
        "[label](url)\n\n".repeat(graph_search_types::limits::MAX_MARKDOWN_LINKS_PER_FILE)
    );
    std::fs::write(root.path().join("guide.md"), source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    let report = index.reindex().unwrap();
    assert_eq!(report.coverage.source_link_truncated_files, 1);
    assert_eq!(report.coverage.source_unit_truncated_files, 0);
    drop(index);
    let index = Index::open(options()).unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("lastBodyNeedle"))
        .unwrap();
    assert_eq!(result.context.coverage.source_link_truncated_files, 1);
    assert!(result.items.iter().any(|item| item.node.path == "guide.md"
        && item.snippet.as_ref().is_some_and(|snippet| {
            snippet
                .lines
                .iter()
                .any(|line| line.contains("lastBodyNeedle"))
        })));
}

#[test]
fn link_fields_persist_and_incremental_edits_match_fresh_extraction() {
    use graph_search_core::ports::GraphStore;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("guide.md");
    let source =
        "# Guide\r\n[linkNeedle](docs/café.md \"Authored title\")\r\n\r\n![image](img.svg)\r\n";
    std::fs::write(&path, source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    let store_dir = index.store_dir().to_path_buf();
    drop(index);
    let read = || {
        let store = graph_search_engine::NativeStore::open(
            &store_dir,
            &graph_search_engine::StoreOptions::default(),
        )
        .unwrap();
        store.snapshot().unwrap().source_files().unwrap()["guide.md"].clone()
    };
    let before = read();
    let links: Vec<_> = before.units.iter().flat_map(|unit| &unit.links).collect();
    assert_eq!(links.len(), 2);
    assert_eq!(
        &source[links[0].destination_span.start_byte as usize
            ..links[0].destination_span.end_byte as usize],
        "docs/café.md"
    );
    let index = Index::open(options()).unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("linkNeedle"))
        .unwrap();
    assert!(result.items.iter().any(|item| item.node.path == "guide.md"
        && item.snippet.as_ref().is_some_and(|snippet| {
            snippet
                .lines
                .iter()
                .any(|line| line.contains("docs/café.md"))
        })));
    let changed = source.replace("docs/café.md", "next/élève.md");
    std::fs::write(&path, &changed).unwrap();
    index.sync().unwrap();
    let after = read();
    assert_ne!(before.source_hash, after.source_hash);
    let expected = graph_search_core::units::extract(
        "guide.md",
        &changed,
        &after.source_hash,
        graph_search_types::Language::Unknown,
        &[],
    );
    assert_eq!(after, expected);
    drop(index);
    assert_eq!(read(), after);
}

#[test]
fn paragraph_and_list_evidence_survive_reopen_and_live_edits() {
    let root = tempfile::tempdir().unwrap();
    let source = "# Guide\r\n\r\nparagraphNeedle café\r\n\r\nUnrelated paragraph.\r\n\r\n1. listNeedle café\r\n   continuation\r\n   ## Nested heading\r\n2. other item\r\n";
    std::fs::write(root.path().join("guide.md"), source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        reconcile: graph_search::Reconcile::Never,
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    for (query, kind, expected) in [
        (
            "paragraphNeedle",
            SourceUnitKind::MarkdownParagraph,
            "paragraphNeedle café\r\n\r\n",
        ),
        (
            "listNeedle",
            SourceUnitKind::MarkdownListItem,
            "1. listNeedle café\r\n   continuation\r\n   ## Nested heading\r\n",
        ),
    ] {
        let output = index.search().explore(&ExploreQuery::new(query)).unwrap();
        let evidence = output
            .items
            .iter()
            .find(|item| item.node.path == "guide.md")
            .unwrap()
            .evidence
            .as_ref()
            .unwrap();
        assert_eq!(evidence.kind, kind);
        assert_eq!(
            &source[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
            expected
        );
        assert!(!evidence.live);
    }
    let changed = source.replace("1. listNeedle", "- updatedListNeedle");
    std::fs::write(root.path().join("guide.md"), &changed).unwrap();
    let output = index
        .search()
        .explore(&ExploreQuery::new("updatedListNeedle"))
        .unwrap();
    let evidence = output
        .items
        .iter()
        .find(|item| item.node.path == "guide.md")
        .unwrap()
        .evidence
        .as_ref()
        .unwrap();
    assert_eq!(evidence.kind, SourceUnitKind::MarkdownListItem);
    assert!(evidence.live);
    assert!(
        changed[evidence.span.start_byte as usize..evidence.span.end_byte as usize]
            .starts_with("- updatedListNeedle")
    );
}

#[test]
fn fenced_examples_keep_their_delimiters_in_bounded_body_context() {
    let root = tempfile::tempdir().unwrap();
    let source = "# Guide\r\nIntroduction outside.\r\n````rust\r\n# embedded heading\r\nlet café = needleUnique();\r\n```\r\n````\r\n## Next\r\nUnrelated paragraph.\r\n";
    std::fs::write(root.path().join("guide.md"), source).unwrap();
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    })
    .unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("needleUnique"))
        .unwrap();
    let item = result
        .items
        .iter()
        .find(|item| item.node.path == "guide.md")
        .unwrap();
    let evidence = item.evidence.as_ref().unwrap();
    assert_eq!(evidence.kind, SourceUnitKind::MarkdownCodeFence);
    assert_eq!(evidence.span.start_line, 3);
    assert_eq!(evidence.span.end_line, 7);
    assert_eq!(
        &source[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
        "````rust\r\n# embedded heading\r\nlet café = needleUnique();\r\n```\r\n````\r\n"
    );
    let lines: std::collections::BTreeMap<_, _> = item
        .snippet
        .iter()
        .chain(item.excerpts.iter().map(|excerpt| &excerpt.snippet))
        .flat_map(|snippet| {
            snippet.lines.iter().enumerate().map(move |(offset, line)| {
                (
                    snippet
                        .start_line
                        .saturating_add(u32::try_from(offset).unwrap()),
                    line.as_str(),
                )
            })
        })
        .collect();
    assert_eq!(lines.get(&3), Some(&"````rust"));
    assert_eq!(lines.get(&7), Some(&"````"));
    assert_eq!(lines.get(&5), Some(&"let café = needleUnique();"));
}

#[test]
fn long_fence_fragments_stay_inside_their_original_block() {
    let mut source = String::from("# Heading\nprose\n~~~text\n");
    source.push_str(&"payload\n".repeat(200));
    source.push_str("~~~\n# After\nend\n");
    let facts = graph_search_core::units::extract(
        "guide.markdown",
        &source,
        "hash",
        graph_search_types::Language::Unknown,
        &[],
    );
    assert!(!facts.truncated);
    let code: Vec<_> = facts
        .units
        .iter()
        .filter(|unit| unit.kind == SourceUnitKind::MarkdownCodeFence)
        .collect();
    assert_eq!(code.len(), 3);
    assert_eq!(code[0].span.start_line, 3);
    assert_eq!(code.last().unwrap().span.end_line, 204);
    let block = code[0].fence.unwrap();
    assert!(block.closed);
    assert_eq!((block.span.start_line, block.span.end_line), (3, 204));
    let label = block.language_span.unwrap();
    assert_eq!(
        &source[label.start_byte as usize..label.end_byte as usize],
        "text"
    );
    assert_eq!(
        (block.content_span.start_line, block.content_span.end_line),
        (4, 203)
    );
    for unit in code {
        assert_eq!(unit.fence, Some(block));
        assert!(unit.span.start_line >= 3 && unit.span.end_line <= 204);
        assert!(unit.span.end_line.saturating_sub(unit.span.start_line) < 80);
        assert!(
            source
                .get(unit.span.start_byte as usize..unit.span.end_byte as usize)
                .is_some()
        );
    }
    assert_eq!(
        facts.units.last().unwrap().kind,
        SourceUnitKind::MarkdownParagraph
    );
    assert_eq!(facts.units.last().unwrap().span.start_line, 206);
    assert_eq!(facts.units.last().unwrap().headings[0].span.start_line, 205);
}

#[test]
fn frontmatter_is_searchable_after_reopen_without_prose_or_false_fence_context() {
    let root = tempfile::tempdir().unwrap();
    let frontmatter =
        "---\r\ntitle: café frontmatterNeedle\r\n# authored metadata comment\r\n```\r\n---\r\n";
    let source = format!("{frontmatter}# Guide\r\nUnrelated prose.\r\n");
    std::fs::write(root.path().join("guide.md"), &source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        ..OpenOptions::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("frontmatterNeedle"))
        .unwrap();
    let evidence = result
        .items
        .iter()
        .find(|item| item.node.path == "guide.md")
        .and_then(|item| item.evidence.as_ref())
        .unwrap();
    assert_eq!(evidence.kind, SourceUnitKind::MarkdownFrontmatter);
    assert_eq!(evidence.span.start_line, 1);
    assert_eq!(evidence.span.end_line, 5);
    assert_eq!(
        &source[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
        frontmatter
    );
}

#[test]
fn long_frontmatter_is_bounded_and_covers_raw_bytes_without_crossing_into_prose() {
    let mut source = String::from("+++\n");
    source.push_str(&"key = 'élève'\r\n".repeat(200));
    source.push_str("+++\n# Guide\nprose\n");
    let facts = graph_search_core::units::extract(
        "guide.mdx",
        &source,
        "hash",
        graph_search_types::Language::Unknown,
        &[],
    );
    let metadata: Vec<_> = facts
        .units
        .iter()
        .filter(|unit| unit.kind == SourceUnitKind::MarkdownFrontmatter)
        .collect();
    assert_eq!(metadata.len(), 3);
    assert_eq!(metadata[0].span.start_byte, 0);
    let end = source.find("# Guide").unwrap();
    assert_eq!(metadata.last().unwrap().span.end_byte as usize, end);
    let mut covered_until = 0;
    for unit in metadata {
        let start = unit.span.start_byte as usize;
        let finish = unit.span.end_byte as usize;
        assert!(start <= covered_until);
        assert!(finish <= end);
        assert!(unit.span.end_line - unit.span.start_line < 80);
        assert!(source.get(start..finish).is_some());
        covered_until = finish;
    }
    assert_eq!(covered_until, end);
    assert_eq!(
        facts.units.last().unwrap().kind,
        SourceUnitKind::MarkdownParagraph
    );
    assert_eq!(
        facts.units.last().unwrap().headings[0].span.start_byte as usize,
        end
    );
    assert!(!facts.truncated);
}

#[test]
fn setext_title_stays_with_its_searchable_section() {
    let source = "# Intro\r\nOther material.\r\n\r\nélève configuration\r\ncontinued title\r\n===\r\nsetextNeedle body\r\n\r\nNext section\r\n---\r\nOther material.\r\n";
    let facts = graph_search_core::units::extract(
        "guide.md",
        source,
        "hash",
        graph_search_types::Language::Unknown,
        &[],
    );
    let unit = facts
        .units
        .iter()
        .find(|unit| unit.terms.contains_key("needle"))
        .unwrap();
    assert_eq!(unit.kind, SourceUnitKind::MarkdownParagraph);
    assert_eq!(unit.span.start_line, 7);
    assert_eq!(unit.span.end_line, 8);
    assert_eq!(
        &source[unit.span.start_byte as usize..unit.span.end_byte as usize],
        "setextNeedle body\r\n\r\n"
    );
    let title = unit.headings[0].title_span;
    assert_eq!(
        &source[title.start_byte as usize..title.end_byte as usize],
        "élève configuration\r\ncontinued title\r\n"
    );
    assert_eq!(facts.units.len(), 6);
}

#[test]
fn heading_ancestry_survives_fragmentation_publication_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut source = String::from("# Guide ###\r\nintroduction\r\n### Deep café\r\n~~~rust\r\n");
    source.push_str(&"plain body\r\n".repeat(100));
    source.push_str(
        "ancestryNeedle();\r\n~~~\r\nSibling\r\n---\r\nother\r\n# Replacement\r\nlast\r\n",
    );
    assert_ancestry_fragments(&source);
    std::fs::write(root.path().join("guide.md"), &source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        reconcile: graph_search::Reconcile::Never,
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    let output = index
        .search()
        .explore(&ExploreQuery::new("ancestryNeedle"))
        .unwrap();
    let evidence = output
        .items
        .iter()
        .find_map(|item| item.evidence.as_ref())
        .unwrap();
    assert_eq!(evidence.kind, SourceUnitKind::MarkdownCodeFence);
    assert!(
        serde_json::to_vec(&output).unwrap().len() <= graph_search_types::limits::MAX_TOTAL_BYTES
    );
    let item = &output.items[0];
    let parents: Vec<_> = item
        .excerpts
        .iter()
        .filter(|excerpt| excerpt.role == graph_search_types::result::ExcerptRole::DocumentHeading)
        .flat_map(|excerpt| excerpt.snippet.lines.iter().map(String::as_str))
        .collect();
    assert!(parents.contains(&"# Guide ###"));
    assert!(parents.contains(&"### Deep café"));
    assert!(item.excerpts.iter()
        .filter(|excerpt| excerpt.role == graph_search_types::result::ExcerptRole::DocumentFence)
        .flat_map(|excerpt| &excerpt.snippet.lines).any(|line| line == "~~~rust"));
    let old_hash = evidence.source_hash.clone();
    let changed = source
        .replacen("Guide", "Updated Guide", 1)
        .replacen("~~~rust", "~~~python", 1);
    std::fs::write(root.path().join("guide.md"), &changed).unwrap();
    let live = index
        .search()
        .explore(&ExploreQuery::new("ancestryNeedle"))
        .unwrap();
    let evidence = live
        .items
        .iter()
        .find_map(|item| item.evidence.as_ref())
        .unwrap();
    assert!(evidence.live);
    assert_ne!(evidence.source_hash, old_hash);
    assert!(live.items.iter().flat_map(|item| &item.excerpts)
        .filter(|excerpt| excerpt.role == graph_search_types::result::ExcerptRole::DocumentFence)
        .flat_map(|excerpt| &excerpt.snippet.lines).any(|line| line == "~~~python"));
    assert!(
        live.items
            .iter()
            .flat_map(|item| &item.excerpts)
            .filter(
                |excerpt| excerpt.role == graph_search_types::result::ExcerptRole::DocumentHeading
            )
            .flat_map(|excerpt| &excerpt.snippet.lines)
            .any(|line| line == "# Updated Guide ###")
    );
}

fn assert_ancestry_fragments(source: &str) {
    let facts = graph_search_core::units::extract(
        "guide.md",
        source,
        "hash",
        graph_search_types::Language::Unknown,
        &[],
    );
    let code: Vec<_> = facts
        .units
        .iter()
        .filter(|u| u.kind == SourceUnitKind::MarkdownCodeFence)
        .collect();
    assert!(code.len() > 1);
    for unit in code {
        assert_eq!(
            unit.headings.iter().map(|h| h.level).collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(unit.headings[0].title_span.start_line, 1);
        assert_eq!(unit.headings[1].title_span.start_line, 3);
        for (heading, expected) in unit.headings.iter().zip(["Guide", "Deep café"]) {
            assert_eq!(
                &source
                    [heading.title_span.start_byte as usize..heading.title_span.end_byte as usize],
                expected
            );
        }
    }
    let sibling = facts
        .units
        .iter()
        .find(|u| u.terms.contains_key("sibling"))
        .unwrap();
    assert_eq!(
        sibling.headings.iter().map(|h| h.level).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(facts.units.last().unwrap().headings.len(), 1);
}

#[test]
fn long_table_row_groups_retain_original_headers_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let header = "| Label\\|alias | Value |\r\n| :--- | ---: |\r\n";
    let source = format!(
        "# Guide\r\n{header}{}| target | tableNeedle |\r\n\r\nOutside prose.\r\n",
        "| row | value |\r\n".repeat(100)
    );
    let facts = graph_search_core::units::extract(
        "guide.md",
        &source,
        "hash",
        graph_search_types::Language::Unknown,
        &[],
    );
    let tables: Vec<_> = facts
        .units
        .iter()
        .filter(|unit| unit.kind == SourceUnitKind::MarkdownTable)
        .collect();
    assert_eq!(tables.len(), 2);
    let descriptor = tables[0].table.unwrap();
    assert_eq!(descriptor.columns, 2);
    assert_eq!(
        (descriptor.span.start_line, descriptor.span.end_line),
        (2, 104)
    );
    for unit in tables {
        assert_eq!(unit.table, Some(descriptor));
        assert!(unit.span.end_line.saturating_sub(unit.span.start_line) < 80);
        assert_eq!(unit.headings.len(), 1);
    }
    let file = graph_search_types::Node {
        kind: graph_search_types::NodeKind::File,
        path: "guide.md".into(),
        content_hash: Some("hash".into()),
        bytes: Some(source.len() as u64),
        ..Default::default()
    };
    assert!(graph_search_core::units::validate(&file, &facts, |_| None).is_ok());
    for case in 0..4 {
        let mut invalid = facts.clone();
        let unit = invalid
            .units
            .iter_mut()
            .find(|unit| unit.table.is_some())
            .unwrap();
        let table = unit.table.as_mut().unwrap();
        match case {
            0 => table.columns = 0,
            1 => table.span.end_byte = u32::MAX,
            2 => table.delimiter_span.start_byte = 0,
            _ => unit.table = None,
        }
        assert!(graph_search_core::units::validate(&file, &invalid, |_| None).is_err());
    }
    std::fs::write(root.path().join("guide.md"), &source).unwrap();
    let options = || OpenOptions {
        root: root.path().into(),
        ..Default::default()
    };
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    let result = index
        .search()
        .explore(&ExploreQuery::new("tableNeedle"))
        .unwrap();
    let item = result
        .items
        .iter()
        .find(|item| item.node.path == "guide.md")
        .unwrap();
    assert_eq!(
        item.evidence.as_ref().unwrap().kind,
        SourceUnitKind::MarkdownTable
    );
    let parent = item
        .excerpts
        .iter()
        .find(|excerpt| {
            excerpt.role == graph_search_types::result::ExcerptRole::DocumentTableHeader
        })
        .unwrap();
    assert_eq!(parent.snippet.start_line, 2);
    assert_eq!(
        parent.snippet.lines,
        header.lines().map(String::from).collect::<Vec<_>>()
    );
    assert!(
        serde_json::to_vec(&result).unwrap().len() <= graph_search_types::limits::MAX_TOTAL_BYTES
    );
}

#[test]
fn identical_document_versions_keep_path_occurrences_through_updates_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let source = "# Versioned guide\r\n\r\nversionmarker café [reference](target.md)\r\n";
    let paths = ["docs/v1/guide.md", "docs/v2/guide.md"];
    for path in paths {
        let file = root.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, source).unwrap();
    }
    let options = || OpenOptions {
        root: root.path().into(),
        reconcile: graph_search::Reconcile::Never,
        ..Default::default()
    };
    let mut query = ExploreQuery::new("versionmarker");
    query.retrieval.ranking = graph_search_types::RankingStrategy::Body;
    let index = Index::open(options()).unwrap();
    index.reindex().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    let result = index.search().explore(&query).unwrap();
    assert_eq!(result.items.len(), 2);
    let mut original_hash = None;
    for path in paths {
        let item = result
            .items
            .iter()
            .find(|item| item.node.path == path)
            .unwrap();
        assert_eq!(
            item.node.id,
            graph_search_types::NodeId::file(path).to_string()
        );
        let evidence = item.evidence.as_ref().unwrap();
        assert!(!evidence.live);
        assert_eq!(evidence.kind, SourceUnitKind::MarkdownParagraph);
        assert_eq!(evidence.match_line, 3);
        assert_eq!(
            &source[evidence.span.start_byte as usize..evidence.span.end_byte as usize],
            "versionmarker café [reference](target.md)\r\n"
        );
        if let Some(hash) = &original_hash {
            assert_eq!(&evidence.source_hash, hash);
        } else {
            original_hash = Some(evidence.source_hash.clone());
        }
        assert!(
            item.snippet
                .as_ref()
                .unwrap()
                .lines
                .iter()
                .any(|line| line.contains("versionmarker"))
        );
    }
    std::fs::write(
        root.path().join(paths[1]),
        "# Version two\nreplacementmarker\n",
    )
    .unwrap();
    index.sync().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    let original = index.search().explore(&query).unwrap();
    assert_eq!(original.items.len(), 1);
    assert_eq!(original.items[0].node.path, paths[0]);
    assert_eq!(
        original.items[0].evidence.as_ref().unwrap().source_hash,
        original_hash.unwrap()
    );
    query.query = "replacementmarker".into();
    let replacement = index.search().explore(&query).unwrap();
    assert_eq!(replacement.items.len(), 1);
    assert_eq!(replacement.items[0].node.path, paths[1]);
    std::fs::remove_file(root.path().join(paths[0])).unwrap();
    index.sync().unwrap();
    drop(index);
    let index = Index::open(options()).unwrap();
    assert_eq!(
        index.search().explore(&query).unwrap().items[0].node.path,
        paths[1]
    );
    query.query = "versionmarker".into();
    assert!(index.search().explore(&query).unwrap().items.is_empty());
}
