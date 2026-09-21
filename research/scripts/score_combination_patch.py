"""Exact disposable native query intervention; production remains unchanged.

The experiment is restricted to clean indexed generations. An actual live-body
lane is explicitly rejected rather than mixing raw scores from different corpora.
"""

PREPARE = '''        // Research-only clean-generation normalized combination.
        if !live.is_empty() {
            return Err(Error::InvalidQuery("normalized experiment requires no live body lane".into()));
        }
        let exact_ids: BTreeSet<_> = metadata.hits.iter()
            .filter(|hit| hit.score >= 2.0).map(|hit| hit.item.id.clone()).collect();
        let normalized_metadata = research_minmax(metadata.hits.iter()
            .filter(|hit| hit.score < 2.0)
            .map(|hit| (hit.item.id.clone(), hit.score)).collect());
        let mut raw_body = BTreeMap::new();
        for hit in &indexed {
            let unit = &self.snapshot.source_files()[&hit.path].units[hit.unit];
            let owner = unit.documentation.as_ref().and_then(|doc| doc.documented_symbol.as_ref())
                .or(unit.owner.as_ref());
            let id = owner.map(|id| self.snapshot.node_by_id(id)).transpose()?.flatten()
                .map_or_else(|| NodeId::file(&hit.path), |node| node.id);
            if !exact_ids.contains(&id) {
                raw_body.entry(id).or_insert(hit.score);
            }
        }
        let normalized_body = research_minmax(raw_body);
'''

HELPER = '''
// A flat nonempty lane supplies equal evidence; a missing lane supplies none.
fn research_minmax(mut scores: BTreeMap<NodeId, f32>) -> BTreeMap<NodeId, f32> {
    let low = scores.values().copied().fold(f32::INFINITY, f32::min);
    let high = scores.values().copied().fold(f32::NEG_INFINITY, f32::max);
    for score in scores.values_mut() {
        *score = if high > low { (*score - low) / (high - low) } else { 1.0 };
    }
    scores
}
'''

PATCHES = [
    ('''        let ranking = if automatic_ranking {
            if query.query.split_whitespace().count() > 1 {
                RankingStrategy::Body
            } else {
                RankingStrategy::Fusion
            }
        } else {
            query.retrieval.ranking
        };''', '''        let ranking = if automatic_ranking {
            RankingStrategy::Fusion
        } else {
            query.retrieval.ranking
        };'''),
    ('        let mut body_hits: Vec<_> = indexed', PREPARE + '        let mut body_hits: Vec<_> = indexed'),
    ('''            // Exact names are a separate priority tier. Other channels combine by rank.
            hit.score = if hit.score >= 2.0 {
                2.0
            } else {
                rank_score(rank)
            };''', '''            // Exact names remain a separate priority tier in this experiment.
            let _ = rank;
            hit.score = if hit.score >= 2.0 { 2.0 } else {
                0.25 * normalized_metadata.get(&hit.item.id).copied().unwrap_or(0.0)
            };'''),
    ('            let score = rank_score(body_rank);',
     '            let score = 0.75 * normalized_body.get(&node.id).copied().unwrap_or(0.0);'),
    ('fn rank_score(rank: usize) -> f32 {', HELPER + '\nfn rank_score(rank: usize) -> f32 {'),
]
