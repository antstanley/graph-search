//! Native bounded ESM export lookup. No runtime module loader is invoked.
use crate::resolve::{SymbolTable, compatible};
use graph_search_types::{EdgeKind, NodeId, js_module::JsModule};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub(crate) struct Modules(pub(crate) BTreeMap<String, JsModule>);

impl Modules {
    pub(crate) fn resolve(
        &self,
        path: &str,
        name: &str,
        kind: EdgeKind,
        table: &SymbolTable<'_>,
        known: &BTreeSet<String>,
    ) -> Result<NodeId, &'static str> {
        let mut lookup = Lookup {
            modules: self,
            table,
            known,
            active: BTreeSet::new(),
            remaining: 4096,
        };
        let targets = lookup.visit(path, name, kind)?;
        match targets.len() {
            0 => Err("js_export_missing"),
            1 => targets.into_iter().next().ok_or("js_export_missing"),
            _ => Err("js_export_ambiguous"),
        }
    }
}

struct Lookup<'a> {
    modules: &'a Modules,
    table: &'a SymbolTable<'a>,
    known: &'a BTreeSet<String>,
    active: BTreeSet<(String, String)>,
    remaining: usize,
}
impl Lookup<'_> {
    fn visit(
        &mut self,
        path: &str,
        name: &str,
        kind: EdgeKind,
    ) -> Result<BTreeSet<NodeId>, &'static str> {
        if self.active.len() >= 64 || self.remaining == 0 {
            return Err("js_export_resolution_limit");
        }
        self.remaining = self.remaining.saturating_sub(1);
        let key = (path.to_owned(), name.to_owned());
        if !self.active.insert(key.clone()) {
            return Ok(BTreeSet::new());
        }
        let result = self.exports(path, name, kind);
        self.active.remove(&key);
        result
    }

    fn exports(
        &mut self,
        path: &str,
        name: &str,
        kind: EdgeKind,
    ) -> Result<BTreeSet<NodeId>, &'static str> {
        let module = self
            .modules
            .0
            .get(path)
            .ok_or("js_module_facts_unavailable")?;
        if !module.complete {
            return Err("js_module_surface_incomplete");
        }
        let explicit: Vec<_> = module
            .exports
            .iter()
            .filter(|export| export.exported == name)
            .collect();
        if explicit.len() > 1 {
            return Err("js_export_ambiguous");
        }
        if let Some(export) = explicit.first() {
            if export.type_only && kind == EdgeKind::Calls {
                return Err("js_type_only_export");
            }
            let local = export.local.as_deref().ok_or("js_export_value_unmodeled")?;
            if local == "*" {
                return Err("js_namespace_value_unmodeled");
            }
            if let Some(source) = &export.source {
                let target = self.table.js_specifier(path, source, self.known)?;
                return self.visit(&target, local, kind);
            }
            let file = self.table.in_path(path);
            let symbols: Vec<_> = file
                .iter()
                .filter(|node| {
                    crate::symbols::indexed(node, crate::symbols::SymbolIndex::Name, local)
                        && node.attribute("lexical_local") != Some("true")
                        && node
                            .parent
                            .as_ref()
                            .is_none_or(|parent| parent == &NodeId::file(path))
                })
                .collect();
            let imported: Vec<_> = module
                .imports
                .iter()
                .filter(|import| import.local == local)
                .collect();
            if symbols.len().saturating_add(imported.len()) > 1 {
                return Err("js_export_local_ambiguous");
            }
            if let Some(node) = symbols.first() {
                if kind != EdgeKind::Imports && !compatible(kind, node.kind) {
                    return Err("js_export_kind_incompatible");
                }
                return Ok(BTreeSet::from([node.id.clone()]));
            }
            if let Some(import) = imported.first() {
                if import.type_only && kind == EdgeKind::Calls {
                    return Err("js_type_only_import");
                }
                if import.imported == "*" {
                    return Err("js_namespace_value_unmodeled");
                }
                let target = self.table.js_specifier(path, &import.source, self.known)?;
                return self.visit(&target, &import.imported, kind);
            }
            return Err("js_export_local_missing");
        }
        let mut targets = BTreeSet::new();
        if name != "default" {
            for export in &module.exports {
                if export.exported != "*" || (export.type_only && kind == EdgeKind::Calls) {
                    continue;
                }
                let source = export.source.as_ref().ok_or("js_export_module_missing")?;
                let target = self.table.js_specifier(path, source, self.known)?;
                targets.extend(self.visit(&target, name, kind)?);
            }
        }
        Ok(targets)
    }
}
