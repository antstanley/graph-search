#!/usr/bin/env python3
"""Independent Python oracle built on the standard library `ast` module.

Reads {"root": ..., "files": [rel, ...]} on stdin and prints
{"defs": [...], "calls": [...], "errors": [...]}.

A definition is *comparable* when it is a module-level or class-body function
or class (including those under module/class-level `if`/`try`/`with`
blocks); functions nested in functions are recorded but not comparable.
Each call records its chain of enclosing functions (innermost first) and
whether any comparable function encloses it.
"""
import ast
import json
import sys
import warnings
from pathlib import Path

warnings.filterwarnings("ignore")


class Visitor(ast.NodeVisitor):
    def __init__(self, path):
        self.path = path
        self.defs = []
        self.calls = []
        # (name, line, comparable, is_class)
        self.stack = []

    def _enclosing_scope_comparable(self):
        # Comparable when every enclosing definition is a class (or none).
        return all(entry[3] for entry in self.stack)

    def _chain(self):
        """Enclosing function names, innermost first."""
        return [name for name, _, _, is_class in reversed(self.stack) if not is_class]

    def _owner(self):
        """The class whose method directly encloses the call, if any."""
        for index in range(len(self.stack) - 1, -1, -1):
            if not self.stack[index][3]:
                if index > 0 and self.stack[index - 1][3]:
                    return ".".join(entry[0] for entry in self.stack[:index])
                return None
        return None

    def _toplevel(self):
        return not any(comparable and not is_class for _, _, comparable, is_class in self.stack)

    def _def(self, node, kind):
        comparable = self._enclosing_scope_comparable()
        in_class = bool(self.stack) and self.stack[-1][3]
        if kind == "function" and in_class:
            kind = "method"
        qual = ".".join([entry[0] for entry in self.stack] + [node.name])
        self.defs.append({
            "path": self.path,
            "name": node.name,
            "qual": qual,
            "kind": kind,
            "line": node.lineno,
            "end": getattr(node, "end_lineno", node.lineno),
            "doc": ast.get_docstring(node) or "",
            "comparable": comparable,
        })
        # Decorators run in the enclosing scope.
        for decorator in node.decorator_list:
            self.visit(decorator)
        self.stack.append((node.name, node.lineno, comparable, kind == "class"))
        for statement in node.body:
            self.visit(statement)
        self.stack.pop()

    def visit_FunctionDef(self, node):
        self._def(node, "function")

    visit_AsyncFunctionDef = visit_FunctionDef

    def visit_ClassDef(self, node):
        for base in node.bases:
            self.visit(base)
        self._def(node, "class")

    def visit_Lambda(self, node):
        self.generic_visit(node)

    def visit_Call(self, node):
        func = node.func
        name = recv = None
        if isinstance(func, ast.Name):
            name, recv = func.id, "none"
        elif isinstance(func, ast.Attribute):
            name = func.attr
            base = func.value
            recv = "self" if isinstance(base, ast.Name) and base.id in ("self", "cls") else "other"
        if name:
            self.calls.append({
                "path": self.path,
                "line": node.lineno,
                "name": name,
                "recv": recv,
                "chain": self._chain(),
                "toplevel": self._toplevel(),
                "owner": self._owner(),
            })
        self.generic_visit(node)


def main():
    request = json.load(sys.stdin)
    root = Path(request["root"])
    out = {"defs": [], "calls": [], "errors": []}
    for rel in request["files"]:
        try:
            text = (root / rel).read_text(encoding="utf-8")
            tree = ast.parse(text, filename=rel)
        except (SyntaxError, UnicodeDecodeError, ValueError, OSError) as error:
            out["errors"].append({"path": rel, "error": type(error).__name__})
            continue
        visitor = Visitor(rel)
        visitor.visit(tree)
        out["defs"].extend(visitor.defs)
        out["calls"].extend(visitor.calls)
    json.dump(out, sys.stdout)


if __name__ == "__main__":
    main()
