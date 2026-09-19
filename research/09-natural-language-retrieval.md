# Natural-language retrieval: bounded bodies and file diversity

Base: `bdcbecef41d9863bca23830ed30615f1c219064d` (PR #2). Implementation branch:
`feature/natural-language-retrieval`. This study evaluates additional lexical
content, query expansion, and file diversity separately. It does not add an
embedding model, change storage engines, or equate retrieval with agent success.

## Decision and study design

Promote a separately normalized bounded function/method body field (weight 1)
and greedy file diversity (factor 0.5), while retaining priority for exact and
complete split identifiers. Keep standalone leading comments, documentation
passages, and generic inflection expansion experimental. Documentation retrieval
remains a known gap; the body change does not solve prose-only questions.

The primary selection data are 36 development prompts from the source-backed
task suite (12 each in nanus, blogwright, whatsurvey). Another 24 previously
published prompts are confirmation data, never used to choose field weights or
the diversity factor. Nine source-authored documentation questions (6 development,
3 confirmation) diagnose documentation retrieval separately from code-file hits.
The original 90 exact, 90 split-name, 60 held-out split-name, 30 discovery and 15
natural-language queries are compatibility checks. Labels were not rewritten in
response to misses. See [protocol](natural-language/PROTOCOL.md),
[reproduction and evidence index](natural-language/README.md), and
[task methodology](08-task-evaluation.md).

All prototype arms use the same baseline graph candidates. Name/path/signature
metadata keeps weights 8/2/1. Additional fields have separate BM25 document
frequencies and length normalization. Bodies are capped at 64 lines/4,096 Unicode
characters; leading comments at 32 lines/2,048 characters; documentation at
64-line/4,096-character passages within the first 64 KiB of an indexed file.
The prototype uses line spans; the shipped implementation uses exact byte spans.
Sources and their existing indexes are read-only. The final core probe records
before/after repository snapshots and validates task source hashes.

## Content ablations: development only

File hits are required-file recall@8 (one required file per task in this suite).
MRR is reciprocal rank of that file. Documentation hits are separate diagnostics.

| Fields added to metadata | File hits / 36 | MRR | Doc hits / 6 |
|---|---:|---:|---:|
| metadata | 23 | 0.295 | 0 |
| comments@0.25 | 21 | 0.286 | 0 |
| comments@0.5 | 24 | 0.307 | 0 |
| comments@1.0 | 25 | 0.330 | 0 |
| bodies@0.25 | 25 | 0.341 | 0 |
| bodies@0.5 | 25 | 0.359 | 0 |
| bodies@1.0 | 29 | 0.438 | 0 |
| docs@0.25 | 23 | 0.295 | 0 |
| docs@0.5 | 23 | 0.295 | 0 |
| docs@1.0 | 22 | 0.290 | 2 |
| comments+bodies+docs@0.25 | 25 | 0.331 | 0 |
| comments+bodies+docs@0.5 | 28 | 0.365 | 0 |
| comments+bodies+docs@1.0 | 29 | 0.434 | 2 |

Bodies at weight 1 beat comments alone and documentation alone. All three fields
at weight 1 tie body-only file recall but have slightly worse MRR. The decision
is specific to these code tasks: documentation at weight 1 recovers 2/6 dedicated
documentation questions, versus none with metadata alone. A better doc-specific
retrieval path remains worth testing; indiscriminately mixing passages into this
symbol candidate list is not justified by these results.

The first development run incorrectly included empty symbol placeholders in the
documentation corpus statistics. This suppressed documentation matches. The
failure is retained under `results/pilot/`, and was corrected before opening
confirmation tasks. A later audit corrected the documented 64 KiB bound from
characters to UTF-8 bytes; rerunning development preserved the selected ranker.

## Expansion and diversity, measured separately

Expansion uses a frozen generic inflection rule and half-weight alternative
terms from the indexed vocabulary. It has no task-specific synonym dictionary.
Diversity multiplies each ordinary candidate's relevance by `factor^n`, where
`n` already-selected seeds share its file. Exact symbol candidates are exempt.

| Intervention on metadata + bodies | File hits / 36 | MRR | Symbol-region overlap |
|---|---:|---:|---:|
| selected_content | 29 | 0.438 | 0.384 |
| expansion | 29 | 0.546 | 0.481 |
| diversity@0.5 | 31 | 0.464 | 0.227 |
| diversity@0.25 | 31 | 0.464 | 0.227 |
| expanded-diversity@0.5 | 30 | 0.567 | 0.255 |
| expanded-diversity@0.25 | 30 | 0.567 | 0.255 |

Diversity wins the predeclared file-recall objective. Expansion improves MRR
substantially but adds no files; its combination with diversity loses one file.
This does not establish expansion as generally harmful: a different objective
or richer evidence-selection policy could value its ordering improvement.
Diversity also reduces development symbol-span overlap. That is a real tradeoff,
not hidden by the file-recall gain. Running prototype rankings through the
actual evidence-read protocol gives 6/36 evidence-ready trials for metadata,
7/36 for bodies and 7/36 for bodies + diversity, with no errors. These remain
prototype-backend evidence measurements, not public-engine or model results.

## Prototype confirmation

The frozen selected configuration is metadata 1 + bodies 1 + diversity 0.5.
On 24 confirmation tasks it improves file hits from 14 to 18, MRR from 0.384 to
0.467, and mean symbol-region overlap from 0.292 to 0.354. By repository:

| Repository | Metadata file hits / 8 | Selected file hits / 8 |
|---|---:|---:|
| nanus | 6 | 8 |
| blogwright | 5 | 7 |
| whatsurvey | 3 | 3 |

Neither configuration finds the three dedicated documentation confirmation
files. Improvements are uneven: whatsurvey remains difficult, and blogwright's
file recall improves while MRR declines slightly (0.432 to 0.416). This is a small
published confirmation set, not independent proof of generalization.

## Production implementation and discovered errors

`Projector::extract_one` attaches normalized term-frequency maps after parsing
and symbol-cap validation. Only functions and methods receive the reserved
`graph_search.body_terms.v1` attribute. Both graph nodes and raw extraction facts
persist the bounded map and a truncation flag; no raw function body is persisted.
The text includes the declaration and in-span comments/literals, so this is not
a comment-free body ablation. The query builds separate metadata/body statistics
from the same snapshot, combines raw scores, and greedily selects diverse seeds.
Filters, fallback scan limits, graph assembly and response budgets still apply.

The implementation uncovered an existing Rust/JS/TS extractor defect: byte
offsets were passed through the one-based line-number conversion. Spans were
shifted by one byte and EOF spans could not be sliced. Both extractors now use
zero-based byte offsets with an exclusive end. Tests cover UTF-8 before/inside
functions, EOF, and same-line siblings. Parser version 3 forces a cache rebuild;
schema remains 2. Staleness detects version mismatch, so the normal before-query
reconcile path upgrades old indexes. Explicit no-reconcile behavior remains
under the caller's control.

Exact bare/qualified names remain first in stable path/line/id order. Complete
split-name matches follow. Only when no complete split-name target exists can a
short query naming several bare symbols prioritize those targets for graph
connections. This is a measured departure from the prototype to preserve the
existing `entry leaf` connection behavior. The initial implementation conflated
these two named cases: `load pds secret` promoted individual `secret` symbols.
The partial failed run is retained in `rejected-split-priority.json`; the fix
requires complete split-name matches to win over separate-name interpretation.
No content weights or diversity factors were retuned on compatibility results.

The bounded-body approximation note also survives final byte fitting. Previously
`fit_explore` replaced the whole approximation object while updating edge
counts, which would discard this note. It now updates the counts in place.

## Public-library task comparison

The controlled baseline and current public `Index` hosts each ran all 60 task
prompts against unchanged sources, using the same four-call evidence protocol.
Both completed without tool errors. Required-file hits improve **37/60 → 49/60**;
complete delivered evidence changes **12/60 → 11/60**. No model was run, so task
success remains null in both arms.

| Repository | Baseline file hits / 20 | Current file hits / 20 | Baseline evidence-ready | Current evidence-ready |
|---|---:|---:|---:|---:|
| nanus | 13 | 19 | 1 | 2 |
| blogwright | 15 | 19 | 7 | 8 |
| whatsurvey | 9 | 11 | 4 | 1 |

Development evidence-ready is 6/36 → 7/36; confirmation evidence-ready is
6/24 → 4/24. [Action/location diagnostics](natural-language/results/evidence-changes.json)
explain the changes without copying external source:

- `nanus.write.debug` and `blogwright.record-key.debug` gain complete evidence.
- `whatsurvey.storage-envelope.change` still finds the right file, but its
  candidate is now fourth; the protocol spends its three follow-up reads on
  earlier files. Its four-line snippet covers only 4/15 required lines.
- Both `whatsurvey.captcha-retry` tasks now anchor to `verifyCaptcha` at line 78
  rather than `RETRY_ATTEMPT` at line 15. The read starts at line 68, missing the
  earlier retry/transport logic required by lines 32–96. Coverage is 29/65 lines.

These are meaningful evidence regressions. They do not violate the predeclared
file-recall selection objective or exact-name gate, but they limit the practical
claim: broader file discovery is not yet better task completion. Do not tune
weights on these confirmation cases. The next experiment should improve match
locations and follow-up read selection on new development tasks, then use new
confirmation tasks.

The earlier text/CodeGraph results remain in `evaluation/results/` and
[the task-suite report](08-task-evaluation.md). They are not rerun or relabelled
as new content experiments here. In particular, the literal text arm's failure
on full natural-language prompts is not evidence that an agent using smaller
keywords would fail. CodeGraph and graph-search have different response shapes
and evidence paths.

## Frozen production retrieval checks

The core probe runs 354 identical requests per arm and records every returned
candidate and miss. It checks before/after repository snapshots and task hashes.
The final result is:

| Query family | Baseline hits | Current hits | Baseline MRR | Current MRR |
|---|---:|---:|---:|---:|
| exact | 90/90 | 90/90 | 0.993 | 0.993 |
| split | 89/90 | 90/90 | 0.940 | 0.976 |
| heldout-split | 60/60 | 60/60 | 0.972 | 0.981 |
| discovery | 29/30 | 29/30 | 0.944 | 0.944 |
| natural | 8/15 | 10/15 | 0.378 | 0.501 |
| task-dev | 23/36 | 31/36 | 0.295 | 0.464 |
| task-heldout | 14/24 | 18/24 | 0.384 | 0.473 |
| documentation-dev | 0/6 | 0/6 | 0.000 | 0.000 |
| documentation-heldout | 0/3 | 0/3 | 0.000 | 0.000 |

All 90 exact target ranks are individually unchanged or better; this is stronger
than equal aggregate hit count. Split-name hits improve, but some within-top-eight
ordering changes remain: nanus original split MRR changes 0.983 → 0.978 and
whatsurvey confirmation split MRR 1.000 → 0.967. The frozen natural-language
challenge improves 8/15 → 10/15; it remains small and separate from the 60 task
prompts. No documentation-target hits are gained by the promoted configuration.
The core task-file results agree with the separate public-library task run.

## Cost and bounded coverage

A separate sequential debug profile uses three exact `explore` prompts and three
development task prompts per repository, each repeated three times. It runs
after the other experiments, reverses arm order for blogwright, and measures the
public `Index` including freshness, serialization and adapter overhead. These
small local samples are not release benchmarks or tail-latency guarantees.

| Repository | Exact explore median ms, old → new | Task explore median ms, old → new | Live store MiB, old → new |
|---|---:|---:|---:|
| nanus | 729 → 1058 | 746 → 1095 | 17.14 → 19.85 |
| blogwright | 898 → 1056 | 922 → 1112 | 17.57 → 18.72 |
| whatsurvey | 2143 → 2441 | 2176 → 2553 | 49.36 → 51.21 |

Exact-symbol **accuracy** is preserved; exact-query latency is not. Across these
samples, exact explore is 14–45% slower and natural-language explore is 17–47%
slower. Live store size grows roughly 4–16%. Query construction still reads graph
nodes, decodes term maps and rebuilds lexical statistics for every call. This is
an obvious optimization candidate, but this experiment does not isolate each
component's share of the latency. There is no claim of a query-speed improvement.
The main frozen-query/task-run timings overlapped other work and are not used
for this cost comparison. No peak-memory or release-build measurement was made.

| Repository | Functions/methods with body terms | Truncated bodies | Serialized node term-map bytes |
|---|---:|---:|---:|
| nanus | 3404/3404 | 62 | 1,127,093 |
| blogwright | 1393/1393 | 32 | 481,299 |
| whatsurvey | 2804/2804 | 24 | 749,626 |

All function/method candidates in these corpora receive a valid body map. The
byte column counts node attributes only; the manifest also stores the maps in
raw facts. It is not the total on-disk increase. Caps bound each function, not
total corpus memory, and nested functions can index overlapping text. Matches
beyond the caps may rely on the existing low-priority literal fallback.

## Verification and next experiments

[The verification record](natural-language/VERIFICATION.md) traces extraction,
cache migration, ranking, filtering and output fitting. The 101-test Rust suite,
strict Clippy, formatting, 24 taskbench tests and three prototype tests pass.
`results/summary.json` mechanically checks query counts, per-query exact ranks,
absence of tool errors, unchanged source snapshots and matching task/profile
executables. CI now runs the workspace tests and Clippy alongside taskbench.

The next priorities, in order, are:

1. Return useful match locations and choose follow-up reads from those locations,
   not only definition starts. Evaluate complete evidence alongside file recall
   on new development/confirmation tasks before changing the policy.
2. Cache lexical statistics for an immutable graph generation and invalidate them
   on successful reconcile. Profile cold/warm queries, exact names, memory and
   persistence separately before claiming that this recovers the measured cost.
3. Try a dedicated documentation retrieval route or calibrated passage/symbol
   fusion. The current body ranker retrieves none of the nine documentation
   targets; docs-only improvements on two development prompts do not generalize
   in this small study.
4. Revisit expansion with a predeclared objective that includes useful evidence
   ordering. Its MRR gain is real in development, but its combination with file
   diversity loses a file under the current objective.
5. Run actual agent trials and blind grading using the existing harness. Retrieval
   and delivered evidence are still proxies. Module/scope binding and explicit
   graph-work budgets remain separate correctness/performance projects.
