"""Engine-independent corpus validation, evidence accounting, and blind grading."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path


def digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


def source_path(root: Path, relative: str) -> Path:
    path = (root / relative).resolve()
    if Path(relative).is_absolute() or not path.is_relative_to(root.resolve()):
        raise ValueError(f"source path escapes repository: {relative}")
    if not path.is_file():
        raise ValueError(f"source file missing: {relative}")
    return path


def validate(tasks: list[dict], oracles: dict, roots: dict[str, Path]) -> None:
    ids, families = set(), {}
    for task in tasks:
        identifier = task["id"]
        if identifier in ids:
            raise ValueError(f"duplicate task: {identifier}")
        ids.add(identifier)
        if set(task) != {"id", "repo", "family", "split", "kind", "prompt"}:
            raise ValueError(f"unexpected public fields: {identifier}")
        if task["split"] not in {"dev", "heldout"} or task["kind"] not in {"debug", "change"}:
            raise ValueError(f"invalid task classification: {identifier}")
        key = task["repo"], task["family"]
        if key in families and families[key] != task["split"]:
            raise ValueError(f"family leaks across splits: {key}")
        families[key] = task["split"]
        oracle = oracles[identifier]
        if not oracle["regions"] or not oracle["criteria"]:
            raise ValueError(f"empty oracle: {identifier}")
        region_ids = set()
        for region in oracle["regions"]:
            if region["id"] in region_ids:
                raise ValueError("duplicate region")
            region_ids.add(region["id"])
            data = source_path(roots[task["repo"]], region["path"]).read_bytes()
            lines = data.decode().splitlines(keepends=True)
            start, end = region["start"], region["end"]
            if not 1 <= start <= end <= len(lines):
                raise ValueError(f"invalid source region: {identifier}")
            if digest(data) != region["file_sha256"] or digest("".join(lines[start-1:end]).encode()) != region["sha256"]:
                raise ValueError(f"source drift: {identifier}: {region['path']}")
        criteria_ids = set()
        for criterion in oracle["criteria"]:
            if criterion["id"] in criteria_ids or not criterion["description"].strip():
                raise ValueError("invalid criterion")
            criteria_ids.add(criterion["id"])
            if not criterion["regions"] or not set(criterion["regions"]) <= region_ids:
                raise ValueError("criterion references missing evidence")
        for relation in oracle.get("relationships", []):
            if not relation["description"] or not set(relation["regions"]) <= region_ids:
                raise ValueError("invalid relationship evidence")
    if ids != set(oracles):
        raise ValueError("task/oracle IDs differ")


def bounded(text: str, limit: int) -> tuple[str, bool]:
    if limit < 0:
        raise ValueError("negative byte budget")
    encoded = text.encode()
    return encoded[:limit].decode("utf-8", errors="ignore"), len(encoded) > limit


def read_source(root: Path, path: str, start: int, count: int = 80) -> str:
    if not isinstance(start, int) or not isinstance(count, int) or start < 1 or not 1 <= count <= 200:
        raise ValueError("read requires start >= 1 and 1 <= count <= 200")
    lines = source_path(root, path).read_text().splitlines()
    return "\n".join(f"{path}:{i+1}\t{line}" for i, line in enumerate(lines) if start-1 <= i < start-1+count)


def delivered_lines(text: str, root: Path) -> set[tuple[str, int]]:
    """Only complete, byte-exact delivered source lines earn coverage, never spans."""
    import re
    result, cache = set(), {}
    for line in text.splitlines():
        match = re.fullmatch(r"(.+):(\d+)\t(.*)", line)
        if not match:
            continue
        path, number, content = match.groups()
        try:
            if path not in cache:
                cache[path] = source_path(root, path).read_text().splitlines()
            number = int(number)
            if number >= 1 and cache[path][number-1] == content:
                result.add((path, number))
        except (ValueError, IndexError, UnicodeError, OSError):
            continue
    return result


def coverage(oracle: dict, seen: set[tuple[str, int]]) -> dict:
    regions = {}
    for region in oracle["regions"]:
        expected = {(region["path"], i) for i in range(region["start"], region["end"]+1)}
        regions[region["id"]] = len(expected & seen) / len(expected)
    required_files = {r["path"] for r in oracle["regions"]}
    seen_files = {p for p, _ in seen}
    relations = oracle.get("relationships", [])
    return {
        "required_file_recall": len(required_files & seen_files) / len(required_files),
        "region_coverage": regions,
        "evidence_ready": all(value == 1 for value in regions.values()),
        "relationship_evidence_recall": (sum(all(regions[r] == 1 for r in rel["regions"]) for rel in relations) / len(relations)) if relations else None,
        "task_success": None,
    }


def grading_packet(task: dict, oracle: dict, trial: dict) -> dict:
    """Arm and backend metadata deliberately excluded from reviewer packet."""
    if not trial.get("answer"):
        raise ValueError("cannot grade a retrieval-only trial")
    packet = {"task": task, "oracle": oracle, "answer": trial["answer"],
              "citations": trial.get("citations", []), "seen": trial["seen"],
              "trial_sha256": digest(canonical(trial).encode())}
    return {**packet, "packet_sha256": digest(canonical(packet).encode())}


def grade(packet: dict, judgments: dict) -> dict:
    payload = {k: v for k, v in packet.items() if k != "packet_sha256"}
    if digest(canonical(payload).encode()) != packet["packet_sha256"]:
        raise ValueError("packet changed")
    if judgments.get("packet_sha256") != packet["packet_sha256"] or not judgments.get("reviewer", "").strip():
        raise ValueError("grade must identify packet and reviewer")
    expected = {c["id"] for c in packet["oracle"]["criteria"]}
    decisions = judgments["criteria"]
    if set(decisions) != expected or any(type(v) is not bool for v in decisions.values()):
        raise ValueError("every rubric criterion needs a boolean judgment")
    if type(judgments.get("unsupported_claims")) is not bool:
        raise ValueError("unsupported claims judgment required")
    seen = {tuple(item) for item in packet["seen"]}
    citations = packet["citations"]
    citations_valid = bool(citations) and all((c["path"], c["line"]) in seen for c in citations)
    return {"task_success": all(decisions.values()) and not judgments["unsupported_claims"] and citations_valid,
            "criterion_fraction": sum(decisions.values()) / len(decisions),
            "citations_valid": citations_valid, "reviewer": judgments["reviewer"],
            "packet_sha256": packet["packet_sha256"]}
