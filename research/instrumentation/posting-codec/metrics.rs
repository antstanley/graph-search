//! Disposable-build adapter; no source strings or IDs are emitted.
use crate::research_codec_impl::Encoded;
use serde_json::{Value, json};
use std::{hint::black_box, mem::size_of, time::Instant};

fn fold(hash: u64, value: usize) -> u64 {
    (hash ^ value as u64).wrapping_mul(1099511628211)
}

pub fn measure(lists: impl Iterator<Item = Vec<usize>>) -> Value {
    let mut buckets: [Vec<Vec<usize>>; 5] = Default::default();
    let mut counts = [0usize; 5];
    let mut entries = [0usize; 5];
    let mut packed = [0usize; 5];
    let mut capacities = [0usize; 5];
    let mut hash = 14695981039346656037u64;
    let mut state = 8128u64;
    for documents in lists {
        let bucket = match documents.len() {
            0..=1 => 0,
            2..=4 => 1,
            5..=16 => 2,
            17..=128 => 3,
            _ => 4,
        };
        counts[bucket] += 1;
        entries[bucket] += documents.len();
        let encoded = Encoded::new(&documents).unwrap();
        packed[bucket] += encoded.payload_bytes();
        capacities[bucket] += encoded.capacity_bytes();
        let mut ordinal = 0;
        encoded
            .visit(|value| {
                assert_eq!(Some(&value), documents.get(ordinal));
                ordinal += 1;
            })
            .unwrap();
        assert_eq!(ordinal, documents.len());
        for target in [
            0,
            documents.last().copied().unwrap_or(0) / 2,
            documents.last().copied().unwrap_or(0),
            documents.last().copied().unwrap_or(0).saturating_add(1),
            usize::MAX,
        ] {
            assert_eq!(
                encoded.seek(target),
                documents
                    .get(documents.partition_point(|&x| x < target))
                    .copied()
            );
        }
        hash = fold(hash, documents.len());
        for &document in &documents {
            hash = fold(hash, document);
        }
        // Deterministic bounded reservoir per list-size bucket, not alphabetical prefixes.
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let selected = ((state >> 32) as usize) % counts[bucket];
        if buckets[bucket].len() < 64 {
            buckets[bucket].push(documents);
        } else if selected < 64 {
            buckets[bucket][selected] = documents;
        }
    }
    let mut timings = Vec::new();
    for (bucket, samples) in buckets.iter().enumerate() {
        let encoded: Vec<_> = samples.iter().map(|v| Encoded::new(v).unwrap()).collect();
        let targets: Vec<Vec<usize>> = samples
            .iter()
            .map(|v| {
                let maximum = v.last().copied().unwrap_or(0);
                (0..64)
                    .map(|i| match i % 4 {
                        0 => v.get(i * v.len() / 64).copied().unwrap_or(0),
                        1 => v
                            .get(i * v.len() / 64)
                            .copied()
                            .unwrap_or(0)
                            .saturating_add(1),
                        2 => maximum.saturating_mul(i) / 63,
                        _ => maximum.saturating_add(1),
                    })
                    .collect()
            })
            .collect();
        for (v, (encoded, targets)) in samples.iter().zip(encoded.iter().zip(&targets)) {
            for &target in targets {
                assert_eq!(
                    encoded.seek(target),
                    v.get(v.partition_point(|&x| x < target)).copied()
                );
            }
        }
        let mut rows = Vec::new();
        for repeat in 0..6 {
            // Alternate execution order. Both scan kernels compute the same checksum.
            let mut row = serde_json::Map::new();
            for arm in if repeat % 2 == 0 { [0, 1] } else { [1, 0] } {
                let started = Instant::now();
                let mut checksum = 0u64;
                for _ in 0..16 {
                    for (plain, compressed) in samples.iter().zip(&encoded) {
                        if arm == 0 {
                            for &value in black_box(plain) {
                                checksum = fold(checksum, value);
                            }
                        } else {
                            black_box(compressed)
                                .visit(|value| checksum = fold(checksum, value))
                                .unwrap();
                        }
                    }
                }
                black_box(checksum);
                row.insert(
                    format!("{}_scan_ns", if arm == 0 { "plain" } else { "codec" }),
                    json!(started.elapsed().as_nanos()),
                );
                row.insert(format!("{}_scan_checksum", arm), json!(checksum));
                let started = Instant::now();
                let mut checksum = 0u64;
                for _ in 0..16 {
                    for ((plain, compressed), targets) in samples.iter().zip(&encoded).zip(&targets)
                    {
                        for &target in black_box(targets) {
                            let result = if arm == 0 {
                                plain.get(plain.partition_point(|&x| x < target)).copied()
                            } else {
                                compressed.seek(target)
                            };
                            checksum = fold(checksum, result.unwrap_or(usize::MAX));
                        }
                    }
                }
                black_box(checksum);
                row.insert(
                    format!("{}_seek_ns", if arm == 0 { "plain" } else { "codec" }),
                    json!(started.elapsed().as_nanos()),
                );
                row.insert(format!("{}_seek_checksum", arm), json!(checksum));
                let started = Instant::now();
                for _ in 0..16 {
                    for plain in samples {
                        if arm == 0 {
                            black_box(black_box(plain).clone());
                        } else {
                            black_box(Encoded::new(black_box(plain)).unwrap());
                        }
                    }
                }
                row.insert(
                    format!("{}_build_ns", if arm == 0 { "plain" } else { "codec" }),
                    json!(started.elapsed().as_nanos()),
                );
            }
            assert_eq!(row["0_scan_checksum"], row["1_scan_checksum"]);
            assert_eq!(row["0_seek_checksum"], row["1_seek_checksum"]);
            rows.push(row);
        }
        timings.push(json!({"bucket":bucket,"sample_lists":samples.len(),
            "sample_entries":samples.iter().map(Vec::len).sum::<usize>(),
            "scan_repeats":16,"seek_repeats":16,"targets_per_list":64,"rows":rows}));
    }
    json!({"list_counts":counts,"entry_counts":entries,"plain_payload_bytes":entries.map(|n|n*size_of::<usize>()),
        "codec_payload_including_restarts_bytes":packed,"codec_capacity_including_restarts_bytes":capacities,
        "plain_inline_bytes_per_list":size_of::<Vec<usize>>(),"codec_inline_bytes_per_list":Encoded::inline_bytes(),
        "ordinal_checksum":format!("{hash:016x}"),"timings":timings,
        "contract":"doc ordinals only; 128-entry restart blocks; no TF/positions, persistent offsets, checksums, allocator overhead or query integration; timings are kernel microbenchmarks"})
}
