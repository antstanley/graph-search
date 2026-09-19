# Task-based evaluation suite

The identifier benchmark answered whether the engine can recover a function
whose name is already reflected in the query. It did not establish whether an
agent can debug behavior or plan a safe change. The new executable
[`evaluation/`](../evaluation/README.md) separates those questions explicitly.

## Corpus and labels

There are 60 manually authored tasks across nanus, blogwright and whatsurvey:
20 per repository, with a debugging question and change-planning request for
each of ten feature families. Six families per repository are development data;
four are held out. Related variants never cross splits. Tasks were authored
from current source before the full retrieval run, rather than selecting tasks
where an engine already won. They are realistic source-derived scenarios, not
claims that these bugs were reported by users or observed in production.

Public prompts contain no oracle paths or expected answers. Separate labels
record required code regions, relationships and semantic criteria, with hashes
of the source file and exact region. Examples cover context eviction, tool
approval and scheduling, publication synchronization, credentials, webhook
verification, consent decisions and Svelte clipboard behavior. Some tasks are
small helper explanations; others require multiple regions and longer control
flow. This first corpus is mostly within-file reasoning; it does not establish
cross-module architecture or arbitrary repository comprehension.

Source review is by the suite author, independent of returned search rankings,
not independent human review. Task and rubric quality still benefit from human
adjudication. Labels stay fixed for an experiment: source drift is an error,
not permission to silently regenerate expected answers. Gold is committed for
reproducibility, so the held-out split is a tuning discipline, not a secret test
set. Future tuning based on these results needs a fresh held-out version.

## Protocol and measurements

The evidence protocol searches the public task prompt and reads up to three
distinct returned files near their first returned location, within four calls,
16 KiB per response and 48 KiB cumulative tool content. Planning never consults
the labels. The common read operation is constrained to the repository and
bounded to 200 lines. Source credit is awarded only to actual, exact returned
lines after truncation; CodeGraph gaps and graph node spans are not silently
filled. Coverage reports include required files, individual code regions and
source evidence for labelled relationships. Complete-region coverage is a
conservative evidence proxy, not a semantic answer score.

An external agent driver can instead choose actions and produce an answer.
It receives public tasks, tool schemas and history, never gold. Provider tokens
are optional actual usage, distinct from character-based estimates. The driver
is trusted code: temporary working directories do not provide OS isolation.
Model identity and effort are recorded but must be honored by the driver.
The tests use a scripted driver solely to validate this protocol.

Answer grading is metadata-blind and requires a named reviewer, all rubric
judgments, an unsupported-claims judgment and citations to observed source
locations. Hashes link grading packets to their trial. A file hit cannot set
`task_success`; ungraded or retrieval-only results remain null. Reports retain
errors and unavailable arms, distinguish denominators, and compute paired
comparisons only for shared task/repeat IDs.

The graph-search arm uses the public resident Rust library; index setup is
outside trial timing. CodeGraph and ripgrep include subprocess startup. The
text arm is a deliberately basic fixed OR-term query, not adaptive use of grep.
Comparisons describe these integration/protocol combinations. They cannot prove
that one search technology is universally superior. Source snapshots and
read-only CodeGraph freshness audits are recorded before and after the run.

## First frozen-corpus evidence run

The verified run scheduled 180 comparisons: **160 available trials, 20 unavailable
CodeGraph/nanus trials, zero tool-error trials**. No actual model was used; all
answer-success scores are null. Every task source hash and repository content
snapshot matched before and after execution. Existing CodeGraph indexes were
audited without rebuilding them. See the manifest for their exact freshness.

| Repository | Arm | Required file found | All required regions delivered | Median trial ms | Median response bytes |
|---|---|---|---|---|---|
| nanus | graph-search | 13/20 | 1/20 | 745 | 26660 |
| nanus | codegraph | unavailable | unavailable | — | — |
| nanus | text | 0/20 | 0/20 | 29 | 33047 |
| blogwright | graph-search | 15/20 | 7/20 | 895 | 22284 |
| blogwright | codegraph | 8/20 | 6/20 | 412 | 31241 |
| blogwright | text | 0/20 | 0/20 | 33 | 34364 |
| whatsurvey | graph-search | 9/20 | 4/20 | 2113 | 31233 |
| whatsurvey | codegraph | 4/20 | 0/20 | 515 | 35626 |
| whatsurvey | text | 0/20 | 0/20 | 81 | 29758 |

These combined-split figures are descriptive; the machine-readable report
separates development/held-out and debugging/change-planning tasks and includes
paired denominators. Debug/change variants share a family and are correlated,
so do not treat these 60 prompts as 60 independent feature samples. Timings are
one repeat, debug Rust builds, shared-machine measurements.

The text baseline's zero file hits reveals how poor a fixed broad OR query and
path ordering can be under a tight output cap. It is not evidence that an agent
using targeted grep queries cannot solve these tasks. Similarly, a graph-search
file hit often fails to deliver the complete labelled implementation. An
adaptive agent trial is the next experiment; it must use the same model, effort,
and budgets across arms. No ranking or query-policy tuning followed inspection
of these held-out results. The earlier development smoke run and initial full
run are private run artifacts; the published run additionally records complete
implementation and source provenance.

Artifacts:

- [Per-trial sanitized results](../evaluation/results/baseline-v1-results.json)
- [Stratified and paired report](../evaluation/results/baseline-v1-report.json)
- [Versions, hashes and freshness manifest](../evaluation/results/baseline-v1-manifest.json)

Raw external source snippets and driver transcripts are intentionally excluded
from committed results. The corpus and labels contain source locations, hashes
and authored rubrics, not copied external implementations.
