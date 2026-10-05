#!/usr/bin/env python3
"""Aggregates run.py output into summary.json and REPORT.md.

Pooled figures are micro-averages: every scored query (or definition, or call
site) counts once, whichever repository it came from. Per-repository rows show
the sample size beside each figure; small samples are noisy.
"""
import argparse
import json
from collections import defaultdict
from pathlib import Path

K = 8
LANG_NAMES = {"python": "Python", "ts": "TypeScript/JavaScript", "rust": "Rust"}


def pct(value):
    return "–" if value is None else f"{100 * value:.1f}"


class Pool:
    """Sums of hits over totals, keyed by metric name."""

    def __init__(self):
        self.hits = defaultdict(float)
        self.totals = defaultdict(float)

    def add(self, key, rate, total):
        if rate is None or not total:
            return
        self.hits[key] += rate * total
        self.totals[key] += total

    def add_ranks(self, prefix, metrics):
        n = metrics.get("n", 0)
        for name in ("hit@1", "hit@3", f"hit@{K}", "mrr"):
            if name in metrics:
                self.add(f"{prefix}.{name}", metrics[name], n)

    def rate(self, key):
        total = self.totals.get(key)
        return self.hits[key] / total if total else None

    def n(self, key):
        return int(self.totals.get(key, 0))

    def as_dict(self):
        return {key: {"rate": self.rate(key), "n": self.n(key)} for key in sorted(self.totals)}


def pool_language(pool, s):
    e = s["extraction"]
    pool.add("extraction.recall", e["recall"], e["oracle_defs"])
    pool.add("extraction.precision", e["precision"], e["gs_defs_checked"])
    pool.add("extraction.containment_gap", (e["containment_gaps"] / e["oracle_defs"]) if e["oracle_defs"] else None,
             e["oracle_defs"])
    pool.add_ranks("symbol.all", s["symbol"]["all"])
    pool.add("symbol.all.found", s["symbol"]["all"].get("found"), s["symbol"]["all"].get("n", 0))
    pool.add_ranks("symbol.unique", s["symbol"]["unique_names"])
    for kind, metrics in s["symbol"]["by_kind"].items():
        pool.add(f"symbol.kind.{kind}.hit@1", metrics.get("hit@1"), metrics.get("n", 0))
        pool.add(f"symbol.kind.{kind}.found", metrics.get("found"), metrics.get("n", 0))
    c = s["callers"]
    pool.add("callers.site_recall", c["site_recall"], c["sites"])
    pool.add("callers.precision", c["caller_precision"], c["gs_callers"])
    pool.add("callers.targets", 1.0, c["targets"])
    for kind, metrics in c.get("recall_by_site_kind", {}).items():
        pool.add(f"callers.site_kind.{kind}", metrics["recall"], metrics["n"])
    b = s.get("binding", {})
    if b.get("n"):
        pool.add("binding.recall", b["recall"], b["n"])
        pool.add("binding.precision", b["precision"], b["correct"] + b["wrong"])
    pool.add_ranks("explore.symbol", s["explore"]["symbol"])
    pool.add_ranks("explore.file", s["explore"]["file"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results", type=Path)
    parser.add_argument("--exclude-from-pool", nargs="*", default=[],
                        help="repositories shown per-repo but left out of pooled figures")
    args = parser.parse_args()
    repos = []
    for path in sorted(args.results.glob("*.json")):
        if path.name.endswith(".failures.json") or path.name == "summary.json":
            continue
        data = json.loads(path.read_text())
        if isinstance(data, dict) and "languages" in data:
            repos.append(data)
    errors = sorted(p.stem.replace(".error", "") for p in args.results.glob("*.error.txt"))
    pools = defaultdict(Pool)
    for repo in repos:
        if repo["repo"] in args.exclude_from_pool:
            continue
        for lang, s in repo["languages"].items():
            if lang != "okf":
                pool_language(pools[lang], s)
                pool_language(pools["all"], s)
    okf = {repo["repo"]: repo["languages"]["okf"] for repo in repos if "okf" in repo["languages"]}
    summary = {"repos": [r["repo"] for r in repos], "failed": errors,
               "excluded_from_pool": args.exclude_from_pool,
               "pooled": {lang: pool.as_dict() for lang, pool in pools.items()}, "okf": okf}
    (args.results / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")

    lines = ["# graph-search accuracy benchmark", ""]
    lines.append(f"Repositories scored: {len(repos)}" + (f"; failed: {', '.join(errors)}" if errors else "") + ".")
    if args.exclude_from_pool:
        lines.append(f"Left out of pooled figures: {', '.join(args.exclude_from_pool)} (marked † below).")
    dropped = {r["repo"]: r.get("excluded_directory_names") for r in repos if r.get("excluded_directory_names")}
    for name, dirs in dropped.items():
        lines.append(f"{name}: indexed with directories named {', '.join(dirs)} excluded (a file in each aborts indexing).")
    lines.append("")
    lines.append("## Pooled results (micro-averaged, %)")
    lines.append("")
    order = [l for l in ("python", "ts", "rust", "all") if l in pools]
    header = "| Metric | " + " | ".join(LANG_NAMES.get(l, "All code") for l in order) + " |"
    lines += [header, "|---|" + "---|" * len(order)]
    rows = [
        ("Extraction recall", "extraction.recall"),
        ("Extraction precision", "extraction.precision"),
        ("Symbol lookup: found in results", "symbol.all.found"),
        ("Symbol lookup: rank 1 (all names)", "symbol.all.hit@1"),
        ("Symbol lookup: rank 1 (repo-unique names)", "symbol.unique.hit@1"),
        ("Callers: call-site recall", "callers.site_recall"),
        ("Callers: caller precision", "callers.precision"),
        ("  recall, bare calls `f()`", "callers.site_kind.none"),
        ("  recall, `self.f()` / `this.f()` / `Self::f()`", "callers.site_kind.self"),
        ("  recall, path calls `Type::f()` (Rust)", "callers.site_kind.path"),
        ("  recall, other receivers `x.f()`", "callers.site_kind.other"),
        ("  recall, inside macro arguments (Rust)", "callers.site_kind.macro_arg"),
        ("Binding, ambiguous `self.f()`: correct target", "binding.recall"),
        ("Binding, ambiguous `self.f()`: precision when bound", "binding.precision"),
        ("Explore (doc query): definition @1", "explore.symbol.hit@1"),
        (f"Explore (doc query): definition @{K}", f"explore.symbol.hit@{K}"),
        ("Explore (doc query): definition MRR", "explore.symbol.mrr"),
        (f"Explore (doc query): file @{K}", f"explore.file.hit@{K}"),
    ]
    for label, key in rows:
        cells = []
        for lang in order:
            rate, n = pools[lang].rate(key), pools[lang].n(key)
            cells.append(f"{pct(rate)} (n={n})" if n else "–")
        lines.append(f"| {label} | " + " | ".join(cells) + " |")
    lines.append("")

    lines.append("## Per repository")
    lines.append("")
    lines.append("Extraction R/P · symbol found / rank-1 on repo-unique names · callers site recall / precision · "
                 f"ambiguous self-call binding recall / precision · explore definition @1 / @{K} · n = scored items.")
    lines.append("")
    lines.append(f"| Repo | Lang | Defs | Extract R / P | Symbol found / @1 uniq | Callers R / P (sites) | Binding R / P (n) | Explore @1 / @{K} (n) |")
    lines.append("|---|---|---|---|---|---|---|---|")
    for repo in repos:
        for lang, s in sorted(repo["languages"].items()):
            if lang == "okf":
                continue
            e, sy, c, ex = s["extraction"], s["symbol"], s["callers"], s["explore"]["symbol"]
            lines.append(
                f"| {repo['repo']}{' †' if repo['repo'] in args.exclude_from_pool else ''} | {lang} | {s['comparable_defs']} | {pct(e['recall'])} / {pct(e['precision'])} "
                f"| {pct(sy['all'].get('found'))} / {pct(sy['unique_names'].get('hit@1'))} (n={sy['all'].get('n', 0)}) "
                f"| {pct(c['site_recall'])} / {pct(c['caller_precision'])} ({c['sites']}) "
                f"| {pct(s['binding']['recall'])} / {pct(s['binding']['precision'])} ({s['binding']['n']}) "
                f"| {pct(ex.get('hit@1'))} / {pct(ex.get(f'hit@{K}'))} ({ex.get('n', 0)}) |")
    lines.append("")

    for name, s in okf.items():
        lines.append(f"## OKF v0.2: {name}")
        lines.append("")
        lines.append(f"{s['concepts']} concepts, {s['sections']} sections, {s['oracle_links']} oracle links in {s['files']} files.")
        lines.append("")
        lines.append("| Task | n | @1 | @3 | @8 / found | MRR |")
        lines.append("|---|---|---|---|---|---|")
        for label, m in (("Concept lookup by title", s["concept_lookup"]),
                         ("Section lookup by heading", s["section_lookup"]),
                         ("Explore by description: concept", s["explore"]["concept"]),
                         ("Explore by description: file", s["explore"]["file"])):
            lines.append(f"| {label} | {m.get('n', 0)} | {pct(m.get('hit@1'))} | {pct(m.get('hit@3'))} "
                         f"| {pct(m.get(f'hit@{K}'))} / {pct(m.get('found'))} | {pct(m.get('mrr'))} |")
        lines.append("")
        lines.append("| Relationship | oracle pairs | graph-search pairs | recall | precision |")
        lines.append("|---|---|---|---|---|")
        for kind, m in s["links"].items():
            lines.append(f"| {kind} | {m['oracle']} | {m['gs']} | {pct(m['recall'])} | {pct(m['precision'])} |")
        lines.append("")
    (args.results / "REPORT.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
