# Follow-up review of the root checkout

Compared the four modified tracked files and the untracked `dbg_removal.rs` example against research branch commit `0941cb6`. The root checkout remains unchanged. File hashes are in [provenance](results/root-checkout-provenance.json).

## What was already incorporated

| Root change | Research branch disposition |
| --- | --- |
| Preserve incoming references when a target changes | Covered by full-tree reprojection on changed sync (F23), with modification/addition/deletion regressions. |
| Remove deleted files and symbols from resolution candidates | Covered by building known files from the current walk and excluding removed symbols. |
| Resolve Rust `self` through the enclosing type | Covered in extraction, with a two-type regression that distinguishes lexical ownership from global uniqueness. |
| Callee edit/deletion integration tests | Their underlying behaviors are covered. Root tests additionally exercise implicit query-triggered freshness; those exact test bodies were not copied. |
| `dbg_removal.rs` | Manual diagnostic for the covered deletion case; no additional product behavior. It uses a fixed temporary directory, so it is less suitable than isolated regression fixtures. |

## Distinct ideas not incorporated

The root ranker adds sentence stopwords, signature matching, inverse-frequency weights over symbol-name substrings, a larger additional-term bonus, a symbol bonus, and shorter-qualified-name tie breaking. These exact ranking changes were **not** measured in the original 225-query experiment. The previously rejected normalization experiment is not evidence that this entire root ranker performs poorly. Signature matching and frequency weighting deserve separate ablations against the frozen query sets; neither should be described as shipped by this PR.

However, this implementation has a blocking correctness defect: `score_against` adds `0.05` to every non-file symbol even when all terms score zero. `seed` accepts every score above zero. A search for `zzzzunmatchabletoken` in a file containing only `fn apple() {}` therefore returns `apple`. The isolated probe reproduces this. Require at least one actual lexical match before any bonus.

Additional ranking concerns from inspection: repeated query terms count as additional matches despite the comment promising distinct matches; signature matching precedes path matching at a lower weight (0.3 vs 0.4), so adding a signature occurrence can lower that term's score; the larger bonus still saturates at 1.0; sentence detection uses term count rather than query intent. Evaluate these independently, with no-answer queries and exact-name priority included. These are not claims of measured corpus regressions.

The root sync implementation expands only the reverse closure of **resolved** stored edges. This can avoid reparsing unrelated files, so it is a useful optimization direction. It cannot find a dangling caller when a newly added file supplies the missing name: dangling edges have no `to`, and the closure explicitly skips them. A formerly unique name becoming ambiguous in a new file also requires reconsidering callers whose old target file did not change. Persist raw reference facts and binding dependencies, including unresolved-name dependencies, before replacing the current full reprojection. The root also leaves `unchanged` equal to the original diff count after re-extracting some of those files.

The root receiver heuristic chooses the only workspace method with a matching suffix (or function for `::`). This offers more apparent resolution but does not establish receiver identity. Unknown external receivers can bind to unrelated workspace symbols. Do not port that fallback as a correctness fix; any future heuristic should expose weaker evidence explicitly.

The root's explicit `self` path has a separator mismatch: `self.execute` selects `.` and tries to split an enclosing `ToolRegistry::run_tools` on `.`. Its supplied unit test passes through the global unique-method fallback. With two types defining `finish`, the stronger branch regression fails on the root implementation. The branch extractor handles simple Rust `self` ownership before resolution and passes that case.

## Validation

The root's two receiver unit tests and its two new callee-file integration tests pass. Passing those tests does not cover the gaps above.

An isolated archive of root HEAD with the four tracked modifications overlaid was tested with the branch regression fixtures plus the minimal no-match probe. Source timestamps were refreshed before using a shared Cargo target directory to prevent reuse of stale workspace artifacts. All four selected probes fail on that root snapshot, as expected from the source traces:

1. Unrelated query returns no symbols: fails; unrelated `apple` returned.
2. `self` receiver uses lexical impl owner: fails with two methods of the same name.
3. Sync rebinds unchanged callers after target edits/addition: fails when the new file supplies the formerly missing `leaf`.
4. Foreign qualified calls do not bind to bare local names: fails; unknown qualification is discarded or bypassed.

See [root failure log](results/root-checkout-review.log). The same 25 branch accuracy tests plus the no-match probe pass on the research branch; see [branch comparison log](results/root-review-branch-comparison.log). The no-match probe is:

```rust
let (_d, i) = fixture(&[("a.rs", "fn apple() {}")]);
let r = i.search().explore(&ExploreQuery::new("zzzzunmatchabletoken")).unwrap();
assert!(r.items.is_empty(), "{r:?}");
```

`fixture` and the other named probes are in `crates/graph-search/tests/accuracy.rs`. The temporary comparison test files were removed after execution. No production changes were needed from this comparison. Retain the root's ideas as candidate ranking ablations and incremental-dependency work, rather than copying the patches wholesale.

VERDICT: CONCERNS
CONFIDENCE: high
SUMMARY: The concrete fixes are covered by the research branch; the distinct root optimizations contain reproduced correctness gaps and need refinement before incorporation.
