//! Independent Rust oracle built on `syn` (no tree-sitter). Reads
//! `{"root": ..., "files": [rel, ...]}` on stdin and prints
//! `{"defs": [...], "calls": [...], "errors": [...]}`.
//!
//! Comparable definitions: free functions, structs, enums, unions, traits, type
//! aliases, consts, statics, `macro_rules!` macros, and functions, consts and
//! types in `impl` and `trait` blocks, at any depth of inline modules. Items inside function bodies
//! are recorded but not comparable. Calls (including those inside macro
//! arguments that parse as comma-separated expressions) record their chain of
//! enclosing functions, innermost first.
use proc_macro2::Span;
use serde_json::{Value, json};
use std::io::Read;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, Ident, Token};

struct Frame {
    name: String,
    /// An `impl` or `trait` block.
    owner: bool,
    comparable: bool,
    function: bool,
}

struct Oracle<'a> {
    path: &'a str,
    defs: Vec<Value>,
    calls: Vec<Value>,
    stack: Vec<Frame>,
    /// Depth of macro arguments being visited.
    in_macro: usize,
}

fn doc(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(pair) = &attr.meta {
            if let Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(text), .. }) = &pair.value {
                lines.push(text.value().trim().to_owned());
            }
        }
    }
    lines.join("\n")
}

fn line(span: Span) -> usize {
    span.start().line
}

impl Oracle<'_> {
    fn in_function(&self) -> bool {
        self.stack.iter().any(|frame| frame.function)
    }

    fn define(&mut self, ident: &Ident, kind: &str, attrs: &[Attribute], end: Span) -> Frame {
        let comparable = !self.in_function();
        let name = ident.to_string();
        let name = name.strip_prefix("r#").unwrap_or(&name).to_owned();
        let mut qual: Vec<&str> = self.stack.iter().map(|frame| frame.name.as_str()).collect();
        qual.push(&name);
        let def_line = line(ident.span());
        self.defs.push(json!({
            "path": self.path,
            "name": name,
            "qual": qual.join("::"),
            "kind": kind,
            "line": def_line,
            "end": end.end().line,
            "doc": doc(attrs),
            "comparable": comparable,
        }));
        Frame { name, owner: false, comparable, function: false }
    }

    fn scoped(&mut self, frame: Frame, body: impl FnOnce(&mut Self)) {
        self.stack.push(frame);
        body(self);
        self.stack.pop();
    }

    fn call(&mut self, ident: &Ident, recv: &str) {
        let chain: Vec<&str> = self
            .stack
            .iter()
            .rev()
            .filter(|frame| frame.function)
            .map(|frame| frame.name.as_str())
            .collect();
        let toplevel = !self.stack.iter().any(|frame| frame.comparable && frame.function);
        // The impl/trait type whose method directly encloses the call, if any.
        let owner = self
            .stack
            .iter()
            .rposition(|frame| frame.function)
            .filter(|&index| index > 0 && self.stack[index - 1].owner)
            .map(|index| self.stack[index - 1].name.clone());
        let name = ident.to_string();
        self.calls.push(json!({
            "path": self.path,
            "line": line(ident.span()),
            "name": name.strip_prefix("r#").unwrap_or(&name),
            "recv": recv,
            "chain": chain,
            "toplevel": toplevel,
            "in_macro": self.in_macro > 0,
            "owner": owner,
        }));
    }
}

impl<'ast> Visit<'ast> for Oracle<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let mut frame = self.define(&item.sig.ident, "function", &item.attrs, item.span());
        frame.function = true;
        self.scoped(frame, |this| visit::visit_item_fn(this, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let mut frame = self.define(&item.sig.ident, "method", &item.attrs, item.span());
        frame.function = true;
        self.scoped(frame, |this| visit::visit_impl_item_fn(this, item));
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        let mut frame = self.define(&item.sig.ident, "method", &item.attrs, item.span());
        frame.function = true;
        self.scoped(frame, |this| visit::visit_trait_item_fn(this, item));
    }

    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.define(&item.ident, "const", &item.attrs, item.span());
        visit::visit_impl_item_const(self, item);
    }

    fn visit_impl_item_type(&mut self, item: &'ast syn::ImplItemType) {
        self.define(&item.ident, "type_alias", &item.attrs, item.span());
        visit::visit_impl_item_type(self, item);
    }

    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        self.define(&item.ident, "const", &item.attrs, item.span());
        visit::visit_trait_item_const(self, item);
    }

    fn visit_trait_item_type(&mut self, item: &'ast syn::TraitItemType) {
        self.define(&item.ident, "type_alias", &item.attrs, item.span());
        visit::visit_trait_item_type(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.define(&item.ident, "struct", &item.attrs, item.span());
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.define(&item.ident, "enum", &item.attrs, item.span());
        visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        self.define(&item.ident, "struct", &item.attrs, item.span());
        visit::visit_item_union(self, item);
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        let mut frame = self.define(&item.ident, "trait", &item.attrs, item.span());
        frame.owner = true;
        self.scoped(frame, |this| visit::visit_item_trait(this, item));
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.define(&item.ident, "type_alias", &item.attrs, item.span());
        visit::visit_item_type(self, item);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.define(&item.ident, "const", &item.attrs, item.span());
        visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.define(&item.ident, "static", &item.attrs, item.span());
        visit::visit_item_static(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if let Some(ident) = &item.ident {
            // `macro_rules! name { ... }`: a definition; its body is not code.
            self.define(ident, "macro", &item.attrs, item.span());
            return;
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let name = match item.self_ty.as_ref() {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map_or_else(|| "<impl>".to_owned(), |segment| segment.ident.to_string()),
            _ => "<impl>".to_owned(),
        };
        let frame = Frame {
            name,
            owner: true,
            comparable: !self.in_function(),
            function: false,
        };
        self.scoped(frame, |this| visit::visit_item_impl(this, item));
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let frame = Frame {
            name: item.ident.to_string(),
            owner: false,
            comparable: !self.in_function(),
            function: false,
        };
        self.scoped(frame, |this| visit::visit_item_mod(this, item));
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expr.func.as_ref() {
            let segments = &path.path.segments;
            if let Some(last) = segments.last() {
                let recv = match segments.len() {
                    1 => "none",
                    _ if segments.first().is_some_and(|first| first.ident == "Self") => "self",
                    _ => "path",
                };
                self.call(&last.ident, recv);
            }
        }
        visit::visit_expr_call(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let recv = match expr.receiver.as_ref() {
            Expr::Path(path) if path.path.is_ident("self") => "self",
            _ => "other",
        };
        self.call(&expr.method, recv);
        visit::visit_expr_method_call(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        // Most call-carrying macros (`assert!`, `format!`, `vec!`, `println!`,
        // `write!`) take comma-separated expressions.
        if let Ok(args) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            self.in_macro += 1;
            for arg in &args {
                self.visit_expr(arg);
            }
            self.in_macro -= 1;
        }
        visit::visit_macro(self, mac);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    let root = std::path::PathBuf::from(request["root"].as_str().ok_or("root")?);
    let mut out = json!({"defs": [], "calls": [], "errors": []});
    for rel in request["files"].as_array().ok_or("files")? {
        let rel = rel.as_str().ok_or("file")?;
        let parsed = std::fs::read_to_string(root.join(rel))
            .map_err(|error| error.to_string())
            .and_then(|text| syn::parse_file(&text).map_err(|error| error.to_string()));
        let file = match parsed {
            Ok(file) => file,
            Err(error) => {
                out["errors"]
                    .as_array_mut()
                    .ok_or("errors")?
                    .push(json!({"path": rel, "error": error.chars().take(200).collect::<String>()}));
                continue;
            }
        };
        let mut oracle = Oracle { path: rel, defs: Vec::new(), calls: Vec::new(), stack: Vec::new(), in_macro: 0 };
        oracle.visit_file(&file);
        out["defs"].as_array_mut().ok_or("defs")?.extend(oracle.defs);
        out["calls"].as_array_mut().ok_or("calls")?.extend(oracle.calls);
    }
    println!("{out}");
    Ok(())
}
