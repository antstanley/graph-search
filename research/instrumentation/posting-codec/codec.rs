//! Research-only scalar posting ordinals. No persisted format or production API.
//! Each 128-entry block restarts at an absolute ordinal; other values are deltas.
use std::mem::size_of;

const BLOCK: usize = 128;

#[derive(Clone, Debug)]
struct Restart {
    first: usize,
    offset: usize,
}

#[derive(Clone, Debug)]
pub struct Encoded {
    bytes: Vec<u8>,
    restarts: Vec<Restart>,
    len: usize,
}

fn put(mut value: usize, bytes: &mut Vec<u8>) {
    while value >= 128 {
        bytes.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    bytes.push(value as u8);
}

fn get(bytes: &[u8], offset: &mut usize) -> Option<usize> {
    let mut value = 0usize;
    for shift in (0..usize::BITS).step_by(7) {
        let byte = *bytes.get(*offset)?;
        *offset += 1;
        let part = usize::from(byte & 127);
        if part > usize::MAX >> shift {
            return None;
        }
        value |= part << shift;
        if byte & 128 == 0 {
            return (shift == 0 || part != 0).then_some(value);
        }
    }
    None
}

impl Encoded {
    pub fn new(documents: &[usize]) -> Option<Self> {
        if documents.windows(2).any(|pair| pair[0] >= pair[1]) {
            return None;
        }
        let mut bytes = Vec::new();
        let mut restarts = Vec::new();
        for block in documents.chunks(BLOCK) {
            restarts.push(Restart {
                first: block[0],
                offset: bytes.len(),
            });
            put(block[0], &mut bytes);
            for pair in block.windows(2) {
                put(pair[1] - pair[0], &mut bytes);
            }
        }
        Some(Self {
            bytes,
            restarts,
            len: documents.len(),
        })
    }

    /// Count and validate every decoded ordinal, with no output allocation.
    pub fn visit(&self, mut visitor: impl FnMut(usize)) -> Option<()> {
        let mut previous = None;
        let mut offset = 0;
        for (block, restart) in self.restarts.iter().enumerate() {
            if offset != restart.offset {
                return None;
            }
            let mut document = get(&self.bytes, &mut offset)?;
            if document != restart.first || previous.is_some_and(|p| p >= document) {
                return None;
            }
            visitor(document);
            for _ in 1..BLOCK.min(self.len.checked_sub(block.checked_mul(BLOCK)?)?) {
                let delta = get(&self.bytes, &mut offset)?;
                if delta == 0 {
                    return None;
                }
                document = document.checked_add(delta)?;
                visitor(document);
            }
            previous = Some(document);
        }
        (self.restarts.len() == self.len.div_ceil(BLOCK) && offset == self.bytes.len())
            .then_some(())
    }

    /// Lower bound over a trusted encoder-produced list; at most one block decoded.
    pub fn seek(&self, target: usize) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let block = self
            .restarts
            .partition_point(|r| r.first <= target)
            .saturating_sub(1);
        let mut offset = self.restarts[block].offset;
        let mut document = get(&self.bytes, &mut offset)?;
        if document >= target {
            return Some(document);
        }
        for _ in 1..BLOCK.min(self.len - block * BLOCK) {
            document = document.checked_add(get(&self.bytes, &mut offset)?)?;
            if document >= target {
                return Some(document);
            }
        }
        self.restarts.get(block + 1).map(|r| r.first)
    }

    pub fn payload_bytes(&self) -> usize {
        self.bytes.len() + self.restarts.len() * size_of::<Restart>()
    }

    pub fn capacity_bytes(&self) -> usize {
        self.bytes.capacity() + self.restarts.capacity() * size_of::<Restart>()
    }

    pub fn inline_bytes() -> usize {
        size_of::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(values: &[usize], targets: impl IntoIterator<Item = usize>) {
        let encoded = Encoded::new(values).unwrap();
        let mut decoded = Vec::new();
        assert_eq!(encoded.visit(|x| decoded.push(x)), Some(()));
        assert_eq!(decoded, values);
        for target in targets {
            let expected = values.get(values.partition_point(|&x| x < target)).copied();
            assert_eq!(encoded.seek(target), expected, "target {target}");
        }
    }

    #[test]
    fn empty_boundaries_extremes_and_every_gap() {
        check(&[], [0, usize::MAX]);
        check(&[0], [0, 1, usize::MAX]);
        check(&[usize::MAX], [0, usize::MAX - 1, usize::MAX]);
        check(
            &[0, 127, 128, 16383, 16384, usize::MAX],
            [0, 1, 127, 128, 129, 16383, 16384, usize::MAX],
        );
        for len in [1, 127, 128, 129, 255, 256, 257, 4096] {
            let values: Vec<_> = (0..len).map(|i| i * 3 + 7).collect();
            check(&values, 0..len * 3 + 10);
        }
    }

    #[test]
    fn deterministic_nonuniform_gaps() {
        let mut state = 12345u64;
        let mut value = 0;
        let values: Vec<_> = (0..10000)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                value += 1 + ((state >> 32) as usize % 10000);
                value
            })
            .collect();
        check(&values, values.iter().flat_map(|&v| [v - 1, v, v + 1]));
    }

    #[test]
    fn reject_nonmonotonic_and_malformed_encodings() {
        assert!(Encoded::new(&[1, 1]).is_none());
        assert!(Encoded::new(&[2, 1]).is_none());
        for bytes in [vec![], vec![128], vec![128, 0], vec![255; 20]] {
            assert!(get(&bytes, &mut 0).is_none());
        }
        let valid = Encoded::new(&[usize::MAX - 1, usize::MAX]).unwrap();
        let mut overflow = valid.clone();
        *overflow.bytes.last_mut().unwrap() = 2;
        assert!(overflow.visit(|_| {}).is_none());
        let mut duplicate = valid.clone();
        *duplicate.bytes.last_mut().unwrap() = 0;
        assert!(duplicate.visit(|_| {}).is_none());
        let mut trailing = valid.clone();
        trailing.bytes.push(0);
        assert!(trailing.visit(|_| {}).is_none());
        let mut truncated = valid.clone();
        truncated.bytes.pop();
        assert!(truncated.visit(|_| {}).is_none());
        let mut count = valid;
        count.len = 0;
        assert!(count.visit(|_| {}).is_none());
    }
}
