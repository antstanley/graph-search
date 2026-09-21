//! Native conjunction over sorted, unique document ordinals. Every inspected
//! posting is charged, including seek probes; only proven intersections emit.
use crate::{Result, work::WorkBudget};

/// Visits matching ordinals, with positions in the original list order. The
/// shortest list drives execution; monotonically advancing galloping seeks skip
/// nonmatching ranges. The callback charges admission before allocating output.
pub(crate) fn intersect<P>(
    lists: &[&[P]],
    document: impl Fn(&P) -> usize,
    accept: impl Fn(usize) -> bool,
    budget: &mut WorkBudget,
    mut emit: impl FnMut(usize, &[usize], &mut WorkBudget) -> Result<bool>,
) -> Result<()> {
    budget.check()?;
    let Some(anchor) = (0..lists.len()).min_by_key(|&i| lists[i].len()) else {
        return Ok(());
    };
    let mut order: Vec<_> = (0..lists.len()).filter(|&i| i != anchor).collect();
    order.sort_by_key(|&i| lists[i].len());
    let mut positions = vec![0; lists.len()];
    'anchors: for (position, posting) in lists[anchor].iter().enumerate() {
        if !budget.posting()? {
            return Ok(());
        }
        let target = document(posting);
        if !accept(target) {
            continue;
        }
        positions[anchor] = position;
        for &i in &order {
            let Some((position, found)) = seek(lists[i], positions[i], target, &document, budget)?
            else {
                return Ok(());
            };
            positions[i] = position;
            if found != target {
                continue 'anchors;
            }
        }
        if !emit(target, &positions, budget)? {
            break;
        }
        // Every list is unique and the next anchor is strictly greater. The
        // just-matched postings can never satisfy a future anchor.
        for &i in &order {
            positions[i] = positions[i].saturating_add(1);
        }
    }
    Ok(())
}

// None means the list is exhausted or the work budget stopped the seek. Both
// terminate conjunction; the budget itself records whether evidence is partial.
fn seek<P>(
    list: &[P],
    start: usize,
    target: usize,
    document: &impl Fn(&P) -> usize,
    budget: &mut WorkBudget,
) -> Result<Option<(usize, usize)>> {
    if start >= list.len() {
        return Ok(None);
    }
    if !budget.posting()? {
        return Ok(None);
    }
    let first = document(&list[start]);
    if first >= target {
        return Ok(Some((start, first)));
    }
    let mut low = start.saturating_add(1);
    let mut step = 1usize;
    let mut high;
    let mut upper = None;
    loop {
        high = start.saturating_add(step).min(list.len());
        if high == list.len() {
            break;
        }
        if !budget.posting()? {
            return Ok(None);
        }
        let value = document(&list[high]);
        if value >= target {
            upper = Some(value);
            break;
        }
        low = high.saturating_add(1);
        step = step.saturating_mul(2);
    }
    let boundary = high;
    while low < high {
        let middle = low.saturating_add(high.saturating_sub(low).div_euclid(2));
        if !budget.posting()? {
            return Ok(None);
        }
        if document(&list[middle]) < target {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    if low == list.len() {
        return Ok(None);
    }
    let found = if low == boundary {
        // A finite upper boundary was already inspected and charged above.
        upper.unwrap_or_default()
    } else {
        if !budget.posting()? {
            return Ok(None);
        }
        document(&list[low])
    };
    Ok(Some((low, found)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::WorkLimits;

    #[test]
    fn generated_intersections_equal_exhaustive_membership_and_preserve_list_positions() {
        for left in 0u32..64 {
            for right in 0u32..64 {
                let sets: Vec<Vec<usize>> = [left, right, left.rotate_left(2) ^ right]
                    .iter()
                    .map(|bits| {
                        (0..6)
                            .filter(|i| bits & (1 << i) != 0)
                            .map(|i| i * i * 17)
                            .collect()
                    })
                    .collect();
                for order in [[0, 1, 2], [2, 0, 1]] {
                    let lists: Vec<_> = order.iter().map(|&i| sets[i].as_slice()).collect();
                    let expected: Vec<_> = sets[0]
                        .iter()
                        .copied()
                        .filter(|d| d % 3 != 0 && sets.iter().all(|set| set.contains(d)))
                        .collect();
                    let mut actual = Vec::new();
                    let mut budget = WorkBudget::new(WorkLimits::default());
                    intersect(
                        &lists,
                        |&id| id,
                        |d| d % 3 != 0,
                        &mut budget,
                        |id, positions, budget| {
                            assert!(lists.iter().zip(positions).all(|(list, &i)| list[i] == id));
                            if !budget.candidate()? {
                                return Ok(false);
                            }
                            actual.push(id);
                            Ok(true)
                        },
                    )
                    .unwrap();
                    assert_eq!(actual, expected);
                    assert!(budget.report().2.is_empty());
                }
            }
        }
    }

    #[test]
    fn dense_conjunction_reads_each_matching_posting_once() {
        let dense: Vec<_> = (0..1000).collect();
        let mut budget = WorkBudget::new(WorkLimits::default());
        let mut matches = 0;
        intersect(
            &[&dense, &dense, &dense],
            |&id| id,
            |_| true,
            &mut budget,
            |_, _, _| {
                matches += 1;
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(matches, 1000);
        assert_eq!(budget.lexical_report().1, 3000);
        assert!(budget.report().2.is_empty());
    }

    #[test]
    fn selective_seeks_skip_common_postings_and_never_emit_unverified_candidates() {
        let common: Vec<_> = (0..100_000).collect();
        let rare = [123, 99_999];
        let lists = [common.as_slice(), rare.as_slice(), rare.as_slice()];
        let mut full = WorkBudget::new(WorkLimits::default());
        let mut expected = Vec::new();
        intersect(
            &lists,
            |&id| id,
            |_| true,
            &mut full,
            |id, _, _| {
                expected.push(id);
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(expected, rare);
        let examined = full.lexical_report().1;
        assert!(
            examined < 128,
            "{examined} probes instead of a 100,004-entry union"
        );
        for cap in 0..=examined {
            let mut budget = WorkBudget::new(WorkLimits {
                postings: usize::try_from(cap).unwrap(),
                ..WorkLimits::default()
            });
            let mut actual = Vec::new();
            intersect(
                &lists,
                |&id| id,
                |_| true,
                &mut budget,
                |id, _, _| {
                    actual.push(id);
                    Ok(true)
                },
            )
            .unwrap();
            assert_eq!(actual, expected[..actual.len()]);
            assert!(budget.lexical_report().1 <= cap);
            if cap < examined {
                assert!(!budget.report().2.is_empty());
            } else {
                assert!(budget.report().2.is_empty());
            }
        }
    }
}
