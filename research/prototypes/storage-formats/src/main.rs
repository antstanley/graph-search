//! Storage/serialisation measurements on one real generation.
//!
//! Usage: `storage-formats-prototype <store-dir> [results.json]`
//! where `<store-dir>` is a copy of `.graph-search/index` (never the live one:
//! nothing here writes to it, but the engine open takes a reader lease).

mod mirror;
mod engines;
// The production codec, compiled from the engine's source so the numbers below
// describe exactly what ships.
#[allow(dead_code, unreachable_pub, clippy::all)]
#[path = "../../../../crates/engine/src/record_codec.rs"]
mod record_codec;
use record_codec::{DecodeRecord as _, EncodeRecord as _};

use graph_search_types::source::SourceFileUnits;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RUNS: usize = 3;

/// Counts live and peak heap bytes, to attribute memory to one build step.
struct Counting;
static LIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let live = LIVE.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed) + layout.size();
        PEAK.fetch_max(live, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        LIVE.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new: usize) -> *mut u8 {
        if new > layout.size() {
            let live = LIVE.fetch_add(new - layout.size(), std::sync::atomic::Ordering::Relaxed) + new - layout.size();
            PEAK.fetch_max(live, std::sync::atomic::Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe { std::alloc::System.realloc(ptr, layout, new) }
    }
}
#[global_allocator]
static GLOBAL: Counting = Counting;

fn median<R>(runs: usize, mut f: impl FnMut() -> R) -> (Duration, R) {
    let mut times = Vec::with_capacity(runs);
    let mut last = None;
    for _ in 0..runs {
        let start = Instant::now();
        let result = f();
        times.push(start.elapsed());
        last = Some(result);
    }
    times.sort();
    (times[times.len() / 2], last.expect("runs > 0"))
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

// ------------------------------------------------------------ on-disk index

#[derive(Deserialize)]
struct Pointer {
    id: String,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RecordIndex {
    records: BTreeMap<String, RecordRef>,
}

#[derive(Deserialize)]
struct RecordRef {
    pack: String,
    offset: usize,
    len: usize,
}

struct Raw {
    packs: BTreeMap<String, Vec<u8>>,
    records: Vec<(String, String, usize, usize)>,
}

impl Raw {
    fn load(gen_dir: &Path) -> Self {
        let index: RecordIndex = serde_json::from_slice(
            &std::fs::read(gen_dir.join("source-units.json")).expect("source-units.json"),
        )
        .expect("record index");
        let mut packs = BTreeMap::new();
        let mut records = Vec::new();
        for (path, r) in index.records {
            packs.entry(r.pack.clone()).or_insert_with(|| {
                std::fs::read(gen_dir.join("source-records").join(&r.pack)).expect("pack")
            });
            records.push((path, r.pack, r.offset, r.len));
        }
        Self { packs, records }
    }
    fn slices(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.records
            .iter()
            .map(|(path, pack, off, len)| (path.as_str(), &self.packs[pack][*off..off + len]))
    }
    fn bytes(&self) -> usize {
        self.packs.values().map(Vec::len).sum()
    }
}

// ------------------------------------------------------------ results

#[derive(Serialize, Default)]
struct Results {
    corpus: BTreeMap<String, serde_json::Value>,
    open_phases_ms: BTreeMap<String, f64>,
    formats: Vec<FormatRow>,
    engines: Vec<engines::EngineRow>,
}

#[derive(Serialize, Clone)]
struct FormatRow {
    artifact: String,
    format: String,
    bytes: usize,
    encode_ms: f64,
    decode_ms: f64,
    roundtrip: String,
}

// ------------------------------------------------------------ codecs

type Enc<T> = Box<dyn Fn(&T) -> Result<Vec<u8>, String>>;
type Dec<T> = Box<dyn Fn(&[u8]) -> Result<T, String>>;

struct Codec<T> {
    name: String,
    enc: Enc<T>,
    dec: Dec<T>,
}

fn e<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn generic_codecs<T: Serialize + DeserializeOwned + 'static>() -> Vec<Codec<T>> {
    let mut codecs: Vec<Codec<T>> = vec![
        Codec {
            name: "json (current)".into(),
            enc: Box::new(|v| serde_json::to_vec(v).map_err(e)),
            dec: Box::new(|b| serde_json::from_slice(b).map_err(e)),
        },
        Codec {
            name: "msgpack (named)".into(),
            enc: Box::new(|v| rmp_serde::to_vec_named(v).map_err(e)),
            dec: Box::new(|b| rmp_serde::from_slice(b).map_err(e)),
        },
        Codec {
            name: "postcard".into(),
            enc: Box::new(|v| postcard::to_allocvec(v).map_err(e)),
            dec: Box::new(|b| postcard::from_bytes(b).map_err(e)),
        },
        Codec {
            name: "bincode2 (serde)".into(),
            enc: Box::new(|v| {
                bincode::serde::encode_to_vec(v, bincode::config::standard()).map_err(e)
            }),
            dec: Box::new(|b| {
                bincode::serde::decode_from_slice(b, bincode::config::standard())
                    .map(|(v, _)| v)
                    .map_err(e)
            }),
        },
    ];
    let bases: Vec<(&str, fn(&T) -> Result<Vec<u8>, String>, fn(&[u8]) -> Result<T, String>)> = vec![
        (
            "json",
            |v| serde_json::to_vec(v).map_err(e),
            |b| serde_json::from_slice(b).map_err(e),
        ),
        (
            "msgpack",
            |v| rmp_serde::to_vec_named(v).map_err(e),
            |b| rmp_serde::from_slice(b).map_err(e),
        ),
    ];
    for (name, enc, dec) in bases {
        codecs.extend(compressed(name, enc, dec));
    }
    codecs
}

fn compressed<T: 'static>(
    name: &str,
    enc: fn(&T) -> Result<Vec<u8>, String>,
    dec: fn(&[u8]) -> Result<T, String>,
) -> Vec<Codec<T>> {
    vec![
        Codec {
            name: format!("{name} + zstd-3"),
            enc: Box::new(move |v| zstd::bulk::compress(&enc(v)?, 3).map_err(e)),
            dec: Box::new(move |b| dec(&zstd::decode_all(b).map_err(e)?)),
        },
        Codec {
            name: format!("{name} + lz4"),
            enc: Box::new(move |v| Ok(lz4_flex::compress_prepend_size(&enc(v)?))),
            dec: Box::new(move |b| dec(&lz4_flex::decompress_size_prepended(b).map_err(e)?)),
        },
    ]
}

fn source_codecs() -> Vec<Codec<SourceFileUnits>> {
    let mut codecs = generic_codecs::<SourceFileUnits>();
    let enc: fn(&SourceFileUnits) -> Result<Vec<u8>, String> = |v| {
        let mut out = Vec::new();
        v.encode_record(&mut out).map_err(e)?;
        Ok(out)
    };
    let dec: fn(&[u8]) -> Result<SourceFileUnits, String> =
        |b| SourceFileUnits::decode_record(b).map_err(e);
    codecs.push(Codec {
        name: "GSR1 (production codec)".into(),
        enc: Box::new(enc),
        dec: Box::new(dec),
    });
    codecs.extend(compressed("GSR1", enc, dec));
    codecs
}

/// Round-trips every item; sizes are summed per record because records are the
/// unit of content addressing, reuse and byte-range reads in the engine.
fn bench_items<T: PartialEq>(
    artifact: &str,
    items: &[(&str, &T)],
    codecs: &[Codec<T>],
    rows: &mut Vec<FormatRow>,
) {
    for codec in codecs {
        let (encode, encoded) = median(RUNS, || {
            items
                .iter()
                .map(|(_, v)| (codec.enc)(v))
                .collect::<Result<Vec<_>, _>>()
        });
        let encoded = match encoded {
            Ok(encoded) => encoded,
            Err(error) => {
                rows.push(row(artifact, &codec.name, 0, encode, Duration::ZERO, format!("encode failed: {error}")));
                continue;
            }
        };
        let bytes = encoded.iter().map(Vec::len).sum();
        let (decode, decoded) = median(RUNS, || {
            encoded
                .iter()
                .map(|b| (codec.dec)(b))
                .collect::<Result<Vec<T>, _>>()
        });
        let roundtrip = match decoded {
            Err(error) => format!("decode failed: {}", truncate(&error)),
            Ok(decoded) => {
                let mismatches = decoded
                    .iter()
                    .zip(items)
                    .filter(|(d, (_, o))| d != o)
                    .count();
                if mismatches == 0 {
                    "ok".into()
                } else {
                    format!("{mismatches} records differ")
                }
            }
        };
        println!(
            "  {artifact:<14} {:<26} {:>10.2} MB  enc {:>8.1} ms  dec {:>8.1} ms  {roundtrip}",
            codec.name,
            bytes as f64 / 1e6,
            ms(encode),
            ms(decode)
        );
        rows.push(row(artifact, &codec.name, bytes, encode, decode, roundtrip));
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(90).collect()
}

fn row(artifact: &str, format: &str, bytes: usize, enc: Duration, dec: Duration, roundtrip: String) -> FormatRow {
    FormatRow {
        artifact: artifact.into(),
        format: format.into(),
        bytes,
        encode_ms: ms(enc),
        decode_ms: ms(dec),
        roundtrip,
    }
}

/// Compress whole ~8 MiB packs instead of records: better ratio, but a reader
/// must decompress a whole pack (or use a seekable framing) to read one record.
fn bench_pack_compression(
    artifact: &str,
    records: &[Vec<u8>],
    label: &str,
    rows: &mut Vec<FormatRow>,
) {
    let mut packs = vec![Vec::new()];
    for record in records {
        if packs.last().is_some_and(|p: &Vec<u8>| p.len() >= 8 << 20) {
            packs.push(Vec::new());
        }
        packs.last_mut().expect("pack").extend_from_slice(record);
    }
    for level in [3, 9] {
        let (encode, compressed) = median(RUNS, || {
            packs
                .iter()
                .map(|p| zstd::bulk::compress(p, level).expect("zstd"))
                .collect::<Vec<_>>()
        });
        let (decode, restored) = median(RUNS, || {
            compressed
                .iter()
                .map(|c| zstd::decode_all(&c[..]).expect("zstd"))
                .collect::<Vec<_>>()
        });
        let ok = restored == packs;
        let bytes = compressed.iter().map(Vec::len).sum::<usize>();
        let name = format!("{label} + zstd-{level} per 8MiB pack");
        println!(
            "  {artifact:<14} {name:<26} {:>10.2} MB  enc {:>8.1} ms  dec {:>8.1} ms  {}",
            bytes as f64 / 1e6,
            ms(encode),
            ms(decode),
            if ok { "ok (bytes only)" } else { "MISMATCH" }
        );
        rows.push(row(artifact, &name, bytes, encode, decode, if ok { "ok (decompress only; excludes record decode)".into() } else { "mismatch".into() }));
    }
}

fn bench_rkyv(sources: &[(&str, &SourceFileUnits)], rows: &mut Vec<FormatRow>) {
    use rkyv::rancor::Error;
    let mirrors: Vec<_> = sources.iter().map(|(_, s)| mirror::mirror(s)).collect();
    let (encode, archives) = median(RUNS, || {
        mirrors
            .iter()
            .map(|m| rkyv::to_bytes::<Error>(m).expect("rkyv"))
            .collect::<Vec<_>>()
    });
    let bytes = archives.iter().map(|a| a.len()).sum::<usize>();
    // Validation plus a full traversal that touches every posting, without
    // allocating: the cost a zero-copy reader pays instead of decoding.
    let (access, postings) = median(RUNS, || {
        let mut postings = 0usize;
        for archive in &archives {
            let file = rkyv::access::<mirror::ArchivedRFile, Error>(archive).expect("valid");
            for unit in file.units.iter() {
                for entry in unit.terms.iter().chain(unit.identifiers.iter()) {
                    postings += entry.1.len();
                }
            }
        }
        postings
    });
    let (deser, _) = median(RUNS, || {
        archives
            .iter()
            .map(|a| rkyv::from_bytes::<mirror::RFile, Error>(a).expect("rkyv"))
            .collect::<Vec<_>>()
    });
    println!(
        "  {:<14} {:<26} {:>10.2} MB  enc {:>8.1} ms  validate+walk {:>6.1} ms ({postings} lines)  owned-deser {:>7.1} ms",
        "source-records",
        "rkyv mirror (zero-copy)",
        bytes as f64 / 1e6,
        ms(encode),
        ms(access),
        ms(deser)
    );
    rows.push(row("source-records", "rkyv mirror: validate + walk (zero-copy)", bytes, encode, access, "n/a (mirror type; no owned conversion)".into()));
    rows.push(row("source-records", "rkyv mirror: owned deserialize", bytes, encode, deser, "n/a (mirror type)".into()));
}

// ------------------------------------------------------------ main

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let store_dir = PathBuf::from(args.get(1).expect("usage: <store-dir> [results.json]"));
    let out = args.get(2).map(PathBuf::from);
    let pointer: Pointer =
        serde_json::from_slice(&std::fs::read(store_dir.join("CURRENT")).expect("CURRENT"))
            .expect("pointer");
    let gen_dir = store_dir.join("generations").join(&pointer.id);
    let mut results = Results::default();
    if std::env::var_os("BODY_MEMORY").is_some() {
        use std::sync::atomic::Ordering::Relaxed;
        let sources = graph_search_engine::sidecar::load_sources(&gen_dir).expect("sources");
        let before = LIVE.load(Relaxed);
        PEAK.store(before, Relaxed);
        let body = graph_search_core::body::BodyIndex::new(&sources);
        let retained = LIVE.load(Relaxed) - before;
        let transient = PEAK.load(Relaxed) - before;
        println!("sources live {:.0} MB; BodyIndex retained {:.1} MB, peak during build {:.1} MB", before as f64 / 1e6, retained as f64 / 1e6, transient as f64 / 1e6);
        drop(body);
        return;
    }
    if std::env::var_os("ONLY_ENCODE").is_some() {
        // Profiling hook: encode every record repeatedly.
        let mut total = 0usize;
        let sources = graph_search_engine::sidecar::load_sources(&gen_dir).expect("sources");
        for _ in 0..8 {
            for source in sources.values() {
                let mut out = Vec::new();
                source.encode_record(&mut out).expect("encode");
                total += out.len();
            }
        }
        println!("encoded {total} bytes");
        return;
    }


    // ---------------------------------------------------------------- phases
    println!("== Where a cold GrafeoStore::open spends its time (warm page cache, median of {RUNS})");
    let phases = &mut results.open_phases_ms;
    let mut phase = |name: &str, d: Duration| {
        println!("  {name:<58} {:>9.1} ms", ms(d));
        phases.insert(name.into(), ms(d));
    };
    let options = graph_search_engine::StoreOptions {
        in_memory: false,
        read_only: true,
    };
    let (d, store) = median(RUNS, || {
        graph_search_engine::GrafeoStore::open(&store_dir, &options).expect("open")
    });
    drop(store);
    phase("TOTAL GrafeoStore::open (read-only)", d);

    let (d, pointer_bytes) = median(RUNS, || {
        pointer
            .files
            .keys()
            .map(|name| std::fs::read(gen_dir.join(name)).expect("artifact"))
            .collect::<Vec<_>>()
    });
    phase("read CURRENT-listed artifacts", d);
    let (d, _) = median(RUNS, || {
        pointer_bytes
            .iter()
            .map(|b| graph_search_core::hash::content_hash(b))
            .collect::<Vec<_>>()
    });
    phase("content-hash CURRENT-listed artifacts", d);

    let (d, sources) = median(RUNS, || {
        graph_search_engine::sidecar::load_sources(&gen_dir).expect("sources")
    });
    phase("sidecar::load_sources (read + verify + decode)", d);

    let native = serde_json::from_slice::<serde_json::Value>(
        &std::fs::read(gen_dir.join("source-units.json")).expect("index"),
    )
    .ok()
    .and_then(|v| v["format"].as_u64())
    .is_some_and(|f| f >= 3);
    let raw = if native { None } else { Some(Raw::load(&gen_dir)) };
    if let Some(raw) = &raw {
        let (d, _) = median(RUNS, || Raw::load(&gen_dir));
        phase("  of which: read source packs", d);
        let (d, _) = median(RUNS, || {
            raw.packs
                .values()
                .map(|b| graph_search_core::hash::content_hash(b))
                .collect::<Vec<_>>()
        });
        phase("  of which: content-hash packs", d);
        let (d, _) = median(RUNS, || {
            raw.slices()
                .map(|(_, b)| graph_search_core::hash::content_hash(b))
                .collect::<Vec<_>>()
        });
        phase("  of which: content-hash records (again)", d);
        let (d, _) = median(RUNS, || {
            raw.slices()
                .map(|(_, b)| serde_json::from_slice::<SourceFileUnits>(b).expect("json"))
                .collect::<Vec<_>>()
        });
        phase("  of which: JSON decode records", d);
    }
    // Hash throughput on the same bytes (the old JSON packs when available).
    let sample: Vec<u8> = match &raw {
        Some(raw) => raw.packs.values().flatten().copied().collect(),
        None => sources
            .values()
            .flat_map(|s| serde_json::to_vec(s).expect("json"))
            .collect(),
    };
    let (d, _) = median(RUNS, || {
        use sha2::Digest as _;
        sha2::Sha256::digest(&sample)
    });
    phase("  (hash) SHA-256, sha2 0.10 software path", d);
    let (d, _) = median(RUNS, || {
        use sha2_011::Digest as _;
        sha2_011::Sha256::digest(&sample)
    });
    phase("  (hash) SHA-256, sha2 0.11 (ARMv8 SHA2 instructions)", d);
    let (d, _) = median(RUNS, || blake3::hash(&sample));
    phase("  (hash) BLAKE3 (NEON here; SSE4.1/AVX2/AVX-512 on x86)", d);

    let (d, _) = median(RUNS, || graph_search_core::body::BodyIndex::new(&sources));
    phase("BodyIndex::new (derived postings)", d);
    let (d, occurrences) = median(RUNS, || {
        graph_search_engine::sidecar::load_occurrences(&gen_dir).expect("occurrences")
    });
    phase("sidecar::load_occurrences", d);
    let (d, _) = median(RUNS, || {
        graph_search_core::occurrences::OccurrenceIndex::new(&occurrences)
    });
    phase("OccurrenceIndex::new", d);
    let (d, dangling) = median(RUNS, || {
        graph_search_engine::sidecar::load_dangling(&gen_dir).expect("dangling")
    });
    phase("sidecar::load_dangling", d);

    results.corpus.insert("generation".into(), pointer.id.clone().into());
    results.corpus.insert("source_records".into(), sources.len().into());
    if let Some(raw) = &raw {
        results.corpus.insert("source_pack_bytes".into(), raw.bytes().into());
    }
    results
        .corpus
        .insert("source_units".into(), sources.values().map(|s| s.units.len()).sum::<usize>().into());

    // ---------------------------------------------------------------- formats
    println!("\n== Serialisation formats (encode/decode all items, median of {RUNS})");
    let items: Vec<(&str, &SourceFileUnits)> =
        sources.iter().map(|(k, v)| (k.as_str(), v)).collect();
    bench_items("source-records", &items, &source_codecs(), &mut results.formats);
    bench_rkyv(&items, &mut results.formats);
    let json_records: Vec<Vec<u8>> = items
        .iter()
        .map(|(_, v)| serde_json::to_vec(v).expect("json"))
        .collect();
    let gsr1_records: Vec<Vec<u8>> = items
        .iter()
        .map(|(_, v)| {
            let mut out = Vec::new();
            v.encode_record(&mut out).expect("encode");
            out
        })
        .collect();
    bench_pack_compression("source-records", &json_records, "json", &mut results.formats);
    bench_pack_compression("source-records", &gsr1_records, "GSR1", &mut results.formats);

    let whole = [("occurrences", &occurrences)];
    bench_items("occurrences", &whole, &generic_codecs(), &mut results.formats);
    let dangling_items = [("dangling", &dangling)];
    bench_items("dangling", &dangling_items, &generic_codecs(), &mut results.formats);
    if let Ok(Some(manifest)) = graph_search_engine::sidecar::load_manifest(&gen_dir) {
        let m = [("manifest", &manifest)];
        bench_items("manifest+facts", &m, &generic_codecs(), &mut results.formats);
    }
    let deps_bytes = match std::fs::read(gen_dir.join("dependencies.json.zst")) {
        Ok(frame) => zstd::decode_all(frame.as_slice()).expect("deps frame"),
        Err(_) => std::fs::read(gen_dir.join("dependencies.json")).expect("deps"),
    };
    let deps: Option<graph_search_core::dependencies::DependencyIndex> =
        serde_json::from_slice(&deps_bytes).expect("deps json");
    let d = [("dependencies", &deps)];
    bench_items("dependencies", &d, &generic_codecs(), &mut results.formats);

    // ---------------------------------------------------------------- engines
    println!("\n== Storage engines (GSR1 record bytes; fsync durability on every commit)");
    let paths: Vec<String> = items.iter().map(|(p, _)| (*p).to_owned()).collect();
    let scratch = std::env::temp_dir().join(format!("gs-storage-{}", std::process::id()));
    results.engines = engines::run(&scratch, &paths, &gsr1_records, &json_records);
    let _ = std::fs::remove_dir_all(&scratch);

    if let Some(out) = out {
        std::fs::write(&out, serde_json::to_vec_pretty(&results).expect("results")).expect("write");
        println!("\nwrote {}", out.display());
    }
}
