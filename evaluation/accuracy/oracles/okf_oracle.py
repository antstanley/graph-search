#!/usr/bin/env python3
"""Independent OKF v0.2 oracle: a small CommonMark-subset reader (no tree-sitter).

Reads {"root": ..., "files": [rel, ...], "walked": [rel, ...]} on stdin and
prints {"concepts": [...], "sections": [...], "links": [...], "errors": [...]}.

* concepts: every non-reserved document (not index.md / log.md), titled by
  frontmatter `title` else the file stem, with its `description`.
* sections: ATX and setext headings outside fenced code, by reader-visible text.
* links: (source, target) for inline and reference links/images whose
  destination resolves to a walked file (OKF §6: `/` is bundle-relative, a
  directory names its index.md, an extensionless path names `x.md`), plus
  `cites` pairs for path-valued `sources[].resource` entries.
"""
import json
import posixpath
import re
import sys
from pathlib import Path
from urllib.parse import unquote

RESERVED = {"index.md", "log.md"}
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
ATX = re.compile(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$")
SETEXT = re.compile(r"^ {0,3}(=+|-+)[ \t]*$")
INLINE = re.compile(r"!?\[((?:[^\[\]]|\[[^\]]*\])*)\]\(\s*(<[^>]*>|[^\s()]*(?:\([^\s()]*\)[^\s()]*)*)(?:\s+(?:\"[^\"]*\"|'[^']*'|\([^)]*\)))?\s*\)")
FULL_REF = re.compile(r"!?\[((?:[^\[\]]|\[[^\]]*\])*)\]\[([^\]]*)\]")
SHORTCUT = re.compile(r"!?\[([^\[\]^][^\[\]]*)\](?![\[(:])")
REF_DEF = re.compile(r"^ {0,3}\[([^\]^][^\]]*)\]:\s*(<[^>]*>|\S+)")
CODE_SPAN = re.compile(r"(`+)(.+?)\1")
SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.\-]*:")


def frontmatter(lines):
    """(fields, body_start): top-level scalars plus sources[].resource values."""
    if not lines or lines[0].strip() != "---":
        return {}, 0
    for end in range(1, len(lines)):
        if lines[end].strip() in ("---", "..."):
            break
    else:
        return {}, len(lines)
    fields, resources, key = {}, [], None
    for raw in lines[1:end]:
        top = re.match(r"^([A-Za-z_][\w\-]*):\s*(.*)$", raw)
        if top:
            key = top.group(1)
            fields[key] = scalar(top.group(2))
            continue
        resource = re.match(r"^\s*(?:-\s+)?resource:\s*(.+)$", raw)
        if resource and key == "sources":
            resources.append(scalar(resource.group(1)))
    fields["_resources"] = resources
    return fields, end + 1


def scalar(text):
    text = text.strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        inner = text[1:-1]
        return inner.replace("''", "'") if text[0] == "'" else inner.encode().decode("unicode_escape", "ignore") if "\\" in inner else inner
    return text


def visible(text):
    """Reader-visible heading text."""
    text = re.sub(r"\[\^[^\]]*\]", "", text)
    text = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"!?\[([^\]]*)\]\[[^\]]*\]", r"\1", text)
    text = CODE_SPAN.sub(lambda m: m.group(2).strip(), text)
    text = re.sub(r"(\*\*|__|\*|_|~~)(?=\S)(.+?)(?<=\S)\1", r"\2", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"\\([!-/:-@\[-`{-~])", r"\1", text)
    return " ".join(text.split())


def bundle_root(rel, index_dirs):
    """The outermost ancestor directory holding an index.md ('' = root)."""
    parts = posixpath.dirname(rel).split("/") if posixpath.dirname(rel) else []
    for depth in range(len(parts) + 1):
        candidate = "/".join(parts[:depth])
        if candidate in index_dirs:
            return candidate
    return ""


def resolve(dest, rel, root_dir, walked, fallback_root=False):
    dest = dest.strip()
    if dest.startswith("<") and dest.endswith(">"):
        dest = dest[1:-1]
    if not dest or SCHEME.match(dest) or dest.startswith("//") or dest[0] in "#?":
        return None
    dest = re.split(r"[#?]", dest, maxsplit=1)[0]
    dest = unquote(re.sub(r"\\([!-/:-@\[-`{-~])", r"\1", dest))
    bases = []
    if dest.startswith("/"):
        bases.append(posixpath.join(root_dir, dest.lstrip("/")))
    else:
        bases.append(posixpath.join(posixpath.dirname(rel), dest))
        if fallback_root:
            bases.append(posixpath.join(root_dir, dest))
    for base in bases:
        path = posixpath.normpath(base)
        if path.startswith("../") or path == "..":
            continue
        path = "" if path == "." else path
        candidates = [path]
        if dest.endswith("/") or path == "" or (path + "/index.md") in walked:
            candidates.append(posixpath.join(path, "index.md") if path else "index.md")
        if not posixpath.splitext(path)[1]:
            candidates.append(path + ".md")
        for candidate in candidates:
            if candidate in walked:
                return candidate
    return None


def analyse(rel, text, walked, index_dirs):
    lines = text.split("\n")
    fields, start = frontmatter(lines)
    root_dir = bundle_root(rel, index_dirs)
    out = {"concept": None, "sections": [], "links": []}
    name = posixpath.basename(rel)
    if name not in RESERVED:
        out["concept"] = {
            "path": rel,
            "title": fields.get("title") or posixpath.splitext(name)[0],
            "description": fields.get("description", ""),
            "type": fields.get("type", ""),
        }
    # Body lines outside fenced code, with code spans blanked.
    body, fence = [], None
    for number in range(start, len(lines)):
        line = lines[number]
        opener = FENCE.match(line)
        if fence:
            if opener and opener.group(1)[0] == fence[0] and len(opener.group(1)) >= len(fence):
                fence = None
            continue
        if opener:
            fence = opener.group(1)
            continue
        body.append((number + 1, CODE_SPAN.sub(lambda m: " " * len(m.group(0)), line)))
    definitions = {}
    for number, line in body:
        match = REF_DEF.match(line)
        if match:
            definitions.setdefault(match.group(1).strip().lower(), match.group(2))
    previous = None
    for number, line in body:
        heading = ATX.match(line)
        if heading and not line.lstrip().startswith("#" * 7):
            out["sections"].append({"path": rel, "name": visible(heading.group(2) or ""), "line": number})
        elif (previous and SETEXT.match(line) and previous[1].strip()
              and not ATX.match(previous[1]) and not re.match(r"^\s*([-*+>|]|\d+[.)])", previous[1])):
            out["sections"].append({"path": rel, "name": visible(previous[1]), "line": previous[0]})
        previous = (number, line)
        if REF_DEF.match(line):
            continue
        destinations = [m.group(2) for m in INLINE.finditer(line)]
        stripped = INLINE.sub(" ", line)
        for m in FULL_REF.finditer(stripped):
            label = (m.group(2) or m.group(1)).strip().lower()
            if label in definitions:
                destinations.append(definitions[label])
        stripped = FULL_REF.sub(" ", stripped)
        for m in SHORTCUT.finditer(stripped):
            label = m.group(1).strip().lower()
            if label in definitions:
                destinations.append(definitions[label])
        for dest in destinations:
            target = resolve(dest, rel, root_dir, walked)
            if target:
                out["links"].append({"source": rel, "target": target, "line": number, "kind": "links_to"})
    for resource in fields.get("_resources", []):
        if not resource or re.search(r"\s", resource):
            continue
        if "/" not in resource and not posixpath.splitext(resource)[1]:
            continue
        target = resolve(resource, rel, root_dir, walked, fallback_root=True)
        if target:
            out["links"].append({"source": rel, "target": target, "line": 0, "kind": "cites"})
    return out


def main():
    request = json.load(sys.stdin)
    root = Path(request["root"])
    walked = set(request["walked"])
    index_dirs = {posixpath.dirname(p) for p in walked if posixpath.basename(p) == "index.md"}
    result = {"concepts": [], "sections": [], "links": [], "errors": []}
    for rel in request["files"]:
        try:
            text = (root / rel).read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            result["errors"].append({"path": rel, "error": type(error).__name__})
            continue
        doc = analyse(rel, text, walked, index_dirs)
        if doc["concept"]:
            result["concepts"].append(doc["concept"])
        result["sections"].extend(doc["sections"])
        result["links"].extend(doc["links"])
    json.dump(result, sys.stdout)


if __name__ == "__main__":
    main()
