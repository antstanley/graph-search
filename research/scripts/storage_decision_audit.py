"""Audit the conditional compression decision against frozen measured evidence."""
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RESULTS = ROOT / 'research/results/native-implementation'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    inputs = {}

    def read(relative):
        path = RESULTS / relative
        inputs[relative] = digest(path)
        return json.loads(path.read_text())

    baseline = 'storage-composition-measured'
    checks = read(f'{baseline}/checks.json')
    assert all(checks['stability'].values()) and checks['composition_repeat_equality']
    compaction = read('posting-compaction/checks.json')
    assert all(compaction.values())
    codec = read('posting-codec-measured/validation.json')
    assert codec['codec_tests_passed'] == 3 and codec['codec_tests_failed'] == 0
    assert all(codec['stability'].values()) and not codec['production_codec_adopted']
    layouts = {}
    for capture in [baseline, 'posting-codec-measured']:
        provenance = read(f'{capture}/provenance.json')
        for name in ['crates/core/src/lexical.rs', 'crates/core/src/body.rs',
                     'crates/core/src/metadata.rs']:
            current = digest(ROOT / name)
            assert current == provenance['sources'][name], (capture, name)
            layouts[name] = current
    summaries = {}
    for repository in ['nanus', 'blogwright', 'whatsurvey']:
        source = read(f'{baseline}/{repository}-composition.json')
        process = read(f'{baseline}/{repository}-process.json')
        lanes = [source['metadata']['split'], source['metadata']['identifiers'],
                 source['body'], source['body']['identifier_lane']]
        summaries[repository] = {
            'native_components_not_a_total_heap_census': {
                'posting_live_bytes': sum(lane['postings']['vector_length_bytes'] for lane in lanes),
                'posting_capacity_bytes': sum(lane['postings']['vector_capacity_bytes'] for lane in lanes),
                'posting_dictionary_utf8_bytes': sum(lane['postings']['term_utf8_bytes'] for lane in lanes),
                'metadata_norm_live_bytes': sum(lane['norm_length_bytes'] for lane in lanes[:2]),
                'source_line_occurrence_live_bytes': source['source_facts']['line_length_bytes'],
                'source_line_occurrence_capacity_bytes': source['source_facts']['line_capacity_bytes'],
                'adjacency_incident_ordinal_live_bytes': source['adjacency']['incident']['vector_length_bytes'],
                'adjacency_edge_inline_capacity_bytes': source['adjacency']['edge_inline_capacity_bytes'],
                'metadata_node_inline_capacity_bytes': source['metadata']['node_inline_capacity_bytes'],
                'exact_name_ordinal_live_bytes': sum(source['metadata'][name]['vector_length_bytes']
                                                     for name in ['bare', 'qualified', 'folded']),
            },
            'serialized_bytes_not_heap': {
                'source_facts': source['source_facts']['json_bytes_not_heap'],
                'occurrence_facts': source['occurrence_facts']['json_bytes_not_heap'],
                'adjacency_edges': source['adjacency']['edges_json_bytes_not_heap'],
                'metadata_nodes': source['metadata']['nodes_json_bytes_not_heap'],
            },
            'disk': process['disk'],
            'source_blob_and_positions_contract': source['note'],
        }
    read('posting-codec-measured/summary.json')
    read('posting-compaction/maintenance-comparison.json')
    output = RESULTS / 'storage-decision-audit'
    output.mkdir(exist_ok=True)
    (output / 'audit.json').write_text(json.dumps({
        'recommendation_source_sha256': digest(ROOT / 'research/09-native-search-review.md'),
        'audit_driver_sha256': digest(Path(__file__)),
        'input_sha256': inputs, 'current_layout_sha256': layouts,
        'corpora': summaries,
        'decision': 'retain current native vectors; neither tested candidate is adopted',
        'recommendation_27': 'conditional gate satisfied; not a claim of completed compression or total heap attribution',
        'current_parser_benchmark_claim': False,
    }, indent=2, sort_keys=True) + '\n')
    print('Evidence checks pass; three current layout modules match both measured captures.')
    for name, values in summaries.items():
        print(name, values['native_components_not_a_total_heap_census'], values['serialized_bytes_not_heap'])


if __name__ == '__main__':
    main()
