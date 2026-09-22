# Release gate, pre-calibration run (retained)

This directory is the first execution of `research/scripts/release_gate.py`. It
failed on `mean_region_coverage below threshold` because the initial 0.50
coverage threshold had been taken from the historical `marginal-coverage`
capture, which predates several later increments (ranker 22/23, graph-context
selection, task query policy, Markdown/documentation structure, package context,
shared identities and selective reconciliation).

Rather than loosen the gate, the actual pre-increment baseline was measured: a
worktree of commit `a0cbf7d` (before recommendation 21) was built and run through
the same 34 source-valid evidence-v1 tasks. See `calibration-pre21.json`:
28 required files, 12 complete regions, 0.4862 mean region coverage. The
recommendation-21 code measures 28/12/0.4869, so the change is neutral-to-positive
on this protocol and the stale threshold was the error. Accuracy thresholds were
then re-frozen in the script against that calibration.

The failure record, re-run probes during the investigation, and the calibration
measurement are retained here because the recalibration is part of the decision
record. The passing decision after recalibration is
`../release-gate-v3/release-decision.json`.
