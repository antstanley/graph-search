# Recommendation 11 acceptance audit

The original recommendation requires separate candidate/context selection,
protected navigation, channel provenance, controlled ranking/diversity experiments,
and preservation of complementary source evidence. It does not mandate adopting
fusion or normalized scores when they lose evidence. Each requirement is mapped
below to current implementation and measured decisions.

| Requirement | Current implementation and evidence |
|---|---|
| Retrieve deeper than the final eight seeds | `QueryEngine::seed` retains a pool of max(k × 4, 64), capped by the graph ceiling, separately from final k. Native metadata top-k and body owner ranking feed final selection. Work exhaustion is reported; the pool is not claimed exhaustive. |
| Separate exact navigation from conceptual ranking | Explicit ID/name/path/prefix routes return before conceptual stages. Optional inferred exact fast path is distinct from Auto; exact metadata has a protected score tier. Planner/body tests verify priority and absence of unrelated posting work. |
| Retain per-channel provenance and rank | `RetrievalEvidence` retains metadata rank, body rank and exact status when explanation is requested. `RetrievalPlan` records original input/options and attempted routes. Scores are ordering signals, not correctness probabilities. |
| Evaluate body, metadata and rank fusion by class | Ranking × diversity factorial: 918 established-task and 324 fresh-routing trials; separate automatic-route checks. Measured losses reject unconditional RRF; multiword Auto uses body first with empty-body metadata fallback. Explicit channel options remain available. |
| Evaluate normalized score combinations | Nine fixed-pool policies across 58 source-valid tasks, aggregated by repository and debug/change kind. The nominated 75% body min-max candidate then fails 348 native source-delivery trials: 30→27 complete tasks, no gains. Retain the production policy; do not integrate a known losing score combination. |
| Deduplicate occurrences without erasing complementary evidence | Body retrieval groups source regions by documented symbol/owner identity and ranks the owner by its best region. Metadata/body candidates merge by NodeId; additional distinct regions contribute evidence without score inflation or extra candidate slots. Final source accounting deduplicates path/hash/line occurrences across output items. |
| Intent-dependent file diversity, not universal one-hit-per-file | `per_file` is explicit and soft: defer excess same-file candidates, refill unused capacity, exempt exact hits. Zero is the measured default. Multiple functions in the same module can survive. Factorial experiments vary diversity independently of channel choice. |
| Preserve spans for distinct subquestions/relationships | Per-owner complementary regions prefer uncovered query-term masks and distinct match anchors; original spans survive into source assembly. Returned connections retain bridge endpoints and call-site occurrence context. The 400-line/two-term regression returns both distant regions as one owner; repeated-site tests verify separate relationship evidence. This is bounded evidence selection, not automatic semantic decomposition or guaranteed relevance for arbitrary prose. |
| Test representation and diversity separately | The channel/diversity factorial holds native representation fixed. Separate body/source/Markdown representation experiments are recorded independently; the normalized experiment also fixes representation/diversity. No combined representation/grouping change is attributed to a single weight. |

The current design deliberately permits metadata, body and graph explanations to
compete for finite output bytes; final source packing is a separate phase with its
own tests and evidence. Candidate-file gains alone cannot establish complete-region
or task success. The rejected normalized experiment is direct evidence of that
distinction, including changed same-file follow-up anchors and displaced reads.

Primary sources: `crates/core/src/query.rs` (`seed`, `select_diverse`),
`body.rs` (`rank`), `evidence.rs`, `context_dedup.rs`, `types/retrieval.rs`, and
`graph-search/tests/{planner,body}.rs`. Experimental evidence:

- [Channel/diversity factorial](results/native-implementation/ranking-ablation/summary.json)
  and [fresh-routing comparison](results/native-implementation/ranking-fresh/summary.json).
- [Fixed-pool normalized scores](results/native-implementation/channel-combinations/README.md).
- [Native source-delivery rejection and every loss](results/native-implementation/score-combination-context/README.md).
- [Context requirements and limitations](CONTEXT-SELECTION-AUDIT.md).

Recommendation 11 is accepted: all 27 body/planner integration tests and 28
evaluation tests pass, and all 222 captured input hashes match. Recommendation 12
still explicitly requires independent answer-success measurement; recommendation 30
requires fresh broad release and model-task evaluation. Those are not inferred from
the development corpora or moved into a claim that search quality is universally
improved. Package resolution and incremental dependency work remain 21/23.
