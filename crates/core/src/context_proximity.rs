//! Bounded line proximity for marginal source-window utility, not phrase matching.

/// One new distinct query term contributes this many utility units.
pub(crate) const SCALE: u64 = 160;

/// Half a coverage unit, divided by the shortest inclusive line span containing
/// every distinct new term in this window. Repetition adds no extra term weight.
/// Callers provide sorted, unique lines, excluding already delivered term bits;
/// candidate windows contain at most `MAX_EVIDENCE_INTERVAL_LINES` entries.
// Counts are bounded by 80 input lines; sorted coordinates give positive widths.
// Integer truncation is the declared fixed-point scoring policy.
#[allow(clippy::arithmetic_side_effects, clippy::integer_division)]
pub(crate) fn bonus(positions: &[(u32, u128)]) -> u64 {
    let wanted = positions.iter().fold(0, |mask, (_, terms)| mask | terms);
    if wanted.count_ones() < 2 {
        return 0;
    }
    if positions.iter().any(|(_, terms)| *terms == wanted) {
        return SCALE / 2;
    }
    let mut counts = [0usize; 128];
    let mut present = 0u128;
    let mut left = 0;
    let mut shortest = u64::MAX;
    for &(line, mut terms) in positions {
        while terms != 0 {
            let bit = terms.trailing_zeros() as usize;
            counts[bit] += 1;
            present |= 1u128 << bit;
            terms &= terms - 1;
        }
        while present == wanted {
            shortest = shortest.min(u64::from(line) - u64::from(positions[left].0) + 1);
            // No individual line contains every term; two adjacent lines are
            // therefore the shortest possible cover for the remaining inputs.
            if shortest == 2 {
                return (SCALE / 2) / 2;
            }
            let mut terms = positions[left].1;
            while terms != 0 {
                let bit = terms.trailing_zeros() as usize;
                counts[bit] -= 1;
                if counts[bit] == 0 {
                    present &= !(1u128 << bit);
                }
                terms &= terms - 1;
            }
            left += 1;
        }
    }
    (SCALE / 2) / shortest
}

#[cfg(test)]
mod tests {
    #![allow(clippy::arithmetic_side_effects, clippy::integer_division)]
    use super::*;

    // Exhaustive interval enumeration, independent of the sliding counts.
    fn oracle(positions: &[(u32, u128)]) -> u64 {
        let wanted = positions.iter().fold(0, |mask, (_, terms)| mask | terms);
        if wanted.count_ones() < 2 {
            return 0;
        }
        let mut shortest = u64::MAX;
        for start in 0..positions.len() {
            for end in start..positions.len() {
                let mask = positions[start..=end]
                    .iter()
                    .fold(0, |mask, (_, terms)| mask | terms);
                if mask == wanted {
                    shortest = shortest
                        .min(u64::from(positions[end].0) - u64::from(positions[start].0) + 1);
                }
            }
        }
        (SCALE / 2) / shortest
    }

    #[test]
    fn all_small_masks_match_exhaustive_minimum_intervals() {
        // Every assignment of three terms to five lines, with two gap patterns.
        for assignment in 0u32..8u32.pow(5) {
            for coordinates in [[1, 2, 3, 4, 5], [1, 2, 9, 40, 80]] {
                let positions: Vec<_> = coordinates
                    .into_iter()
                    .enumerate()
                    .map(|(i, line)| (line, u128::from((assignment >> (i * 3)) & 7)))
                    .collect();
                assert_eq!(bonus(&positions), oracle(&positions), "{positions:?}");
            }
        }
    }

    #[test]
    fn repeated_terms_full_mask_and_coordinate_translation_preserve_contract() {
        assert_eq!(bonus(&[]), 0);
        assert_eq!(bonus(&[(1, 1), (2, 1), (3, 1)]), 0);
        assert_eq!(bonus(&[(1, 1), (50, 1), (51, 2)]), 40);
        assert_eq!(bonus(&[(1, u128::MAX)]), SCALE / 2);
        assert_eq!(bonus(&[(1, 1), (80, 1 << 127)]), 1);
        let masks = [(1, 1 << 127), (3, 1), (4, (1 << 127) | 1)];
        let shifted: Vec<_> = masks
            .iter()
            .map(|(line, terms)| (line + (u32::MAX - 4), *terms))
            .collect();
        assert_eq!(bonus(&masks), bonus(&shifted));
        // Removing already delivered bits eliminates novelty and its proximity bonus.
        let delivered: Vec<_> = masks
            .iter()
            .map(|(line, terms)| (*line, terms & !1))
            .collect();
        assert_eq!(bonus(&delivered), 0);
    }
}
