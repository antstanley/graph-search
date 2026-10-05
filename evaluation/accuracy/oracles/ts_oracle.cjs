#!/usr/bin/env node
// Independent TypeScript/JavaScript oracle built on the TypeScript compiler's
// syntactic parser (no type checker, no project). Reads
// {"root": ..., "files": [rel, ...]} on stdin and prints
// {"defs": [...], "calls": [...], "errors": [...], "builtins": [...]}.
//
// Comparable definitions: function declarations, classes, class methods and
// accessors (not constructors), class properties initialised with a function,
// interfaces, type aliases, enums, and `const|let|var` declarations whose
// initialiser is an arrow/function expression -- at module level or inside
// namespaces. Anything nested inside a function body is recorded but not
// comparable. Calls are attributed to the innermost comparable function-like
// definition.
"use strict";
const fs = require("fs");
const path = require("path");
const ts = require(process.env.TYPESCRIPT_PATH ||
  "/Volumes/Delorean/code/whatsurvey/node_modules/typescript");

const kinds = {
  ".ts": ts.ScriptKind.TS, ".mts": ts.ScriptKind.TS, ".cts": ts.ScriptKind.TS,
  ".tsx": ts.ScriptKind.TSX, ".js": ts.ScriptKind.JS, ".mjs": ts.ScriptKind.JS,
  ".cjs": ts.ScriptKind.JS, ".jsx": ts.ScriptKind.JSX,
};

function docOf(node) {
  const parts = [];
  for (const doc of node.jsDoc || []) {
    const text = ts.getTextOfJSDocComment(doc.comment);
    if (text) parts.push(text);
  }
  return parts.join("\n");
}

// Class members may be `#private`; their name text keeps the `#`.
function isMemberName(name) {
  return ts.isIdentifier(name) || ts.isPrivateIdentifier(name);
}

function isFunctionLike(init) {
  return init && (ts.isArrowFunction(init) || ts.isFunctionExpression(init));
}

function analyse(rel, text) {
  const ext = path.extname(rel).toLowerCase();
  const sf = ts.createSourceFile(rel, text, ts.ScriptTarget.Latest, true, kinds[ext]);
  const line = (pos) => sf.getLineAndCharacterOfPosition(pos).line + 1;
  const defs = [];
  const calls = [];
  // Each frame: {name, line, comparable, fn, insideBody}
  const stack = [];
  const inFunctionBody = () => stack.some((f) => f.fn);
  const named = (f) => f.fn && f.name !== "<anonymous>";
  const chain = () => stack.filter(named).map((f) => f.name).reverse();
  const toplevel = () => !stack.some((f) => named(f) && f.comparable);
  // The class whose method directly encloses the call, if any.
  const owner = () => {
    for (let i = stack.length - 1; i >= 0; i--) {
      if (stack[i].fn) {
        if (stack[i].name === "<anonymous>" && ts.isArrowFunction(stack[i].node || {})) continue;
        return stack[i].method && i > 0 && stack[i - 1].cls ? stack.slice(0, i).map((f) => f.name).join(".") : null;
      }
    }
    return null;
  };
  const qual = (name) => stack.map((f) => f.name).concat([name]).join(".");

  function define(node, nameNode, kind, fn, docNode) {
    const name = nameNode.text;
    const comparable = !inFunctionBody();
    const def = {
      path: rel, name, qual: qual(name), kind,
      line: line(nameNode.getStart(sf)), end: line(node.getEnd()),
      doc: docOf(docNode || node), comparable,
    };
    defs.push(def);
    return { name, line: def.line, comparable, fn };
  }

  function within(frame, body) {
    stack.push(frame);
    body();
    stack.pop();
  }

  function visit(node) {
    if (ts.isFunctionDeclaration(node) && node.name) {
      return within(define(node, node.name, "function", true), () => ts.forEachChild(node, visit));
    }
    if ((ts.isClassDeclaration(node) || ts.isClassExpression(node)) && node.name) {
      const frame = define(node, node.name, "class", false);
      frame.cls = true;
      return within(frame, () => ts.forEachChild(node, visit));
    }
    if ((ts.isMethodDeclaration(node) || ts.isGetAccessorDeclaration(node) || ts.isSetAccessorDeclaration(node))
        && node.name && isMemberName(node.name) && node.parent && ts.isClassLike(node.parent)) {
      const frame = define(node, node.name, "method", true);
      frame.method = true;
      return within(frame, () => ts.forEachChild(node, visit));
    }
    if ((ts.isMethodDeclaration(node) && node.parent && ts.isObjectLiteralExpression(node.parent)
         || ts.isPropertyAssignment(node) && isFunctionLike(node.initializer))
        && node.name && ts.isIdentifier(node.name)) {
      const def = define(node, node.name, "object_method", true);
      defs[defs.length - 1].comparable = false;
      def.comparable = false;
      return within(def, () => ts.forEachChild(node, visit));
    }
    if (ts.isPropertyDeclaration(node) && node.name && isMemberName(node.name)
        && isFunctionLike(node.initializer)) {
      const frame = define(node, node.name, "method", true);
      frame.method = ts.isArrowFunction(node.initializer);
      return within(frame, () => ts.forEachChild(node, visit));
    }
    if (ts.isInterfaceDeclaration(node)) {
      return within(define(node, node.name, "interface", false), () => ts.forEachChild(node, visit));
    }
    if (ts.isTypeAliasDeclaration(node)) {
      define(node, node.name, "type_alias", false);
      return ts.forEachChild(node, visit);
    }
    if (ts.isEnumDeclaration(node)) {
      define(node, node.name, "enum", false);
      return ts.forEachChild(node, visit);
    }
    // Interface/type-literal method signatures: recorded for name uniqueness.
    if (ts.isMethodSignature(node) && node.name && ts.isIdentifier(node.name)) {
      const def = define(node, node.name, "signature", false);
      defs[defs.length - 1].comparable = false;
      return within(def, () => ts.forEachChild(node, visit));
    }
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && isFunctionLike(node.initializer)) {
      const statement = node.parent && node.parent.parent;
      return within(define(node, node.name, "function", true, statement), () => ts.forEachChild(node, visit));
    }
    if (ts.isConstructorDeclaration(node)) {
      // A calling unit and a definition, but not sampled: it has no name to query.
      const comparable = !inFunctionBody();
      const start = line(node.getStart(sf));
      defs.push({ path: rel, name: "constructor", qual: qual("constructor"), kind: "constructor",
        line: start, end: line(node.getEnd()), doc: "", comparable: false });
      return within({ name: "constructor", line: start, comparable, fn: true, method: true },
        () => ts.forEachChild(node, visit));
    }
    if (ts.isFunctionLike(node)) {
      // Anonymous function bodies: not definitions, but nested defs are local.
      return within({ name: "<anonymous>", line: line(node.getStart(sf)), comparable: false, fn: true, node },
        () => ts.forEachChild(node, visit));
    }
    if (ts.isCallExpression(node)) {
      const callee = node.expression;
      let name = null;
      let recv = null;
      if (ts.isIdentifier(callee)) {
        name = callee.text; recv = "none";
      } else if (ts.isPropertyAccessExpression(callee) && isMemberName(callee.name)) {
        name = callee.name.text;
        recv = callee.expression.kind === ts.SyntaxKind.ThisKeyword ? "self" : "other";
      }
      if (name) {
        calls.push({ path: rel, line: line(callee.getEnd() - 1), name, recv, chain: chain(), toplevel: toplevel(), owner: owner() });
      }
    }
    ts.forEachChild(node, visit);
  }
  visit(sf);
  return { defs, calls, parseErrors: sf.parseDiagnostics ? sf.parseDiagnostics.length : 0 };
}

// Built-in method and global names, for filtering ambiguous call names.
function builtins() {
  const names = new Set();
  const protos = [Object, Array, String, Number, Map, Set, WeakMap, Promise, Date, RegExp,
    Function, Error, JSON, Math, Reflect, Object.prototype, Array.prototype, String.prototype,
    Number.prototype, Map.prototype, Set.prototype, Promise.prototype, Date.prototype,
    RegExp.prototype, Function.prototype, globalThis];
  for (const proto of protos) {
    for (const name of Object.getOwnPropertyNames(proto)) names.add(name);
  }
  return [...names];
}

function main() {
  const request = JSON.parse(fs.readFileSync(0, "utf8"));
  const out = { defs: [], calls: [], errors: [], builtins: builtins() };
  for (const rel of request.files) {
    let text;
    try {
      text = fs.readFileSync(path.join(request.root, rel), "utf8");
    } catch (error) {
      out.errors.push({ path: rel, error: "read" });
      continue;
    }
    try {
      const result = analyse(rel, text);
      if (result.parseErrors > 0) out.errors.push({ path: rel, error: `parse:${result.parseErrors}` });
      for (const d of result.defs) out.defs.push(d);
      for (const c of result.calls) out.calls.push(c);
    } catch (error) {
      out.errors.push({ path: rel, error: String(error).slice(0, 200) });
    }
  }
  process.stdout.write(JSON.stringify(out));
}

main();
