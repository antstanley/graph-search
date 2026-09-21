//! Research-only helpers, injected into a disposable build by storage_composition.py.
use serde_json::{Value, json};
use std::mem::size_of;

pub fn lists<'a>(lists: impl Iterator<Item = (&'a str, usize, usize)>, item_bytes: usize) -> Value {
    let mut counts = [0usize; 5];
    let mut entries = [0usize; 5];
    let (mut terms, mut term_bytes, mut len, mut cap) = (0, 0, 0, 0);
    let mut previous = String::new();
    let mut prefix_suffix_bytes = 0;
    for (term, length, capacity) in lists {
        let bucket = match length { 0..=1 => 0, 2..=4 => 1, 5..=16 => 2, 17..=128 => 3, _ => 4 };
        counts[bucket] += 1;
        entries[bucket] += length;
        terms += 1;
        term_bytes += term.len();
        len += length;
        cap += capacity;
        let shared = previous.bytes().zip(term.bytes()).take_while(|(a,b)| a == b).count();
        prefix_suffix_bytes += term.len() - shared;
        previous.clear();
        previous.push_str(term);
    }
    json!({"terms":terms,"term_utf8_bytes":term_bytes,"entries":len,
        "item_size":item_bytes,"vector_length_bytes":len * item_bytes,
        "vector_capacity_bytes":cap * item_bytes,
        "list_count_by_length_1_4_16_128_more":counts,
        "entry_count_by_length_1_4_16_128_more":entries,
        "key_string_headers_bytes":terms * size_of::<String>(),
        "list_vec_headers_bytes":terms * size_of::<Vec<usize>>(),
        "front_coded_suffix_bytes_lower_bound":prefix_suffix_bytes})
}

pub fn deltas(lists: impl Iterator<Item = Vec<usize>>) -> Value {
    let (mut entries, mut bytes, mut maximum) = (0, 0, 0);
    for list in lists {
        let mut previous = 0;
        for (i, document) in list.into_iter().enumerate() {
            assert!(i == 0 || document > previous);
            let mut delta = document - previous;
            maximum = maximum.max(document);
            previous = document;
            entries += 1;
            bytes += 1;
            while delta >= 128 { bytes += 1; delta >>= 7; }
        }
    }
    json!({"entries":entries,"maximum_ordinal":maximum,
        "plain_usize_bytes":entries * size_of::<usize>(),
        "delta_varint_bytes_lower_bound":bytes,
        "note":"Doc IDs only: excludes payload, offsets, skips, checksums and decoding costs; no codec implemented."})
}
