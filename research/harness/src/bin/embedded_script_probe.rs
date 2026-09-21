//! Adapter probe only: framework ranges are supplied by an independent oracle.
use graph_search_core::{
    ports::SourceFile,
    resolve::{SymbolTable, resolve_reference},
};
use graph_search_types::{EdgeKind, Language, Node, NodeId};
use std::{collections::BTreeSet, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: embedded_script_probe FILE START END js|ts DOMAIN".into());
    }
    let source = std::fs::read_to_string(&args[1])?;
    let language = Language::parse(&args[4]).ok_or("unknown language")?;
    let facts = graph_search_langs::embedded::script(
        &SourceFile {
            path: Path::new(&args[1]),
            text: &source,
        },
        args[2].parse()?..args[3].parse()?,
        language,
        &args[5],
    )?;
    let mut table = SymbolTable::new();
    for symbol in &facts.symbols {
        table.add(&Node {
            id: NodeId::new(format!("sym:component.svelte#{}", symbol.key)),
            path: "component.svelte".into(),
            kind: symbol.kind,
            name: Some(symbol.name.clone()),
            qualified_name: Some(symbol.qualified_name.clone()),
            span: Some(symbol.span),
            attributes: symbol.attributes.clone(),
            ..Node::default()
        });
    }
    let known = BTreeSet::new();
    let calls: Vec<_> = facts
        .references
        .iter()
        .filter(|r| r.kind == EdgeKind::Calls)
        .map(|r| {
            let result = resolve_reference(r, "component.svelte", &table, &known, language);
            serde_json::json!({"raw_name":r.raw_name,"span":r.span,"from_key":r.from_key,
            "target":result.to,"class":result.class,"reason":result.reason})
        })
        .collect();
    println!("{}", serde_json::json!({"facts":facts,"calls":calls}));
    Ok(())
}
