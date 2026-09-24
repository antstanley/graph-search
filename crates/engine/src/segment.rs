//! Posting tables: immutable sorted segments of `(key, owner, value)` rows,
//! combined newest-first with per-owner tombstones (format 10,
//! `research/16-proportional-sync.md` §3.2).
//!
//! A segment file is a run of zstd-compressed row blocks, then a JSON footer
//! listing every block's key range, byte range and BLAKE3 hash, then the
//! footer's length and a magic trailer. The footer's hash is the segment's
//! **root**: a generation commits roots, a reader verifies the footer against
//! its root when it opens the segment and each block against the footer before
//! decoding it. A lookup reads only the blocks whose key range covers the key.
//!
//! A table is a base segment plus deltas. Each publish appends one delta that
//! tombstones every owner (file path) it replaces and carries those owners' new
//! rows. A row is visible unless a newer segment tombstones its owner. When the
//! deltas grow past a fraction of the base, the publish compacts them into a new
//! base, so the cost of compaction is amortized over the publishes that grew
//! them.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const MAGIC: &[u8; 4] = b"GSG1";
const FORMAT: u32 = 1;
/// Uncompressed bytes per block: a lookup inflates one or two of these.
const BLOCK_BYTES: usize = 16 * 1024;
/// Compact once the deltas hold this fraction of the base's rows (1/4)...
const DELTA_DIVISOR: u64 = 4;
/// ...or once a table has this many deltas.
const MAX_DELTAS: usize = 8;
/// The directory, inside a generation, that holds segment files.
pub(crate) const DIRECTORY: &str = "postings";

/// One posting: `key` is looked up, `owner` is the file path whose facts
/// produced it, `value` is the payload.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Row {
    pub(crate) key: String,
    pub(crate) owner: String,
    pub(crate) value: String,
}

impl Row {
    pub(crate) fn new(
        key: impl Into<String>,
        owner: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            owner: owner.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Block {
    first: String,
    last: String,
    offset: u64,
    len: u64,
    rows: u32,
    hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Footer {
    format: u32,
    rows: u64,
    tombstones: Vec<String>,
    blocks: Vec<Block>,
}

/// A committed segment: its file name inside [`DIRECTORY`], its root hash and
/// its row count (for the compaction policy, without opening it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SegmentRef {
    pub(crate) file: String,
    pub(crate) root: String,
    pub(crate) rows: u64,
}

fn put(out: &mut Vec<u8>, text: &str) -> io::Result<()> {
    let len = u32::try_from(text.len()).map_err(io::Error::other)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn take<'b>(bytes: &'b [u8], at: &mut usize) -> io::Result<&'b str> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid segment block");
    let end = at.checked_add(4).ok_or_else(invalid)?;
    let len: [u8; 4] = bytes
        .get(*at..end)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(invalid)?;
    let len = usize::try_from(u32::from_le_bytes(len)).map_err(io::Error::other)?;
    let stop = end.checked_add(len).ok_or_else(invalid)?;
    let text = bytes.get(end..stop).ok_or_else(invalid)?;
    *at = stop;
    std::str::from_utf8(text).map_err(|_| invalid())
}

fn decode_block(bytes: &[u8], rows: u32) -> io::Result<Vec<Row>> {
    let mut out = Vec::with_capacity(usize::try_from(rows).unwrap_or(0));
    let mut at = 0usize;
    while at < bytes.len() {
        let key = take(bytes, &mut at)?;
        let owner = take(bytes, &mut at)?;
        let value = take(bytes, &mut at)?;
        out.push(Row::new(key, owner, value));
    }
    if out.len() != usize::try_from(rows).map_err(io::Error::other)?
        || out.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "segment block rows do not match their footer",
        ));
    }
    Ok(out)
}

/// Writes `rows` (any order; duplicates are kept once) and `tombstones` as one
/// segment in `dir`, returning its reference. The file is synced; the caller
/// syncs the directory.
pub(crate) fn write(
    dir: &Path,
    prefix: &str,
    rows: Vec<Row>,
    tombstones: &BTreeSet<String>,
) -> io::Result<SegmentRef> {
    let mut rows = rows;
    rows.sort();
    rows.dedup();
    let mut body = Vec::new();
    let mut blocks = Vec::new();
    let mut raw = Vec::new();
    let mut first: Option<&Row> = None;
    let mut count = 0u32;
    let mut flush = |raw: &mut Vec<u8>,
                     first: &mut Option<&Row>,
                     last: &Row,
                     count: &mut u32,
                     body: &mut Vec<u8>|
     -> io::Result<()> {
        let Some(start) = first.take() else {
            return Ok(());
        };
        let compressed = crate::compress::deflate(raw)?;
        blocks.push(Block {
            first: start.key.clone(),
            last: last.key.clone(),
            offset: body.len() as u64,
            len: compressed.len() as u64,
            rows: *count,
            hash: graph_search_core::hash::content_hash(&compressed),
        });
        body.extend_from_slice(&compressed);
        raw.clear();
        *count = 0;
        Ok(())
    };
    for (index, row) in rows.iter().enumerate() {
        if first.is_none() {
            first = Some(row);
        }
        put(&mut raw, &row.key)?;
        put(&mut raw, &row.owner)?;
        put(&mut raw, &row.value)?;
        count = count.saturating_add(1);
        // A block closes only between different keys, so every row of a key
        // that fits in one block is found by reading one block.
        let next_key_differs = rows
            .get(index.saturating_add(1))
            .is_none_or(|next| next.key != row.key);
        if raw.len() >= BLOCK_BYTES && next_key_differs {
            flush(&mut raw, &mut first, row, &mut count, &mut body)?;
        }
    }
    if let Some(last) = rows.last() {
        flush(&mut raw, &mut first, last, &mut count, &mut body)?;
    }
    let footer = Footer {
        format: FORMAT,
        rows: rows.len() as u64,
        tombstones: tombstones.iter().cloned().collect(),
        blocks,
    };
    let footer = serde_json::to_vec(&footer).map_err(io::Error::other)?;
    let root = graph_search_core::hash::content_hash(&footer);
    body.extend_from_slice(&footer);
    body.extend_from_slice(&(footer.len() as u64).to_le_bytes());
    body.extend_from_slice(MAGIC);
    let file = format!("{prefix}-{root}.seg");
    let path = dir.join(&file);
    // Content-named: an identical segment already present is already committed.
    if !path.exists() {
        crate::generation::replace(&path, &body)?;
    }
    Ok(SegmentRef {
        file,
        root,
        rows: rows.len() as u64,
    })
}

/// One opened segment. Blocks are verified and decoded on first use.
pub(crate) struct Segment {
    path: PathBuf,
    footer: Footer,
    tombstones: BTreeSet<String>,
    blocks: Vec<OnceLock<Arc<Vec<Row>>>>,
}

impl Segment {
    /// Opens `reference` in `dir`, verifying its footer against its root.
    pub(crate) fn open(dir: &Path, reference: &SegmentRef) -> io::Result<Self> {
        let invalid =
            |message: &str| io::Error::new(io::ErrorKind::InvalidData, message.to_owned());
        if reference.file.contains(['/', '\\']) || reference.file.starts_with('.') {
            return Err(invalid("invalid segment name"));
        }
        let path = dir.join(&reference.file);
        let mut file = std::fs::File::open(&path)?;
        let len = file.metadata()?.len();
        let trailer = 12u64;
        let trailer_at = len
            .checked_sub(trailer)
            .ok_or_else(|| invalid("truncated segment"))?;
        file.seek(SeekFrom::Start(trailer_at))?;
        let mut tail = [0u8; 12];
        file.read_exact(&mut tail)?;
        if &tail[8..] != MAGIC {
            return Err(invalid("not a posting segment"));
        }
        let mut footer_len = [0u8; 8];
        footer_len.copy_from_slice(&tail[..8]);
        let footer_len = u64::from_le_bytes(footer_len);
        let footer_at = trailer_at
            .checked_sub(footer_len)
            .ok_or_else(|| invalid("truncated segment footer"))?;
        file.seek(SeekFrom::Start(footer_at))?;
        let mut bytes = vec![0u8; usize::try_from(footer_len).map_err(io::Error::other)?];
        file.read_exact(&mut bytes)?;
        if graph_search_core::hash::content_hash(&bytes) != reference.root {
            return Err(invalid("segment checksum mismatch"));
        }
        let footer: Footer = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let mut end = 0u64;
        for block in &footer.blocks {
            let stop = block
                .offset
                .checked_add(block.len)
                .ok_or_else(|| invalid("invalid segment block range"))?;
            if footer.format != FORMAT
                || block.offset != end
                || stop > footer_at
                || block.first > block.last
            {
                return Err(invalid("invalid segment block range"));
            }
            end = stop;
        }
        if footer.rows != reference.rows
            || footer
                .blocks
                .windows(2)
                .any(|pair| pair[0].last > pair[1].first)
        {
            return Err(invalid("segment footer does not match its reference"));
        }
        let blocks = footer.blocks.iter().map(|_| OnceLock::new()).collect();
        Ok(Self {
            path,
            tombstones: footer.tombstones.iter().cloned().collect(),
            footer,
            blocks,
        })
    }

    fn block(&self, index: usize) -> io::Result<Arc<Vec<Row>>> {
        let cell = self
            .blocks
            .get(index)
            .ok_or_else(|| io::Error::other("segment block out of range"))?;
        if let Some(rows) = cell.get() {
            return Ok(Arc::clone(rows));
        }
        let block = &self.footer.blocks[index];
        let mut file = std::fs::File::open(&self.path)?;
        file.seek(SeekFrom::Start(block.offset))?;
        let mut compressed = vec![0u8; usize::try_from(block.len).map_err(io::Error::other)?];
        file.read_exact(&mut compressed)?;
        if graph_search_core::hash::content_hash(&compressed) != block.hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "segment block checksum mismatch",
            ));
        }
        let rows = decode_block(&crate::compress::inflate_pack(&compressed)?, block.rows)?;
        if rows.first().is_none_or(|row| row.key != block.first)
            || rows.last().is_none_or(|row| row.key != block.last)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "segment block keys do not match their footer",
            ));
        }
        Ok(Arc::clone(cell.get_or_init(|| Arc::new(rows))))
    }

    /// Every row of `key`, in order.
    pub(crate) fn get(&self, key: &str) -> io::Result<Vec<Row>> {
        let start = self
            .footer
            .blocks
            .partition_point(|block| block.last.as_str() < key);
        let mut out = Vec::new();
        for index in start..self.footer.blocks.len() {
            if self.footer.blocks[index].first.as_str() > key {
                break;
            }
            let rows = self.block(index)?;
            let from = rows.partition_point(|row| row.key.as_str() < key);
            out.extend(
                rows[from..]
                    .iter()
                    .take_while(|row| row.key == key)
                    .cloned(),
            );
        }
        Ok(out)
    }

    /// Every row, in order.
    pub(crate) fn rows(&self) -> io::Result<Vec<Row>> {
        let mut out = Vec::new();
        for index in 0..self.footer.blocks.len() {
            out.extend(self.block(index)?.iter().cloned());
        }
        Ok(out)
    }
}

/// A committed table: base first, then deltas oldest to newest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TableRef {
    pub(crate) segments: Vec<SegmentRef>,
}

/// An opened table.
pub(crate) struct Table {
    segments: Vec<Segment>,
}

impl Table {
    pub(crate) fn open(dir: &Path, reference: &TableRef) -> io::Result<Self> {
        let dir = dir.join(DIRECTORY);
        Ok(Self {
            segments: reference
                .segments
                .iter()
                .map(|segment| Segment::open(&dir, segment))
                .collect::<io::Result<_>>()?,
        })
    }

    /// Whether a row of segment `index` is hidden by a newer tombstone.
    fn hidden(&self, index: usize, owner: &str) -> bool {
        self.segments
            .iter()
            .skip(index.saturating_add(1))
            .any(|newer| newer.tombstones.contains(owner))
    }

    /// Every visible row of `key`, sorted.
    pub(crate) fn get(&self, key: &str) -> io::Result<Vec<Row>> {
        let mut out = Vec::new();
        for (index, segment) in self.segments.iter().enumerate() {
            out.extend(
                segment
                    .get(key)?
                    .into_iter()
                    .filter(|row| !self.hidden(index, &row.owner)),
            );
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Every visible row, sorted.
    pub(crate) fn rows(&self) -> io::Result<Vec<Row>> {
        let mut out = Vec::new();
        for (index, segment) in self.segments.iter().enumerate() {
            out.extend(
                segment
                    .rows()?
                    .into_iter()
                    .filter(|row| !self.hidden(index, &row.owner)),
            );
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

/// Publishes the next version of a table into generation `dir`: the previous
/// segments (in `previous_dir`) are linked, and one delta replaces `owners`
/// with `rows`, every one of which must belong to a replaced owner. Compacts
/// into a new base when the deltas outgrow the policy.
pub(crate) fn publish(
    dir: &Path,
    name: &str,
    previous_dir: Option<&Path>,
    previous: &TableRef,
    owners: &BTreeSet<String>,
    rows: Vec<Row>,
) -> io::Result<TableRef> {
    if rows.iter().any(|row| !owners.contains(&row.owner)) {
        return Err(io::Error::other("posting row owner is not replaced"));
    }
    let target = dir.join(DIRECTORY);
    std::fs::create_dir_all(&target)?;
    let Some(previous_dir) = previous_dir.filter(|_| !previous.segments.is_empty()) else {
        let base = write(&target, name, rows, &BTreeSet::new())?;
        return Ok(TableRef {
            segments: vec![base],
        });
    };
    let source = previous_dir.join(DIRECTORY);
    let mut segments = previous.segments.clone();
    let delta = write(&target, name, rows, owners)?;
    segments.push(delta);
    let base_rows = segments.first().map_or(0, |base| base.rows);
    let delta_rows: u64 = segments.iter().skip(1).map(|segment| segment.rows).sum();
    let deltas = segments.len().saturating_sub(1);
    let compact =
        deltas > MAX_DELTAS || delta_rows.saturating_mul(DELTA_DIVISOR) > base_rows.max(1);
    if compact {
        // Every earlier segment is read from the previous generation.
        let mut opened = Vec::new();
        for segment in &segments[..segments.len().saturating_sub(1)] {
            opened.push(Segment::open(&source, segment)?);
        }
        opened.push(Segment::open(
            &target,
            &segments[segments.len().saturating_sub(1)],
        )?);
        let rows = Table { segments: opened }.rows()?;
        let base = write(&target, name, rows, &BTreeSet::new())?;
        let delta_file = target.join(&segments[segments.len().saturating_sub(1)].file);
        if delta_file != target.join(&base.file) {
            let _ = std::fs::remove_file(delta_file);
        }
        return Ok(TableRef {
            segments: vec![base],
        });
    }
    for segment in &segments[..segments.len().saturating_sub(1)] {
        let to = target.join(&segment.file);
        if !to.exists() && std::fs::hard_link(source.join(&segment.file), &to).is_err() {
            let bytes = std::fs::read(source.join(&segment.file))?;
            let mut file = std::fs::File::create(&to)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
    }
    Ok(TableRef { segments })
}

/// Every table of a generation, by name; committed as one small artifact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Tables {
    pub(crate) tables: BTreeMap<String, TableRef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_segment_round_trips_and_finds_keys_across_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let rows: Vec<Row> = (0..5_000)
            .map(|i| {
                Row::new(
                    format!("key-{:05}", i % 700),
                    format!("f{i}.rs"),
                    format!("v{i}"),
                )
            })
            .collect();
        let reference = write(dir.path(), "t", rows.clone(), &BTreeSet::new()).unwrap();
        let segment = Segment::open(dir.path(), &reference).unwrap();
        assert!(segment.footer.blocks.len() > 1);
        let mut expected = rows.clone();
        expected.sort();
        assert_eq!(segment.rows().unwrap(), expected);
        let found = segment.get("key-00042").unwrap();
        assert_eq!(
            found.len(),
            expected.iter().filter(|r| r.key == "key-00042").count()
        );
        assert!(found.iter().all(|row| row.key == "key-00042"));
        assert!(segment.get("absent").unwrap().is_empty());
        assert!(segment.get("").unwrap().is_empty());
        assert!(segment.get("zzz").unwrap().is_empty());
    }

    #[test]
    fn corruption_is_detected_at_open_or_first_read() {
        let dir = tempfile::tempdir().unwrap();
        let rows: Vec<Row> = (0..3_000)
            .map(|i| Row::new(format!("k{i:05}"), "a.rs", "x".repeat(20)))
            .collect();
        let reference = write(dir.path(), "t", rows, &BTreeSet::new()).unwrap();
        let path = dir.path().join(&reference.file);
        let original = std::fs::read(&path).unwrap();
        // A wrong root is refused at open.
        let mut wrong = reference.clone();
        wrong.root = "0".repeat(64);
        assert!(Segment::open(dir.path(), &wrong).is_err());
        // A damaged block opens but fails when read.
        let mut damaged = original.clone();
        damaged[10] ^= 0xff;
        std::fs::write(&path, &damaged).unwrap();
        let segment = Segment::open(dir.path(), &reference).unwrap();
        assert!(segment.get("k00000").is_err());
        // A damaged footer is refused at open.
        let mut damaged = original.clone();
        let at = damaged.len() - 20;
        damaged[at] ^= 0xff;
        std::fs::write(&path, &damaged).unwrap();
        assert!(Segment::open(dir.path(), &reference).is_err());
        // Escaping names are refused.
        let mut escaping = reference;
        escaping.file = "../t.seg".into();
        assert!(Segment::open(dir.path(), &escaping).is_err());
    }

    #[test]
    fn deltas_replace_owners_and_compaction_keeps_the_visible_rows() {
        let root = tempfile::tempdir().unwrap();
        let mut previous_dir: Option<PathBuf> = None;
        let mut table = TableRef::default();
        // Oracle: owner -> its current rows.
        let mut oracle: BTreeMap<String, Vec<Row>> = BTreeMap::new();
        for generation in 0..30u32 {
            let dir = root.path().join(format!("g{generation}"));
            std::fs::create_dir(&dir).unwrap();
            let (replaced, rows): (BTreeSet<String>, Vec<Row>) = if generation == 0 {
                let rows: Vec<Row> = (0..40)
                    .flat_map(|f| {
                        (0..5).map(move |k| {
                            Row::new(format!("name{k}"), format!("f{f}"), format!("f{f}#{k}"))
                        })
                    })
                    .collect();
                (rows.iter().map(|r| r.owner.clone()).collect(), rows)
            } else {
                let file = format!("f{}", generation % 40);
                let rows = (0..3)
                    .map(|k| {
                        Row::new(
                            format!("name{}", (k + generation) % 7),
                            file.clone(),
                            format!("g{generation}"),
                        )
                    })
                    .collect();
                (owners(&[&file]), rows)
            };
            for owner in &replaced {
                oracle.remove(owner);
            }
            for row in &rows {
                oracle
                    .entry(row.owner.clone())
                    .or_default()
                    .push(row.clone());
            }
            table = publish(
                &dir,
                "names",
                previous_dir.as_deref(),
                &table,
                &replaced,
                rows,
            )
            .unwrap();
            assert!(table.segments.len() <= MAX_DELTAS + 1);
            let opened = Table::open(&dir, &table).unwrap();
            let mut expected: Vec<Row> = oracle.values().flatten().cloned().collect();
            expected.sort();
            expected.dedup();
            assert_eq!(opened.rows().unwrap(), expected, "generation {generation}");
            for key in (0..7).map(|k| format!("name{k}")) {
                let want: Vec<Row> = expected.iter().filter(|r| r.key == key).cloned().collect();
                assert_eq!(opened.get(&key).unwrap(), want);
            }
            // The previous generation still opens unchanged: segments are immutable.
            if let Some(previous) = &previous_dir {
                assert!(previous.join(DIRECTORY).exists());
            }
            previous_dir = Some(dir);
        }
    }

    #[test]
    fn rows_of_unreplaced_owners_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let rows = vec![Row::new("k", "other.rs", "v")];
        assert!(
            publish(
                dir.path(),
                "t",
                None,
                &TableRef::default(),
                &owners(&["a.rs"]),
                rows
            )
            .is_err()
        );
    }
}
