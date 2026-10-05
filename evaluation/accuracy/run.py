#!/usr/bin/env python3
"""Oracle-scored accuracy benchmark for graph-search over local repositories.

For each repository: index it into a disposable external store through the
resident library host, list the walked files, run an independent oracle per
language over exactly those files, then issue sampled queries and score them.

Tasks (code languages):
  extraction  per sampled file, symbols the file contains vs oracle definitions
  symbol      `symbol <name>` finds the oracle definition (recall, rank)
  callers     `callers <id>` vs oracle call sites of distinctively named,
              uniquely defined functions (site recall, caller precision)
  binding     `callees` of a method binds `self.f()` to its own class's f
              when f's name is ambiguous in the repository
  explore     a definition's doc-comment first sentence (its own name removed)
              retrieves that definition / its file in the top k
Tasks (OKF):
  concept, section lookup; links_to / cites pairs; explore by description.

No timing is recorded: this measures accuracy only.
"""
import argparse
import builtins
import json
import os
import random
import re
import shutil
import subprocess
import sys
import time
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from hostclient import Host, HostStartError  # noqa: E402

HOST = Path(os.environ.get("ACCURACY_HOST", "/tmp/accuracy-host-target/release/accuracy-host"))
RUST_ORACLE = Path(os.environ.get("RUST_ORACLE", "/tmp/rust-oracle-target/release/rust-oracle"))
EXCLUDE_ABORTING = False
PRESEEDED_EXCLUDES = {}
STORES = Path(os.environ.get("ACCURACY_STORES", "/tmp/gs-accuracy-stores"))

LANGS = {"python": "python", "type_script": "ts", "java_script": "ts", "rust": "rust"}
SAMPLE = {"extraction_files": 60, "symbol": 300, "callers": 150, "binding": 150, "explore": 150,
          "sections": 200}
K = 8
PRECISION_KINDS = {
    "rust": {"function", "method", "struct", "enum", "trait", "type_alias", "const", "static", "macro"},
    "python": {"function", "method", "class"},
    "ts": {"function", "method", "class", "interface", "type_alias", "enum"},
}
RUST_STD = set("""
to_string to_owned as_str as_ref as_mut as_slice as_bytes into_iter iter_mut unwrap_or
unwrap_or_else unwrap_or_default map_err ok_or ok_or_else and_then or_else is_some is_none
is_ok is_err is_empty push_str to_vec to_lowercase to_uppercase starts_with ends_with
split_whitespace trim_start trim_end find_map filter_map flat_map for_each read_to_string
write_all from_str from_utf8 to_path_buf file_name with_capacity get_mut or_insert
or_insert_with or_default extend_from_slice sort_by sort_by_key sort_unstable sort_unstable_by
binary_search last_mut first_mut split_at try_into try_from into_inner borrow_mut as_deref
as_deref_mut unwrap_err expect_err take_while skip_while step_by max_by_key min_by_key
char_indices split_once rsplit_once strip_prefix strip_suffix to_ascii_lowercase
eq_ignore_ascii_case create_dir_all remove_file read_dir as_millis as_secs duration_since
spawn_blocking block_on try_recv try_send from_secs from_millis is_dir is_file file_stem
set_extension with_extension to_str to_string_lossy as_os_str into_boxed_str into_bytes
as_mut_ptr get_or_insert_with get_or_init contains_key remove_entry values_mut into_keys
into_values retain_mut dedup_by_key chunks_exact split_terminator rsplit_terminator
trim_matches trim_start_matches trim_end_matches parse_terminated to_le_bytes to_be_bytes
from_le_bytes from_be_bytes saturating_sub saturating_add checked_add checked_sub
checked_mul wrapping_add wrapping_sub leading_zeros trailing_zeros count_ones
is_ascii_digit is_alphanumeric is_ascii_alphanumeric is_whitespace is_uppercase
is_lowercase new_v4 from_fn read_line write_fmt write_str to_writer from_reader
from_slice to_vec_pretty to_string_pretty from_value to_value
""".split())


def python_builtin_names():
    import collections, itertools, functools, pathlib, logging, asyncio, subprocess as sp
    import unittest, datetime, json as js, re as rx, os as o, os.path as op, typing, io
    names = set(dir(builtins))
    for obj in (str, bytes, list, dict, set, frozenset, tuple, int, float, object, type,
                collections, collections.OrderedDict, collections.defaultdict, collections.Counter,
                collections.deque, itertools, functools, pathlib.Path, logging, logging.Logger,
                asyncio, asyncio.AbstractEventLoop, sp, unittest.TestCase, datetime.datetime,
                datetime.date, js, rx, rx.compile(""), o, op, typing, io.StringIO,
                io.TextIOWrapper):
        names.update(dir(obj))
    return names


def distinctive(name):
    core = name.strip("_")
    if len(name) < 6 or name.startswith("__"):
        return False
    return "_" in core or bool(re.search(r"[a-z][A-Z]", name))


def minified(root, rel):
    try:
        with open(root / rel, "rb") as handle:
            head = handle.read(200_000)
    except OSError:
        return True
    lines = head.split(b"\n")
    return any(len(line) > 2000 for line in lines) or (len(head) / max(1, len(lines))) > 300


def run_oracle(kind, root, files):
    request = json.dumps({"root": str(root), "files": files})
    if kind == "python":
        command = [sys.executable, str(HERE / "oracles/python_oracle.py")]
    elif kind == "ts":
        command = ["node", "--max-old-space-size=8192", str(HERE / "oracles/ts_oracle.cjs")]
    elif kind == "rust":
        command = [str(RUST_ORACLE)]
    else:
        raise ValueError(kind)
    result = subprocess.run(command, input=request, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


# Byte-identical files are indistinguishable: a path maps to the first path
# (in sorted order) with the same content. Filled per repository.
CANONICAL = {}


def same_file(a, b):
    return a == b or CANONICAL.get(a, a) == CANONICAL.get(b, b)


def canonicalize(root, paths):
    import hashlib
    CANONICAL.clear()
    first = {}
    for path in sorted(paths):
        try:
            digest = hashlib.sha256((root / path).read_bytes()).hexdigest()
        except OSError:
            continue
        CANONICAL[path] = first.setdefault(digest, path)


def contains(node, path, name, line):
    return (same_file(node["path"], path) and node["name"] == name
            and node["start_line"] <= line <= node["end_line"])


def rank_of(nodes, predicate):
    for position, node in enumerate(nodes, 1):
        if predicate(node):
            return position
    return None


def rank_metrics(ranks, total):
    """hit@1/@3/@k and MRR over `total` queries (None = miss)."""
    if total == 0:
        return {"n": 0}
    found = [r for r in ranks if r is not None]
    return {
        "n": total,
        "hit@1": sum(r <= 1 for r in found) / total,
        "hit@3": sum(r <= 3 for r in found) / total,
        f"hit@{K}": sum(r <= K for r in found) / total,
        "found": len(found) / total,
        "mrr": sum(1 / r for r in found) / total,
    }


def nl_query(doc, name):
    """First sentence of a doc comment, markup stripped, own name removed."""
    text = doc.strip().split("\n\n")[0]
    text = re.sub(r"\{@\w+\s+([^}]*)\}", r"\1", text)
    text = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[`?([^\]`]*)`?\]", r"\1", text)
    text = re.split(r"(?<=[.!?])\s|\n\s*[@:]|\n\s*(?:Args|Returns|Raises|Parameters)\b", text)[0]
    words = re.findall(r"[A-Za-z][A-Za-z0-9_']*", text)
    words = [w for w in words if w.lower() != name.lower()]
    return " ".join(words[:24]) if len(words) >= 4 else None


# ---------------------------------------------------------------------------
# Code tasks


def score_extraction(host, files, defs_by_file, all_defs_by_file, lang, rng, records):
    """Per sampled file: oracle definitions found (recall) and graph-search
    definitions the oracle confirms (precision).

    The file's symbols come from `neighbors file:<path> --rel contains`. That
    answer has a byte cap; a definition missing from a truncated listing, or
    one not reachable over `contains`, is looked up with `symbol` before it
    counts as missed (the latter is reported as a containment gap). Precision
    is computed over untruncated listings only.
    """
    candidates = sorted(p for p in files if defs_by_file.get(p))
    sample = rng.sample(candidates, min(SAMPLE["extraction_files"], len(candidates)))
    found_total = total_oracle = tp_precision = total_gs = containment_gaps = truncated_files = 0
    kinds = PRECISION_KINDS[lang]
    for path in sample:
        answer = host.ask(mode="neighbors", target=f"file:{path}", rel="contains", hops=4, limit=500)
        if "error" in answer:
            records.append({"task": "extraction", "path": path, "error": answer["error"]})
            continue
        truncated = bool(answer.get("truncations"))
        truncated_files += truncated
        nodes = [n for n in answer["nodes"] if n["path"] == path]
        oracle = defs_by_file[path]
        every = all_defs_by_file[path]
        missed, gaps = [], []
        for d in oracle:
            if any(contains(n, path, d["name"], d["line"]) for n in nodes):
                continue
            lookup = host.ask(mode="symbol", target=d["name"], limit=500).get("nodes", [])
            if any(contains(n, path, d["name"], d["line"]) for n in lookup):
                if not truncated:
                    gaps.append(d)
                continue
            missed.append(d)
        total_oracle += len(oracle)
        found_total += len(oracle) - len(missed)
        containment_gaps += len(gaps)
        spurious = []
        if not truncated:
            gs_defs = [n for n in nodes if n["kind"] in kinds]
            spurious = [n for n in gs_defs
                        if not any(contains(n, path, d["name"], d["line"]) for d in every)]
            total_gs += len(gs_defs)
            tp_precision += len(gs_defs) - len(spurious)
        if missed or spurious or gaps:
            records.append({
                "task": "extraction", "path": path, "oracle": len(oracle), "truncated": truncated,
                "missed": [f"{d['kind']} {d['qual']}@{d['line']}" for d in missed[:20]],
                "containment_gaps": [f"{d['kind']} {d['qual']}@{d['line']}" for d in gaps[:20]],
                "spurious": [f"{n['kind']} {n['qualified_name']}@{n['start_line']}" for n in spurious[:20]],
            })
    return {
        "files": len(sample),
        "truncated_listings": truncated_files,
        "oracle_defs": total_oracle,
        "recall": found_total / total_oracle if total_oracle else None,
        "containment_gaps": containment_gaps,
        "gs_defs_checked": total_gs,
        "precision": tp_precision / total_gs if total_gs else None,
    }


def score_symbol(host, comparable, name_counts, rng, records):
    sample = rng.sample(comparable, min(SAMPLE["symbol"], len(comparable)))
    ranks, unique_ranks, by_kind = [], [], defaultdict(list)
    for d in sample:
        answer = host.ask(mode="symbol", target=d["name"], limit=500)
        nodes = answer.get("nodes", [])
        r = rank_of(nodes, lambda n: contains(n, d["path"], d["name"], d["line"]))
        ranks.append(r)
        by_kind[d["kind"]].append(r)
        unique = name_counts[d["name"]] == 1
        if unique:
            unique_ranks.append(r)
        if r != 1:
            records.append({"task": "symbol", "def": f"{d['kind']} {d['qual']}", "path": d["path"],
                            "line": d["line"], "rank": r, "unique_name": unique,
                            "error": answer.get("error"),
                            "top": [f"{n['kind']} {n['qualified_name']} {n['path']}:{n['start_line']}" for n in nodes[:3]]})
    return {
        "all": rank_metrics(ranks, len(ranks)),
        "unique_names": rank_metrics(unique_ranks, len(unique_ranks)),
        "by_kind": {kind: rank_metrics(rs, len(rs)) for kind, rs in sorted(by_kind.items())},
    }


def score_callers(host, comparable, name_counts, calls_by_name, stop, covered, rng, records):
    targets = [d for d in comparable
               if d["kind"] in ("function", "method") and name_counts[d["name"]] == 1
               and distinctive(d["name"]) and d["name"] not in stop
               and any(c["recv"] != ("none" if d["kind"] == "method" else "self")
                       for c in calls_by_name.get(d["name"], []))]
    sample = rng.sample(targets, min(SAMPLE["callers"], len(targets)))
    sites_total = sites_hit = gs_total = gs_correct = unresolved = out_of_scope = 0
    by_recv = defaultdict(lambda: [0, 0])
    for d in sample:
        found = host.ask(mode="symbol", target=d["name"], limit=500)
        node = next((n for n in found.get("nodes", []) if contains(n, d["path"], d["name"], d["line"])), None)
        if node is None:
            unresolved += 1
            records.append({"task": "callers", "def": d["qual"], "path": d["path"], "unresolved": True})
            continue
        answer = host.ask(mode="callers", target=node["id"], limit=500)
        if "error" in answer:
            unresolved += 1
            records.append({"task": "callers", "def": d["qual"], "error": answer["error"]})
            continue
        by_id = {n["id"]: n for n in answer["nodes"]}
        callers = [by_id[e["from"]] for e in answer["edges"]
                   if e["kind"] == "calls" and e.get("to") == node["id"] and e["from"] in by_id]
        callers = list({c["id"]: c for c in callers}.values())
        # Callers in files the oracle cannot read (e.g. Svelte scripts) are
        # neither confirmed nor refuted.
        out_of_scope += sum(c["path"] not in covered for c in callers)
        callers = [c for c in callers if c["path"] in covered]
        # A bare call cannot reach a method, nor a receiver call on `self` a free function.
        excluded = "none" if d["kind"] == "method" else "self"
        sites = [c for c in calls_by_name[d["name"]] if c["recv"] != excluded]
        if not sites:
            continue

        def matches(caller, site):
            if caller["path"] != site["path"]:
                return False
            if caller["kind"] in ("file", "module") and caller["start_line"] in (0, 1):
                return site["toplevel"] or not site["chain"]
            return (caller["start_line"] <= site["line"] <= caller["end_line"]
                    and caller["name"] in site["chain"])

        missed = []
        for site in sites:
            hit = any(matches(c, site) for c in callers)
            sites_hit += hit
            bucket = "macro_arg" if site.get("in_macro") else site["recv"]
            by_recv[bucket][0] += hit
            by_recv[bucket][1] += 1
            if not hit:
                missed.append(site)
        wrong = [c for c in callers if not any(matches(c, s) for s in sites)]
        sites_total += len(sites)
        gs_total += len(callers)
        gs_correct += len(callers) - len(wrong)
        if missed or wrong:
            records.append({
                "task": "callers", "def": f"{d['kind']} {d['qual']}", "path": d["path"],
                "sites": len(sites), "missed": [f"{s['path']}:{s['line']} recv={s['recv']} in={s['chain'][:1]}" for s in missed[:10]],
                "spurious": [f"{c['kind']} {c['qualified_name']} {c['path']}:{c['start_line']}" for c in wrong[:10]],
            })
    return {
        "targets": len(sample), "eligible": len(targets), "unresolved_targets": unresolved,
        "sites": sites_total,
        "site_recall": sites_hit / sites_total if sites_total else None,
        "gs_callers": gs_total,
        "gs_callers_out_of_oracle_scope": out_of_scope,
        "caller_precision": gs_correct / gs_total if gs_total else None,
        "recall_by_site_kind": {k: {"n": v[1], "recall": v[0] / v[1]} for k, v in sorted(by_recv.items())},
    }


def score_binding(host, lang, defs, calls, name_counts, rng, records):
    """`self.f()` / `this.f()` inside a method of X, where X itself defines f
    and f's name is defined more than once in the repository: `callees` of the
    enclosing method must bind the call to X's f, not another f."""
    sep = "::" if lang == "rust" else "."

    def key(d):
        # Rust: an impl's owner is its type's last path segment.
        return "::".join(d["qual"].split("::")[-2:]) if lang == "rust" else d["qual"]

    by_key = defaultdict(list)
    for d in defs:
        if d["comparable"] and d["kind"] in ("function", "method"):
            by_key[key(d)].append(d)
    pairs = {}
    for c in calls:
        if c["recv"] != "self" or not c.get("owner") or not c["chain"] or name_counts[c["name"]] < 2:
            continue
        expected = by_key.get(f"{c['owner']}{sep}{c['name']}", [])
        enclosing = by_key.get(f"{c['owner']}{sep}{c['chain'][0]}", [])
        if len(expected) == 1 and len(enclosing) == 1 and enclosing[0]["path"] == c["path"]:
            pairs.setdefault((key(enclosing[0]), c["name"]), (enclosing[0], expected[0]))
    eligible = sorted(pairs)
    sample = rng.sample(eligible, min(SAMPLE["binding"], len(eligible)))
    correct = wrong = unresolved = 0
    for pair in sample:
        enclosing, expected = pairs[pair]
        found = host.ask(mode="symbol", target=enclosing["name"], limit=500).get("nodes", [])
        node = next((n for n in found if contains(n, enclosing["path"], enclosing["name"], enclosing["line"])), None)
        if node is None:
            unresolved += 1
            continue
        answer = host.ask(mode="callees", target=node["id"], limit=500)
        by_id = {n["id"]: n for n in answer.get("nodes", [])}
        targets = [by_id[e["to"]] for e in answer.get("edges", [])
                   if e["kind"] == "calls" and e.get("from") == node["id"] and e.get("to") in by_id
                   and by_id[e["to"]]["name"] == expected["name"]]
        if any(contains(t, expected["path"], expected["name"], expected["line"]) for t in targets):
            correct += 1
        elif targets:
            wrong += 1
            records.append({"task": "binding", "in": enclosing["qual"], "call": expected["name"],
                            "expected": f"{expected['qual']} {expected['path']}:{expected['line']}",
                            "bound": [f"{t['qualified_name']} {t['path']}:{t['start_line']}" for t in targets]})
        else:
            unresolved += 1
            records.append({"task": "binding", "in": enclosing["qual"], "call": expected["name"],
                            "expected": f"{expected['qual']} {expected['path']}:{expected['line']}", "bound": []})
    n = len(sample)
    return {"eligible": len(eligible), "n": n, "correct": correct, "wrong": wrong, "unresolved": unresolved,
            "recall": correct / n if n else None,
            "precision": correct / (correct + wrong) if correct + wrong else None}


def score_explore(host, comparable, rng, records):
    candidates = []
    for d in comparable:
        query = nl_query(d.get("doc") or "", d["name"])
        if query:
            candidates.append((d, query))
    sample = rng.sample(candidates, min(SAMPLE["explore"], len(candidates)))
    symbol_ranks, file_ranks = [], []
    for d, query in sample:
        answer = host.ask(mode="explore", query=query, k=K)
        items = answer.get("items", [])
        sr = rank_of(items, lambda n: contains(n, d["path"], d["name"], d["line"]))
        fr = rank_of(items, lambda n: same_file(n["path"], d["path"]))
        symbol_ranks.append(sr)
        file_ranks.append(fr)
        if sr != 1:
            records.append({"task": "explore", "def": f"{d['kind']} {d['qual']}", "path": d["path"],
                            "query": query, "symbol_rank": sr, "file_rank": fr, "error": answer.get("error"),
                            "top": [f"{n['kind']} {n['qualified_name']} {n['path']}" for n in items[:3]]})
    return {"eligible": len(candidates), "symbol": rank_metrics(symbol_ranks, len(sample)),
            "file": rank_metrics(file_ranks, len(sample))}


def code_oracle(root, lang, files):
    usable = [p for p in files if not (lang == "ts" and minified(root, p))]
    oracle = run_oracle(lang, root, usable)
    hard_errors = {e["path"] for e in oracle["errors"] if not str(e["error"]).startswith("parse:")}
    oracle["defs"] = [d for d in oracle["defs"] if d["path"] not in hard_errors]
    oracle["calls"] = [c for c in oracle["calls"] if c["path"] not in hard_errors]
    oracle["usable"] = usable
    oracle["hard_errors"] = hard_errors
    return oracle


def code_language(host, lang, files, oracle, name_counts, rng, stop):
    """Scores one language. `name_counts` spans every language in the repo:
    `symbol` searches all of them."""
    usable = oracle["usable"]
    defs = oracle["defs"]
    if lang == "ts":
        stop = stop | set(oracle.get("builtins", []))
    # Queries are sampled from hand-written code: ambient `.d.ts` declarations
    # (mostly generated, often duplicated per package) still count for name
    # uniqueness and as legitimate definitions, but are not sampled.
    comparable = [d for d in defs if d["comparable"] and not d["path"].endswith(".d.ts")]
    defs_by_file, all_by_file = defaultdict(list), defaultdict(list)
    for d in defs:
        all_by_file[d["path"]].append(d)
        if d["comparable"] and not d["path"].endswith(".d.ts"):
            defs_by_file[d["path"]].append(d)
    calls_by_name = defaultdict(list)
    for c in oracle["calls"]:
        calls_by_name[c["name"]].append(c)
    records = []
    summary = {
        "files_walked": len(files), "files_oracle": len(usable), "minified_skipped": len(files) - len(usable),
        "oracle_errors": len(oracle["errors"]), "oracle_hard_errors": len(oracle["hard_errors"]),
        "defs": len(defs), "comparable_defs": len(comparable), "calls": len(oracle["calls"]),
        "dts_defs_not_sampled": sum(d["comparable"] and d["path"].endswith(".d.ts") for d in defs),
    }
    summary["extraction"] = score_extraction(host, usable, defs_by_file, all_by_file, lang, rng, records)
    summary["symbol"] = score_symbol(host, comparable, name_counts, rng, records)
    summary["callers"] = score_callers(host, comparable, name_counts, calls_by_name, stop, set(usable), rng, records)
    summary["binding"] = score_binding(host, lang, defs, oracle["calls"], name_counts, rng, records)
    summary["explore"] = score_explore(host, comparable, rng, records)
    return summary, records


# ---------------------------------------------------------------------------
# OKF tasks


def okf_language(host, root, files, walked, rng):
    request = json.dumps({"root": str(root), "files": files, "walked": walked})
    result = subprocess.run([sys.executable, str(HERE / "oracles/okf_oracle.py")], input=request,
                            capture_output=True, text=True, check=True)
    oracle = json.loads(result.stdout)
    records = []
    summary = {"files": len(files), "concepts": len(oracle["concepts"]), "sections": len(oracle["sections"]),
               "oracle_links": len(oracle["links"]), "oracle_errors": len(oracle["errors"])}

    # Concept lookup by title.
    ranks = []
    for c in oracle["concepts"]:
        answer = host.ask(mode="symbol", target=c["title"], limit=500)
        nodes = answer.get("nodes", [])
        r = rank_of(nodes, lambda n: n["kind"] == "concept" and n["path"] == c["path"])
        ranks.append(r)
        if r != 1:
            records.append({"task": "concept", "title": c["title"], "path": c["path"], "rank": r,
                            "error": answer.get("error"),
                            "top": [f"{n['kind']} {n['name']} {n['path']}" for n in nodes[:3]]})
    summary["concept_lookup"] = rank_metrics(ranks, len(ranks))

    # Section lookup by heading text.
    sections = [s for s in oracle["sections"] if s["name"]]
    sample = rng.sample(sections, min(SAMPLE["sections"], len(sections)))
    ranks = []
    for s in sample:
        answer = host.ask(mode="symbol", target=s["name"], limit=500)
        nodes = answer.get("nodes", [])
        r = rank_of(nodes, lambda n: n["kind"] == "section" and n["path"] == s["path"]
                    and n["start_line"] == s["line"])
        ranks.append(r)
        if r is None:
            records.append({"task": "section", "name": s["name"], "path": s["path"], "line": s["line"],
                            "error": answer.get("error"),
                            "top": [f"{n['kind']} {n['qualified_name']} {n['path']}:{n['start_line']}" for n in nodes[:3]]})
    summary["section_lookup"] = rank_metrics(ranks, len(ranks))

    # links_to / cites pairs, collected from each target's incoming edges.
    oracle_pairs = defaultdict(set)
    for link in oracle["links"]:
        if link["source"] != link["target"]:
            oracle_pairs[link["kind"]].add((link["source"], link["target"]))
    gs_pairs = defaultdict(set)
    for target in sorted(set(walked)):
        contained = host.ask(mode="neighbors", target=f"file:{target}", rel="contains", hops=1, limit=50)
        concept = next((n for n in contained.get("nodes", []) if n["kind"] == "concept" and n["path"] == target), None)
        file_id = f"file:{target}"
        ids = {file_id}
        answers = [host.ask(mode="deps", target=target, limit=500)]
        if concept:
            ids.add(concept["id"])
            for rel in ("links_to", "cites"):
                answers.append(host.ask(mode="neighbors", target=concept["id"], rel=rel, hops=1, limit=500))
        for answer in answers:
            for e in answer.get("edges", []):
                if e["kind"] in ("links_to", "cites") and e.get("resolved") and e.get("to") in ids and e.get("path"):
                    if e["path"] != target:
                        gs_pairs[e["kind"]].add((e["path"], target))
    links = {}
    for kind in ("links_to", "cites"):
        o, g = oracle_pairs[kind], gs_pairs[kind]
        links[kind] = {"oracle": len(o), "gs": len(g),
                       "recall": len(o & g) / len(o) if o else None,
                       "precision": len(o & g) / len(g) if g else None}
        for pair in sorted(o - g)[:30]:
            records.append({"task": kind, "missed": pair})
        for pair in sorted(g - o)[:30]:
            records.append({"task": kind, "spurious": pair})
    summary["links"] = links

    # Explore by description.
    candidates = [c for c in oracle["concepts"] if len(c["description"].split()) >= 4]
    concept_ranks, file_ranks = [], []
    for c in candidates:
        answer = host.ask(mode="explore", query=c["description"], k=K)
        items = answer.get("items", [])
        cr = rank_of(items, lambda n: n["kind"] == "concept" and n["path"] == c["path"])
        fr = rank_of(items, lambda n: n["path"] == c["path"])
        concept_ranks.append(cr)
        file_ranks.append(fr)
        if fr != 1:
            records.append({"task": "explore", "title": c["title"], "query": c["description"],
                            "concept_rank": cr, "file_rank": fr, "error": answer.get("error"),
                            "top": [f"{n['kind']} {n['name']} {n['path']}" for n in items[:3]]})
    summary["explore"] = {"concept": rank_metrics(concept_ranks, len(candidates)),
                          "file": rank_metrics(file_ranks, len(candidates))}
    return summary, records


# ---------------------------------------------------------------------------


def run_repo(name, root, out_dir, stop_python, okf_repos):
    store = STORES / name
    shutil.rmtree(store, ignore_errors=True)
    started = time.time()
    # A file whose retrieval facts fail store validation aborts the whole
    # index. With --exclude-aborting, directories named like that file's
    # parent are excluded and indexing retried; exclusions are reported.
    excluded = list(PRESEEDED_EXCLUDES.get(name, []))
    while True:
        try:
            host = Host(HOST, root, store, excluded)
            break
        except HostStartError as error:
            match = re.search(r"source retrieval facts do not match (.+?)\"\)", str(error))
            if not (EXCLUDE_ABORTING and match and len(excluded) < 25):
                raise
            # Excludes match directory names only, so the parent directory goes.
            parent = Path(match.group(1)).parent.name
            if not parent or parent in excluded:
                raise
            excluded.append(parent)
            print(f"   excluding directories named {parent!r} (for {match.group(1)})", flush=True)
            shutil.rmtree(store, ignore_errors=True)
    try:
        ready = host.ready
        files = host.ask(mode="files")["files"]
        by_lang = defaultdict(list)
        for f in files:
            by_lang[f["language"]].append(f["path"])
        result = {
            "repo": name, "root": str(root), "excluded_directory_names": excluded,
            "index": {"counts": ready["index"].get("counts"), "coverage": ready["index"].get("coverage"),
                      "quarantined": len(ready["index"].get("quarantined") or [])},
            "walked": {str(k): len(v) for k, v in sorted(by_lang.items(), key=lambda kv: str(kv[0]))},
            "languages": {},
        }
        all_records = {}
        grouped = defaultdict(list)
        for language, paths in by_lang.items():
            if language in LANGS:
                grouped[LANGS[language]].extend(paths)
        canonicalize(Path(root), [p for paths in grouped.values() for p in paths])
        oracles = {lang: code_oracle(Path(root), lang, sorted(paths)) for lang, paths in grouped.items()}
        name_counts = Counter(d["name"] for o in oracles.values() for d in o["defs"])
        for lang, paths in sorted(grouped.items()):
            rng = random.Random(f"{name}:{lang}:1729")
            stop = stop_python if lang == "python" else RUST_STD if lang == "rust" else set()
            summary, records = code_language(host, lang, sorted(paths), oracles[lang], name_counts, rng, stop)
            result["languages"][lang] = summary
            all_records[lang] = records
        if by_lang.get("okf") and name in okf_repos:
            rng = random.Random(f"{name}:okf:1729")
            summary, records = okf_language(host, Path(root), sorted(by_lang["okf"]),
                                            sorted(f["path"] for f in files), rng)
            result["languages"]["okf"] = summary
            all_records["okf"] = records
    finally:
        host.close()
        shutil.rmtree(store, ignore_errors=True)
    result["wall_s"] = round(time.time() - started, 1)
    (out_dir / f"{name}.json").write_text(json.dumps(result, indent=2) + "\n")
    (out_dir / f"{name}.failures.json").write_text(json.dumps(all_records, indent=1) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repos", type=Path, required=True, help="JSON {name: absolute root}")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--only", nargs="*")
    parser.add_argument("--exclude-aborting", action="store_true",
                        help="exclude files that abort indexing and retry")
    parser.add_argument("--exclude-dir", nargs="*", default=[], metavar="REPO:DIRNAME",
                        help="directory names to exclude up front (found by an earlier --exclude-aborting run)")
    parser.add_argument("--okf", nargs="*", default=["clm-research"],
                        help="repositories whose OKF bundle is scored")
    args = parser.parse_args()
    global EXCLUDE_ABORTING
    EXCLUDE_ABORTING = args.exclude_aborting
    for item in args.exclude_dir:
        repo, _, directory = item.partition(":")
        PRESEEDED_EXCLUDES.setdefault(repo, []).append(directory)
    repos = json.loads(args.repos.read_text())
    args.out.mkdir(parents=True, exist_ok=True)
    stop_python = python_builtin_names()
    for name, root in repos.items():
        if args.only and name not in args.only:
            continue
        print(f"== {name}", flush=True)
        try:
            result = run_repo(name, root, args.out, stop_python, set(args.okf))
            print(json.dumps({lang: {k: v for k, v in s.items() if k in ("comparable_defs", "concepts")}
                              for lang, s in result["languages"].items()}), f"{result['wall_s']}s", flush=True)
        except Exception as error:  # keep going; record the failure
            print(f"!! {name}: {str(error)[:300]}", flush=True)
            (args.out / f"{name}.error.txt").write_text(repr(error) + "\n")


if __name__ == "__main__":
    main()
