//! Per-language extraction fixtures: each small file pins the exact nodes
//! and references extracted, with their lines (`SPEC.md` §15.1).

#![forbid(unsafe_code)]
#![allow(clippy::expect_used, clippy::panic)]

use graph_search_core::extraction::Extraction;
use graph_search_core::ports::{LanguageExtractor, SourceFile};
use graph_search_types::kind::{EdgeKind, NodeKind};
use std::path::Path;

fn extract(extractor: &dyn LanguageExtractor, rel: &str) -> Extraction {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let file = SourceFile {
        path: Path::new(rel),
        text: &text,
    };
    extractor
        .extract(&file)
        .unwrap_or_else(|e| panic!("extract {rel}: {e}"))
}

fn names(extraction: &Extraction, kind: NodeKind) -> Vec<String> {
    extraction
        .symbols
        .iter()
        .filter(|fact| fact.kind == kind)
        .map(|fact| fact.name.clone())
        .collect()
}

fn refs(extraction: &Extraction, kind: EdgeKind) -> Vec<String> {
    extraction
        .references
        .iter()
        .filter(|fact| fact.kind == kind)
        .map(|fact| fact.name.clone())
        .collect()
}

#[test]
fn rust_functions_and_calls() {
    let extraction = extract(&graph_search_langs::RustExtractor, "rust/functions.rs");
    assert_eq!(
        names(&extraction, NodeKind::Function),
        vec!["main", "helper"]
    );
    assert_eq!(
        refs(&extraction, EdgeKind::Calls),
        vec!["helper", "std::mem::drop"]
    );
    // The call belongs to its enclosing function.
    let main_call = extraction
        .references
        .iter()
        .find(|fact| fact.kind == EdgeKind::Calls && fact.name == "helper")
        .expect("helper call");
    assert!(
        main_call
            .from_key
            .as_deref()
            .is_some_and(|k| k.contains("main"))
    );
    assert_eq!(extraction.symbols[0].span.start_line, 1);
}

#[test]
fn rust_items() {
    let extraction = extract(&graph_search_langs::RustExtractor, "rust/items.rs");
    assert_eq!(names(&extraction, NodeKind::Struct), vec!["Parser"]);
    assert_eq!(names(&extraction, NodeKind::Field), vec!["source"]);
    assert_eq!(names(&extraction, NodeKind::Enum), vec!["Mode"]);
    assert_eq!(names(&extraction, NodeKind::Variant), vec!["Fast", "Slow"]);
    assert_eq!(names(&extraction, NodeKind::Trait), vec!["Read"]);
    assert_eq!(
        names(&extraction, NodeKind::Impl),
        vec!["impl Read for Parser"]
    );
    assert_eq!(
        names(&extraction, NodeKind::Method),
        vec!["read"],
        "{:?}",
        names(&extraction, NodeKind::Method)
    );
    assert_eq!(names(&extraction, NodeKind::TypeAlias), vec!["Alias"]);
    assert_eq!(names(&extraction, NodeKind::Const), vec!["MAX"]);
    assert_eq!(names(&extraction, NodeKind::Static), vec!["NAME"]);
    assert_eq!(names(&extraction, NodeKind::Macro), vec!["shout"]);
    assert_eq!(names(&extraction, NodeKind::Module), vec!["nested"]);
    assert_eq!(refs(&extraction, EdgeKind::Implements), vec!["Read"]);
    // The field's declared type is a type use.
    assert!(refs(&extraction, EdgeKind::TypeUses).contains(&String::from("Vec")));
}

#[test]
fn rust_imports() {
    let extraction = extract(&graph_search_langs::RustExtractor, "rust/imports.rs");
    let imports = refs(&extraction, EdgeKind::Imports);
    assert!(
        imports.contains(&String::from("std::collections::HashMap")),
        "{imports:?}"
    );
    assert!(
        imports.contains(&String::from("crate::items::Parser")),
        "{imports:?}"
    );
    assert!(imports.contains(&String::from("helper")), "{imports:?}");
}

#[test]
fn typescript_widget() {
    let extraction = extract(
        &graph_search_langs::TypeScriptExtractor,
        "typescript/widget.ts",
    );
    assert_eq!(names(&extraction, NodeKind::Interface), vec!["Shape"]);
    assert_eq!(names(&extraction, NodeKind::TypeAlias), vec!["Alias"]);
    assert_eq!(
        names(&extraction, NodeKind::Function),
        vec!["makeWidget"],
        "{:?}",
        names(&extraction, NodeKind::Function)
    );
    assert_eq!(names(&extraction, NodeKind::Class), vec!["Widget"]);
    assert_eq!(names(&extraction, NodeKind::Method), vec!["render"]);
    assert_eq!(names(&extraction, NodeKind::Field), vec!["size"]);
    assert!(refs(&extraction, EdgeKind::Imports).contains(&String::from("./alpha")));
    // The aliased import records the *exported* name ("beta") with its
    // specifier, so rule 2 can find the export.
    let beta = extraction
        .references
        .iter()
        .find(|fact| fact.name == "beta" && fact.kind == EdgeKind::Imports)
        .expect("beta binding");
    assert_eq!(beta.via_import.as_deref(), Some("./mixed"));
    assert_eq!(
        refs(&extraction, EdgeKind::Exports),
        vec!["Shape", "Alias", "Widget"]
    );
    assert_eq!(refs(&extraction, EdgeKind::Extends), vec!["BaseWidget"]);
    assert_eq!(refs(&extraction, EdgeKind::Implements), vec!["Shape"]);
    assert!(refs(&extraction, EdgeKind::Calls).contains(&String::from("build")));
}

#[test]
fn javascript_app() {
    let extraction = extract(
        &graph_search_langs::JavaScriptExtractor,
        "javascript/app.js",
    );
    assert_eq!(
        names(&extraction, NodeKind::Function),
        vec!["boot"],
        "{:?}",
        names(&extraction, NodeKind::Function)
    );
    assert_eq!(names(&extraction, NodeKind::Class), vec!["Panel"]);
    assert_eq!(
        names(&extraction, NodeKind::Method),
        vec!["constructor", "open"]
    );
    assert_eq!(
        names(&extraction, NodeKind::Const),
        vec!["store", "VERSION"]
    );
    // require("./store") is an import; the destructured binding carries it.
    assert!(refs(&extraction, EdgeKind::Imports).contains(&String::from("./store")));
    assert!(refs(&extraction, EdgeKind::Imports).contains(&String::from("./ui.js")));
    assert_eq!(refs(&extraction, EdgeKind::Exports), vec!["boot"]);
}

#[test]
fn html_page() {
    let extraction = extract(&graph_search_langs::HtmlExtractor, "html/page.html");
    let elements = names(&extraction, NodeKind::Element);
    // Elements with an id, a class, or a relationship become nodes; bare
    // containers do not.
    assert_eq!(
        elements,
        vec!["link", "script", "main", ".btn"],
        "{elements:?}"
    );
    let main = extraction
        .symbols
        .iter()
        .find(|fact| fact.name == "main")
        .expect("main div");
    assert_eq!(main.attributes.get("tag").map(String::as_str), Some("div"));
    assert_eq!(
        main.attributes.get("classes").map(String::as_str),
        Some("container dark")
    );
    let link_like = extraction
        .symbols
        .iter()
        .filter(|fact| fact.attributes.contains_key("href") || fact.attributes.contains_key("src"))
        .count();
    assert_eq!(
        link_like, 3,
        "the link, the script, and the anchor are link nodes"
    );
    assert_eq!(
        extraction.symbols.len(),
        4,
        "{:?}",
        names(&extraction, NodeKind::Element)
    );
}

#[test]
fn css_site() {
    let extraction = extract(&graph_search_langs::CssExtractor, "css/site.css");
    let rules = names(&extraction, NodeKind::CssRule);
    assert!(rules.contains(&String::from(".container")), "{rules:?}");
    assert!(rules.contains(&String::from("#main")), "{rules:?}");
    assert!(
        rules.contains(&String::from(".btn.primary, .btn.dark")),
        "{rules:?}"
    );
    assert!(rules.contains(&String::from(".container")));
    assert_eq!(
        names(&extraction, NodeKind::CssCustomProperty),
        vec!["--gap"]
    );
    let at_rules = names(&extraction, NodeKind::CssAtRule);
    assert!(
        at_rules.iter().any(|name| name.starts_with("media")),
        "{at_rules:?}"
    );
    assert_eq!(refs(&extraction, EdgeKind::Imports), vec!["./reset.css"]);
}

#[test]
fn javascript_and_typescript_spans_slice_original_utf8() {
    let extractors: Vec<(&dyn LanguageExtractor, &str)> = vec![
        (&graph_search_langs::JavaScriptExtractor, "sample.js"),
        (&graph_search_langs::TypeScriptExtractor, "sample.ts"),
    ];
    for (extractor, path) in extractors {
        for prefix in ["", "// café\r\n"] {
            let declaration = "function café() {\r\n  return 'é';\r\n}";
            let source = format!("{prefix}{declaration}\r\n");
            let facts = extractor
                .extract(&SourceFile {
                    path: Path::new(path),
                    text: &source,
                })
                .expect("extract");
            let symbol = facts
                .symbols
                .iter()
                .find(|s| s.name == "café")
                .expect("function");
            assert_eq!(symbol.span.start_byte as usize, prefix.len());
            assert_eq!(
                &source[symbol.span.start_byte as usize..symbol.span.end_byte as usize],
                declaration
            );
            assert_eq!(
                symbol.span.start_line,
                if prefix.is_empty() { 1 } else { 2 }
            );
        }
    }
}

#[test]
fn every_language_preserves_raw_byte_and_line_coordinates() {
    let cases: Vec<(&dyn LanguageExtractor, &str, &str, &str)> = vec![
        (
            &graph_search_langs::RustExtractor,
            "a.rs",
            "// café\r\n",
            "fn café() {\r\n let value = \"é\";\r\n}",
        ),
        (
            &graph_search_langs::TypeScriptExtractor,
            "a.ts",
            "// café\r\n",
            "function café() {\r\n return 'é';\r\n}",
        ),
        (
            &graph_search_langs::JavaScriptExtractor,
            "a.js",
            "// café\r\n",
            "function café() {\r\n return 'é';\r\n}",
        ),
        (
            &graph_search_langs::HtmlExtractor,
            "a.html",
            "<!-- café -->\r\n",
            "<div id=\"café\">\r\n é\r\n</div>",
        ),
        (
            &graph_search_langs::CssExtractor,
            "a.css",
            "/* café */\r\n",
            ".café {\r\n color: red;\r\n}",
        ),
    ];
    for (extractor, path, prefix, declaration) in cases {
        for prefix in ["", prefix] {
            let source = format!("{prefix}{declaration}");
            let facts = extractor
                .extract(&SourceFile {
                    path: Path::new(path),
                    text: &source,
                })
                .expect("extract");
            assert!(!facts.symbols.is_empty(), "{path}");
            let span = facts.symbols[0].span;
            assert_eq!(
                &source[span.start_byte as usize..span.end_byte as usize],
                declaration,
                "{path}"
            );
            for fact in facts.symbols {
                let span = fact.span;
                let start = span.start_byte as usize;
                let end = span.end_byte as usize;
                assert!(
                    !source
                        .get(start..end)
                        .expect("valid UTF-8 slice")
                        .is_empty()
                );
                assert_eq!(
                    span.start_line as usize,
                    1 + source[..start].bytes().filter(|b| *b == b'\n').count()
                );
                assert_eq!(
                    span.end_line as usize,
                    1 + source[..end].bytes().filter(|b| *b == b'\n').count()
                );
            }
        }
    }
}
