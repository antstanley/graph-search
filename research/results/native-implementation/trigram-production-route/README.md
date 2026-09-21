# Paired production-scanner admission diagnostic

A disposable native admission hook compares the same production scanner with and
without byte-trigram candidate rejection on stable snapshots. All 45 hit lists
agree. The capture records selectivity, source reads, build/teardown cost,
replacement/restore cost, and individual warm-query timings. No production source
is changed; exact instrumentation and probe source are archived here.

Estimated break-even counts from individual medians do not establish cumulative
workload behavior. The subsequent [cumulative capture](../trigram-cumulative/README.md)
adds build-plus-repeated-query curves, including strict read/hash verification,
and is the conditional adoption decision evidence. No live-source speedup or
persistent production index is claimed.
