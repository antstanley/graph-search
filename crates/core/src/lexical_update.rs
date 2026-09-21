//! Immutable posting deltas for metadata changes. Source order may change between
//! generations, so reused frequencies must be remapped to the new ordinals.

use super::{Arc, BTreeMap, LexicalIndex, Node, Posting};

type LengthVectors = (Vec<usize>, Vec<[usize; 4]>, Vec<[usize; 4]>);

impl LexicalIndex {
    /// `reuse[new]` identifies an old document with identical analyzed fields.
    /// Only new/changed documents are analyzed; unchanged posting lists are shared.
    pub(crate) fn updated(&self, nodes: &[Node], reuse: &[Option<usize>]) -> Self {
        debug_assert_eq!(nodes.len(), reuse.len());
        let mut old_to_new = vec![None; self.lengths.len()];
        let mut changed = Vec::new();
        let mut changed_ordinals = Vec::new();
        for (new, old) in reuse.iter().enumerate() {
            if let Some(old) = old {
                old_to_new[*old] = Some(new);
            } else {
                changed.push(nodes[new].clone());
                changed_ordinals.push(new);
            }
        }
        let mut delta = Self::with_fields(&changed, self.fields.0, self.fields.1);
        let mut totals = self.totals;
        for (old, new) in old_to_new.iter().enumerate() {
            if new.is_none() {
                totals.combined = totals.combined.saturating_sub(self.lengths[old]);
                for field in 0..4 {
                    totals.fields[field] = totals.fields[field].saturating_sub(
                        self.field_lengths[old][field].max(self.whole_field_lengths[old][field]),
                    );
                }
            }
        }
        totals.combined = totals.combined.saturating_add(delta.totals.combined);
        for field in 0..4 {
            totals.fields[field] = totals.fields[field].saturating_add(delta.totals.fields[field]);
        }
        let (lengths, split, whole) = self.updated_lengths(reuse, &delta);
        for list in delta.postings.values_mut() {
            for posting in Arc::make_mut(list) {
                posting.document = changed_ordinals[posting.document];
            }
        }
        let altered: Vec<_> = old_to_new
            .iter()
            .enumerate()
            .filter(|(old, new)| **new != Some(*old))
            .map(|(old, _)| old)
            .collect();
        let mut postings = BTreeMap::new();
        for (term, list) in &self.postings {
            let additions = delta.postings.remove(term);
            if additions.is_none() && !touches(list, &altered, &old_to_new) {
                postings.insert(term.clone(), Arc::clone(list));
                continue;
            }
            let mut updated: Vec<_> = list
                .iter()
                .filter_map(|posting| {
                    old_to_new[posting.document].map(|document| Posting {
                        document,
                        ..*posting
                    })
                })
                .collect();
            if let Some(additions) = additions {
                updated.extend(additions.iter().copied());
            }
            updated.sort_unstable_by_key(|posting| posting.document);
            if !updated.is_empty() {
                postings.insert(term.clone(), Arc::new(updated));
            }
        }
        postings.extend(delta.postings);
        Self::from_parts(lengths, split, whole, postings, self.fields, Some(totals))
    }

    fn updated_lengths(&self, reuse: &[Option<usize>], delta: &Self) -> LengthVectors {
        let mut lengths = Vec::with_capacity(reuse.len());
        let mut split = Vec::with_capacity(reuse.len());
        let mut whole = Vec::with_capacity(reuse.len());
        let mut changed = 0usize;
        for old in reuse {
            let (source, ordinal) = if let Some(old) = old {
                (self, *old)
            } else {
                let position = changed;
                changed = changed.saturating_add(1);
                (delta, position)
            };
            lengths.push(source.lengths[ordinal]);
            split.push(source.field_lengths[ordinal]);
            whole.push(source.whole_field_lengths[ordinal]);
        }
        (lengths, split, whole)
    }
}

fn touches(list: &[Posting], altered: &[usize], old_to_new: &[Option<usize>]) -> bool {
    if altered.len() < list.len() {
        altered
            .iter()
            .any(|document| list.binary_search_by_key(document, |p| p.document).is_ok())
    } else {
        list.iter()
            .any(|p| old_to_new[p.document] != Some(p.document))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_search_types::FieldNormalization;

    fn node(name: &str) -> Node {
        Node {
            name: Some(name.into()),
            ..Node::default()
        }
    }

    fn equivalent(actual: &LexicalIndex, expected: &LexicalIndex) {
        assert_eq!(actual.totals.combined, expected.totals.combined);
        assert_eq!(actual.totals.fields, expected.totals.fields);
        assert_eq!(actual.lengths, expected.lengths);
        assert_eq!(actual.field_lengths, expected.field_lengths);
        assert_eq!(actual.whole_field_lengths, expected.whole_field_lengths);
        assert_eq!(
            actual.average_length.to_bits(),
            expected.average_length.to_bits()
        );
        assert_eq!(
            actual.average_field_lengths.map(f32::to_bits),
            expected.average_field_lengths.map(f32::to_bits)
        );
        assert_eq!(
            actual.postings.keys().collect::<Vec<_>>(),
            expected.postings.keys().collect::<Vec<_>>()
        );
        for (term, list) in &actual.postings {
            let other = &expected.postings[term];
            assert_eq!(list.len(), other.len());
            for (a, b) in list.iter().zip(other.iter()) {
                assert_eq!(
                    (a.document, a.frequency.to_bits(), a.fields, a.whole_fields),
                    (b.document, b.frequency.to_bits(), b.fields, b.whole_fields)
                );
            }
        }
        let terms: Vec<_> = actual.postings.keys().cloned().collect();
        for document in 0..actual.lengths.len() {
            for policy in [FieldNormalization::Combined, FieldNormalization::Bm25f] {
                assert_eq!(
                    actual
                        .score_with_normalization(document, &terms, policy)
                        .to_bits(),
                    expected
                        .score_with_normalization(document, &terms, policy)
                        .to_bits()
                );
            }
        }
    }

    #[test]
    fn unchanged_posting_lists_are_shared_without_mutating_prior_generations() {
        let old_nodes = vec![node("common Alpha"), node("common Beta"), node("Gamma")];
        let old = LexicalIndex::with_fields(&old_nodes, true, true);
        let same = old.updated(&old_nodes, &[Some(0), Some(1), Some(2)]);
        for (term, list) in &old.postings {
            assert!(Arc::ptr_eq(list, &same.postings[term]));
        }
        let mut nodes = old_nodes.clone();
        nodes[0].name = Some("common Delta".into());
        let next = old.updated(&nodes, &[None, Some(1), Some(2)]);
        assert!(!next.postings.contains_key("alpha"));
        assert!(next.postings.contains_key("delta"));
        assert!(Arc::ptr_eq(&old.postings["beta"], &next.postings["beta"]));
        assert!(Arc::ptr_eq(&old.postings["gamma"], &next.postings["gamma"]));
        assert!(!Arc::ptr_eq(
            &old.postings["common"],
            &next.postings["common"]
        ));
        equivalent(&next, &LexicalIndex::with_fields(&nodes, true, true));
        equivalent(&old, &LexicalIndex::with_fields(&old_nodes, true, true));
    }

    #[test]
    fn additions_deletions_reordering_and_empty_corpora_match_rebuilds() {
        let original = vec![
            node("common HTTPServer"),
            node("élève common"),
            node("is underscore_name"),
        ];
        for (whole, qualified) in [(false, false), (true, false), (false, true), (true, true)] {
            let old = LexicalIndex::with_fields(&original, whole, qualified);
            for reuse in [
                vec![],
                vec![Some(2)],
                vec![Some(2), Some(0), Some(1)],
                vec![None, Some(0), Some(2)],
                vec![Some(0), Some(1), Some(2), None],
                vec![None, None],
            ] {
                let nodes: Vec<_> = reuse
                    .iter()
                    .map(|old| old.map_or_else(|| node("new HTTPServer"), |i| original[i].clone()))
                    .collect();
                let next = old.updated(&nodes, &reuse);
                equivalent(&next, &LexicalIndex::with_fields(&nodes, whole, qualified));
            }
        }
    }
}
