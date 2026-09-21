"""Dependency-free shortest-path feasibility experiment; no production replacement.

Compare complete, already-loaded undirected adjacency. Preserve oriented edge
identities in reconstructed paths. Work counts are prototype adjacency visits,
not production budget units or timing. Exhaustive distance oracle is Floyd-Warshall.
"""
import itertools
import json
import pathlib


def adjacency(n, edges):
    result = [[] for _ in range(n)]
    for index, (a, b) in enumerate(edges):
        result[a].append((b, index))
        result[b].append((a, index))
    return result


def unwind(parent, node):
    result = []
    while node in parent:
        node, edge = parent[node]
        result.append(edge)
    return result[::-1]


def forward(adj, source, target, hops):
    if source == target:
        return [], 0
    frontier, seen, parent, work = {source}, {source}, {}, 0
    for _ in range(hops):
        next_ring = set()
        for node in sorted(frontier):
            for other, edge in adj[node]:
                work += 1
                if other in seen:
                    continue
                seen.add(other)
                parent[other] = node, edge
                if other == target:
                    return unwind(parent, target), work
                next_ring.add(other)
        frontier = next_ring
        if not frontier:
            break
    return None, work


def bidirectional(adj, source, target, hops):
    if source == target:
        return [], 0
    frontier = [{source}, {target}]
    distance = [{source: 0}, {target: 0}]
    parents = [{}, {}]
    depth, best, meeting, work = [0, 0], None, None, 0
    while all(frontier) and sum(depth) < hops:
        side = 0 if len(frontier[0]) <= len(frontier[1]) else 1
        opposite = 1 - side
        next_ring = set()
        for node in sorted(frontier[side]):
            for other, edge in adj[node]:
                work += 1
                if other not in distance[side]:
                    distance[side][other] = depth[side] + 1
                    parents[side][other] = node, edge
                    next_ring.add(other)
                if other in distance[opposite]:
                    length = distance[side][other] + distance[opposite][other]
                    if length <= hops and (best is None or length < best):
                        best, meeting = length, other
        depth[side] += 1
        frontier[side] = next_ring
        if best is not None and sum(depth) >= best:
            left = unwind(parents[0], meeting)
            right = unwind(parents[1], meeting)
            return left + right[::-1], work
    return None, work


def validate(path, edges, source, target, expected):
    assert (None if path is None else len(path)) == expected
    if path is None:
        return
    cursor = source
    visited = {source}
    for index in path:
        a, b = edges[index]
        assert cursor in (a, b)
        cursor = b if cursor == a else a
        assert cursor not in visited
        visited.add(cursor)
    assert cursor == target


def main():
    comparisons, different, example = 0, 0, None
    for n in (4, 5):
        possible = list(itertools.combinations(range(n), 2))
        for mask in range(1 << len(possible)):
            edges = [edge for i, edge in enumerate(possible) if mask & (1 << i)]
            if mask % 2:
                edges.reverse()
            adj = adjacency(n, edges)
            distances = [[0 if a == b else n + 1 for b in range(n)] for a in range(n)]
            for a, b in edges:
                distances[a][b] = distances[b][a] = 1
            for middle in range(n):
                for a in range(n):
                    for b in range(n):
                        distances[a][b] = min(distances[a][b], distances[a][middle] + distances[middle][b])
            for source, target, hops in itertools.product(range(n), range(n), range(n)):
                expected = distances[source][target]
                expected = expected if expected <= hops else None
                first, _ = forward(adj, source, target, hops)
                second, _ = bidirectional(adj, source, target, hops)
                validate(first, edges, source, target, expected)
                validate(second, edges, source, target, expected)
                comparisons += 1
                if first != second:
                    different += 1
                    if example is None:
                        example = dict(nodes=n, edges=edges, source=source, target=target,
                                       hops=hops, forward=first, bidirectional=second)
    workloads = []
    cases = [
        ("chain_four_hops", 2048, [(i, i + 1) for i in range(2047)], 0, 4, 4),
        ("wide_tree_four_hops", 273, [((i - 1) // 16, i) for i in range(1, 273)], 17, 272, 4),
        ("chain", 2048, [(i, i + 1) for i in range(2047)], 0, 2047, 2047),
        ("star_leaf_pair", 2048, [(0, i) for i in range(1, 2048)], 1, 2047, 2),
        ("star_center_leaf", 2048, [(0, i) for i in range(1, 2048)], 0, 1, 1),
        ("binary_tree_leaves", 2047, [((i - 1) // 2, i) for i in range(1, 2047)], 1023, 2046, 20),
    ]
    for shape, n, edges, source, target, hops in cases:
        adj = adjacency(n, edges)
        first, fw = forward(adj, source, target, hops)
        second, bw = bidirectional(adj, source, target, hops)
        assert len(first) == len(second)
        workloads.append(dict(shape=shape, nodes=n, hops=hops, path_length=len(first),
                              within_current_production_hop_ceiling=hops <= 4,
                              forward_adjacency_visits=fw, bidirectional_adjacency_visits=bw))
    result = dict(exhaustive_comparisons=comparisons, different_shortest_path_choices=different,
                  first_choice_difference=example, workloads=workloads)
    output = pathlib.Path(__file__).resolve().parents[1] / "results/native-implementation/graph-acceptance"
    output.mkdir(parents=True, exist_ok=True)
    (output / "bidirectional.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
