"""Summarize the ordinal-only codec experiment without claiming whole-index savings."""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path


def lanes(value, prefix=''):
    if isinstance(value, dict):
        if 'ordinal_checksum' in value:
            yield prefix, value
        else:
            for key, item in value.items():
                yield from lanes(item, f'{prefix}/{key}')


def summarize(directory):
    checks = json.loads((directory / 'checks.json').read_text())
    assert all(checks['stability'].values())
    assert checks['composition_repeat_equality']
    result = {}
    for repository in ('nanus', 'blogwright', 'whatsurvey'):
        totals = dict(lists=0, entries=0, plain_payload_bytes=0,
                      codec_payload_bytes=0, plain_inline_bytes=0,
                      codec_inline_bytes=0, codec_capacity_bytes=0)
        detail = {}
        ratios = {str(i): {name: [] for name in ('scan', 'seek', 'build')} for i in range(5)}
        for repeat in range(3):
            capture = json.loads((directory / f'{repository}-{repeat}-codec.json').read_text())
            for name, lane in lanes(capture):
                if repeat == 0:
                    count = sum(lane['list_counts'])
                    totals['lists'] += count
                    totals['entries'] += sum(lane['entry_counts'])
                    totals['plain_payload_bytes'] += sum(lane['plain_payload_bytes'])
                    totals['codec_payload_bytes'] += sum(lane['codec_payload_including_restarts_bytes'])
                    totals['codec_capacity_bytes'] += sum(lane['codec_capacity_including_restarts_bytes'])
                    totals['plain_inline_bytes'] += count * lane['plain_inline_bytes_per_list']
                    totals['codec_inline_bytes'] += count * lane['codec_inline_bytes_per_list']
                    detail[name] = {key: value for key, value in lane.items() if key != 'timings'}
                assert detail[name] == {key: value for key, value in lane.items() if key != 'timings'}
                for bucket in lane['timings']:
                    if bucket['sample_lists'] == 0:
                        continue
                    for row in bucket['rows']:
                        assert row['0_scan_checksum'] == row['1_scan_checksum']
                        assert row['0_seek_checksum'] == row['1_seek_checksum']
                        for operation in ('scan', 'seek', 'build'):
                            ratios[str(bucket['bucket'])][operation].append(
                                row[f'codec_{operation}_ns'] / row[f'plain_{operation}_ns'])
        totals['modeled_plain_live_bytes'] = totals['plain_payload_bytes'] + totals['plain_inline_bytes']
        totals['modeled_codec_live_bytes'] = totals['codec_payload_bytes'] + totals['codec_inline_bytes']
        result[repository] = {'totals': totals, 'lanes': detail,
                             'codec_to_plain_paired_ratios': {
                                 bucket: {operation: {
                                     'pairs': len(values), 'median': statistics.median(values),
                                     'min': min(values), 'max': max(values)
                                 } for operation, values in operations.items() if values}
                                 for bucket, operations in ratios.items()}}
    return {'repositories': result,
            'note': 'Standalone ordinal vectors, not resident or complete posting layouts. '
                    'Ratios pool equally weighted lane/process/batch pairs within each size bucket; '
                    'not workload-weighted query latency. Buckets: <=1, 2-4, 5-16, 17-128, >128. '
                    'Build compares encoding with copying an existing plain ordinal vector; '
                    'not extraction, indexing or update cost.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    result = summarize(args.directory)
    (args.directory / 'summary.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    for name, values in result['repositories'].items():
        print(name, values['totals'])
        for bucket, operations in values['codec_to_plain_paired_ratios'].items():
            print('  bucket', bucket, {key: round(value['median'], 2) for key, value in operations.items()})
