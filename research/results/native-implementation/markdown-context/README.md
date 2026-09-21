# Markdown structure versus fixed windows: public API comparison

276 trials: 46 tasks × two arms × three repeats. All completed without protocol
errors. Evidence was identical across repeats. Production/driver hashes, both
binary hashes, sibling source snapshots and all sibling CodeGraph index files
remained unchanged. No dependencies or sibling files were modified.

The structured arm is current production. The control is a disposable copy with
exactly one source intervention: replace Markdown region selection with an empty
region vector in `units::extract`. Markdown retains its source kind and otherwise
uses the same native 80-line windows with eight-line overlap. Both arms use fresh
public `Index` stores, auto ranking, unrestricted per-file seed count, semantic
graph context, and the existing evidence-v1 protocol: one explore plus up to three
100-line candidate reads, four calls, 16 KiB per response, 48 KiB total, 180 seconds.
The control's representation constants are unchanged; its experimental policy is
identified by the source patch and binary hash, never published as a product index.

| Suite | Arm | All required files | Complete regions | Mean region coverage | Mean response bytes |
|---|---|---:|---:|---:|---:|
| Established source-valid | Structured | 28/34 | 12/34 | 50.51% | 34,799 |
| Established source-valid | Fixed | 28/34 | 12/34 | 51.20% | 33,348 |
| Newer routing | Structured | 12/12 | 9/12 | 76.04% | 36,976 |
| Newer routing | Fixed | 12/12 | 10/12 | 83.33% | 36,447 |

These are development evidence tasks, not independent blinded confirmation. No
model answered them. Timings are retained as raw observations, not a performance
comparison. Both indexes were prepared before interleaved queries; builds finished
before retrieval measurements.

## Inspected gains and losses

`paired-changes.json` lists all seven tasks whose region coverage differs. The
other 39 have identical evidence metrics. Complete-evidence transitions:

- `blogwright.fresh-refresh-metadata.debug`: structured 12.5%, fixed 100%.
  Structured follow-up reads select deploying/troubleshooting documentation and
  the CLI README; fixed selects build.ts, repo.ts and build.test.ts. The required
  file is present in initial evidence in both arms, but the follow-up selection
  loses the implementation region. This is candidate competition, not a failed
  source verification or exhausted response byte budget.
- `whatsurvey.webhook-signature.change`: structured 0%, fixed 100%.
  The second follow-up changes from core/whatsapp/signature.ts to the WhatsApp
  survey specification. Other follow-ups select auth-mock/signing.ts and signature
  tests in both arms. Required-file recall alone hides this regression.
- `blogwright.tag-encoding.debug`: structured 100%, fixed 40%.
  The third follow-up changes from s3.test.ts to tags.ts, completing the evidence.

Markdown boundaries also change corpus-wide body statistics and competition;
code ranking can change even when the required evidence is not Markdown. This
experiment deliberately includes that downstream effect. It does not isolate
individual effects of block sizes, field metadata or global statistics.

## Decision and remaining gate

Do not claim that structural units improve retrieval from this comparison. Keep
recommendation 25 open. Source-faithful structural metadata remains useful, but
ranking units and evidence units may need separate policies. Next evaluate frozen
documentation tasks and test a native representation policy that preserves useful
code candidates without globally suppressing documentation. Do not tune to only
these seven exposed tasks or increase budgets to conceal losses.

Reproduce in a new output directory:

```sh
python3 research/scripts/markdown_context.py \
  --output /tmp/markdown-context-new \
  --suite research/fixtures/fresh-routing-2026-09-19
```

The script writes tasks, labels, results, summaries, build provenance and stability
checks. `paired-changes.json` is a derived comparison of repeat-zero evidence by
task ID, retaining rows where structured and fixed evidence differ. Raw source
transcripts and disposable build sources remain in the temporary location printed
by the runner. No external source snippets are included here.
