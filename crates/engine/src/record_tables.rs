//! Pack indexes as posting tables (format 14, `research/16-proportional-sync.md`
//! phase 1f).
//!
//! A pack family (shards, source facts, dependency records) keeps its records
//! in content-addressed packs. Which pack holds a file's record, and where, is
//! a row of the family's `paths` table, keyed by path. The same entry is also
//! a row of its `packs` table, keyed by pack, which gives a pack's members for
//! liveness and repacking. Packs below the family's small-pack size are rows
//! of its `small` table. All three are ordinary posting tables, so a publish
//! writes one delta each and touches only the changed files and the packs they
//! lived in. Nothing proportional to the workspace is written or read.
//!
//! Every entry names its record's content hash, so a reader verifies each
//! record it decodes; the pack's own hash is its file name.

use crate::record_codec::{DecodeRecord, EncodeRecord};
use crate::segment::{Row, Table};
use crate::source_records::{Layout, Writer};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// One pack family's layout and tables.
#[derive(Clone, Copy)]
pub(crate) struct Family {
    pub(crate) layout: Layout,
    /// Entry by path.
    pub(crate) paths: &'static str,
    /// Entry by pack: the pack's members.
    pub(crate) packs: &'static str,
    /// Packs below the small-pack size, under one key.
    pub(crate) small: &'static str,
}

impl Family {
    /// The family's tables.
    pub(crate) const fn tables(self) -> [&'static str; 3] {
        [self.paths, self.packs, self.small]
    }
}

/// The key every `small` row lives under.
const SMALL: &str = "small";

/// Where one file's record lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) pack: String,
    pub(crate) hash: String,
    pub(crate) offset: u64,
    pub(crate) len: u64,
    /// The pack's inflated size.
    pub(crate) size: u64,
}

impl Entry {
    fn encode(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.pack, self.hash, self.offset, self.len, self.size
        )
    }

    fn decode(value: &str) -> io::Result<Self> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid record entry");
        let mut parts = value.split('\t');
        let mut next = || parts.next().ok_or_else(invalid);
        let pack = next()?.to_owned();
        let hash = next()?.to_owned();
        let number = |text: &str| text.parse::<u64>().map_err(|_| invalid());
        let offset = number(next()?)?;
        let len = number(next()?)?;
        let size = number(next()?)?;
        if parts.next().is_some()
            || !crate::source_records::valid_hash(&pack)
            || !crate::source_records::valid_hash(&hash)
            || len == 0
            || offset.checked_add(len).is_none_or(|end| end > size)
        {
            return Err(invalid());
        }
        Ok(Self {
            pack,
            hash,
            offset,
            len,
            size,
        })
    }

    fn range(&self, pack_len: usize) -> io::Result<(usize, usize)> {
        let start = usize::try_from(self.offset).map_err(io::Error::other)?;
        let len = usize::try_from(self.len).map_err(io::Error::other)?;
        start
            .checked_add(len)
            .filter(|&end| end <= pack_len)
            .map(|end| (start, end))
            .ok_or_else(|| io::Error::other("record outside its pack"))
    }
}

/// A family's records in one generation: a view over its opened tables.
pub(crate) struct Records<'t> {
    family: Family,
    paths: &'t Table,
    packs: &'t Table,
    small: Option<&'t Table>,
}

impl<'t> Records<'t> {
    /// The family's records among `tables`, when it has any.
    pub(crate) fn open(family: Family, tables: &'t BTreeMap<String, Table>) -> Option<Self> {
        Some(Self {
            family,
            paths: tables.get(family.paths)?,
            packs: tables.get(family.packs)?,
            small: tables.get(family.small),
        })
    }

    /// Where `path`'s record lives.
    pub(crate) fn get(&self, path: &str) -> io::Result<Option<Entry>> {
        match self.paths.get(path)?.as_slice() {
            [] => Ok(None),
            [row] => Entry::decode(&row.value).map(Some),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a path has more than one record",
            )),
        }
    }

    /// Every path with a record, sorted.
    pub(crate) fn paths(&self) -> io::Result<Vec<String>> {
        let mut paths: Vec<String> = self.paths.rows()?.into_iter().map(|row| row.key).collect();
        paths.dedup();
        Ok(paths)
    }

    /// The members of `pack`, by path.
    fn members(&self, pack: &str) -> io::Result<Vec<(String, Entry)>> {
        self.packs
            .get(pack)?
            .into_iter()
            .map(|row| Ok((row.owner, Entry::decode(&row.value)?)))
            .collect()
    }

    /// The packs below the small-pack size, with their sizes.
    fn small_packs(&self) -> io::Result<BTreeMap<String, u64>> {
        let Some(table) = self.small else {
            return Ok(BTreeMap::new());
        };
        table
            .get(SMALL)?
            .into_iter()
            .map(|row| {
                let size = row.value.parse::<u64>().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid small-pack row")
                })?;
                Ok((row.owner, size))
            })
            .collect()
    }

    /// Every record, verified.
    pub(crate) fn load<T: DecodeRecord + DeserializeOwned>(
        &self,
        objects: &Path,
    ) -> io::Result<BTreeMap<String, T>> {
        let entries = self
            .paths
            .rows()?
            .into_iter()
            .map(|row| Ok((row.key, Entry::decode(&row.value)?)))
            .collect::<io::Result<Vec<_>>>()?;
        self.decode(objects, entries)
    }

    /// The records of `paths` that exist, verified; packs without a selected
    /// record are not read.
    pub(crate) fn load_selected<T: DecodeRecord + DeserializeOwned>(
        &self,
        objects: &Path,
        paths: &BTreeSet<String>,
    ) -> io::Result<BTreeMap<String, T>> {
        let mut entries = Vec::new();
        for path in paths {
            if let Some(entry) = self.get(path)? {
                entries.push((path.clone(), entry));
            }
        }
        self.decode(objects, entries)
    }

    fn decode<T: DecodeRecord + DeserializeOwned>(
        &self,
        objects: &Path,
        entries: Vec<(String, Entry)>,
    ) -> io::Result<BTreeMap<String, T>> {
        let mut grouped: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
        for (path, entry) in entries {
            grouped
                .entry(entry.pack.clone())
                .or_default()
                .push((path, entry));
        }
        let directory = objects.join(self.family.layout.directory);
        let mut out = BTreeMap::new();
        for (pack, group) in grouped {
            let bytes = read_pack(&directory, &pack)?;
            for (path, entry) in group {
                let record = verified(&bytes, &entry)?;
                out.insert(
                    path,
                    crate::source_records::decode(crate::source_records::NATIVE_FORMAT, record)?,
                );
            }
        }
        Ok(out)
    }
}

/// A pack's inflated bytes. The pack is named by its content hash, so a
/// corrupt file is caught before inflation.
fn read_pack(directory: &Path, pack: &str) -> io::Result<Vec<u8>> {
    let path = directory.join(pack);
    if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(io::Error::other("record pack is not a regular file"));
    }
    let file = std::fs::read(path)?;
    if graph_search_core::hash::content_hash(&file) != pack {
        return Err(io::Error::other("record pack checksum mismatch"));
    }
    crate::compress::inflate_pack(&file)
}

/// One record's bytes from its inflated pack, checked against its hash.
fn verified<'b>(bytes: &'b [u8], entry: &Entry) -> io::Result<&'b [u8]> {
    let (start, end) = entry.range(bytes.len())?;
    let record = bytes.get(start..end).unwrap_or_default();
    if graph_search_core::hash::content_hash(record) != entry.hash {
        return Err(io::Error::other("record checksum mismatch"));
    }
    Ok(record)
}

/// One publish's change to a family: the rows and replaced owners of each
/// table, and the packs it wrote and retired.
#[derive(Default)]
pub(crate) struct Delta {
    pub(crate) rows: BTreeMap<&'static str, Vec<Row>>,
    pub(crate) owners: BTreeMap<&'static str, BTreeSet<String>>,
    /// Packs this generation lists that the previous one did not.
    pub(crate) written: BTreeSet<String>,
    /// Packs the previous generation listed that this one does not.
    pub(crate) retired: BTreeSet<String>,
}

/// Records by path, with their entries.
type Members = Vec<(String, Entry)>;

/// The packs a publish retires or repacks.
struct Plan {
    /// Packs left without a record.
    retired: BTreeSet<String>,
    /// Packs to rewrite, with the records that survive in each.
    repack: BTreeMap<String, Vec<(String, Entry)>>,
    /// The previous small packs, with their sizes.
    small: BTreeMap<String, u64>,
}

/// Decides, from the packs `replaced` records lived in, which packs retire
/// and which are rewritten: one left below 75% live bytes, and every small
/// pack once two or more would survive.
fn plan(old: Option<&Records<'_>>, replaced: &BTreeSet<&str>) -> io::Result<Plan> {
    let Some(old) = old else {
        return Ok(Plan {
            retired: BTreeSet::new(),
            repack: BTreeMap::new(),
            small: BTreeMap::new(),
        });
    };
    let survivors = |pack: &str| -> io::Result<(u64, Members)> {
        let members = old.members(pack)?;
        let size = members.first().map_or(0, |(_, entry)| entry.size);
        let kept = members
            .into_iter()
            .filter(|(path, _)| !replaced.contains(path.as_str()))
            .collect();
        Ok((size, kept))
    };
    let mut touched: BTreeSet<String> = BTreeSet::new();
    for path in replaced {
        if let Some(entry) = old.get(path)? {
            touched.insert(entry.pack);
        }
    }
    let mut retired = BTreeSet::new();
    let mut repack = BTreeMap::new();
    for pack in touched {
        let (size, kept) = survivors(&pack)?;
        if kept.is_empty() {
            retired.insert(pack);
        } else if live_bytes(&kept)
            < crate::source_records::minimum_live(usize::try_from(size).map_err(io::Error::other)?)
        {
            repack.insert(pack, kept);
        }
    }
    let small = old.small_packs()?;
    let surviving: Vec<&String> = small
        .keys()
        .filter(|pack| !retired.contains(*pack) && !repack.contains_key(*pack))
        .collect();
    if surviving.len() > 1 {
        for pack in surviving {
            let (_, kept) = survivors(pack)?;
            repack.insert(pack.clone(), kept);
        }
    }
    Ok(Plan {
        retired,
        repack,
        small,
    })
}

/// Writes `records` as new records and drops the records of `dropped`, from
/// the family's records in `old` (see [`plan`] for repacking).
pub(crate) fn update<T: EncodeRecord>(
    objects: &Path,
    family: Family,
    old: Option<&Records<'_>>,
    records: &BTreeMap<String, T>,
    dropped: &BTreeSet<String>,
) -> io::Result<Delta> {
    let layout = family.layout;
    let directory = objects.join(layout.directory);
    std::fs::create_dir_all(&directory)?;
    let replaced: BTreeSet<&str> = records
        .keys()
        .map(String::as_str)
        .chain(dropped.iter().map(String::as_str))
        .collect();
    let Plan {
        mut retired,
        repack,
        small,
    } = plan(old, &replaced)?;

    // Survivors of repacked packs are copied byte for byte, then the new
    // records are encoded after them.
    let mut output = Writer::new(&directory, layout.pack_bytes);
    let mut moved: BTreeSet<String> = BTreeSet::new();
    for (pack, survivors) in &repack {
        retired.insert(pack.clone());
        if survivors.is_empty() {
            continue;
        }
        let bytes = read_pack(&directory, pack)?;
        for (path, entry) in survivors {
            output.add_encoded(path, verified(&bytes, entry)?)?;
            moved.insert(path.clone());
        }
    }
    for (path, record) in records {
        output.add(path, record)?;
    }
    output.flush()?;
    crate::generation::sync_dir(&directory)?;

    let owners: BTreeSet<String> = replaced
        .iter()
        .map(|path| (*path).to_owned())
        .chain(moved)
        .collect();
    finish(family, &output, owners, &retired, &small)
}

/// The delta for records `output` wrote in place of `owners`' old ones.
fn finish(
    family: Family,
    output: &Writer<'_>,
    owners: BTreeSet<String>,
    retired: &BTreeSet<String>,
    small: &BTreeMap<String, u64>,
) -> io::Result<Delta> {
    let mut path_rows = Vec::new();
    let mut pack_rows = Vec::new();
    for (path, packed) in &output.records {
        let size = output
            .sizes
            .get(&packed.pack)
            .copied()
            .ok_or_else(|| io::Error::other("written pack without a size"))?;
        let entry = Entry {
            pack: packed.pack.clone(),
            hash: packed.hash.clone(),
            offset: packed.offset,
            len: packed.len,
            size,
        }
        .encode();
        path_rows.push(Row::new(path.as_str(), path.as_str(), entry.as_str()));
        pack_rows.push(Row::new(packed.pack.as_str(), path.as_str(), entry));
    }
    let mut small_owners: BTreeSet<String> = retired
        .iter()
        .filter(|pack| small.contains_key(*pack))
        .cloned()
        .collect();
    let mut small_rows = Vec::new();
    for (pack, size) in &output.sizes {
        if *size < family.layout.small_pack_bytes() {
            small_owners.insert(pack.clone());
            small_rows.push(Row::new(SMALL, pack.as_str(), size.to_string()));
        }
    }
    let written: BTreeSet<String> = output.sizes.keys().cloned().collect();
    Ok(Delta {
        rows: BTreeMap::from([
            (family.paths, path_rows),
            (family.packs, pack_rows),
            (family.small, small_rows),
        ]),
        owners: BTreeMap::from([
            (family.paths, owners.clone()),
            (family.packs, owners),
            (family.small, small_owners),
        ]),
        // A rewritten pack identical to a retired one is the same object.
        retired: retired.difference(&written).cloned().collect(),
        written,
    })
}

/// A fresh family of the given encoded record bytes, which need not decode:
/// for tests that damage a record while keeping it authenticated.
#[cfg(test)]
pub(crate) fn raw(
    objects: &Path,
    family: Family,
    records: &BTreeMap<String, Vec<u8>>,
) -> io::Result<Delta> {
    let directory = objects.join(family.layout.directory);
    std::fs::create_dir_all(&directory)?;
    let mut output = Writer::new(&directory, family.layout.pack_bytes);
    for (path, bytes) in records {
        output.add_encoded(path, bytes)?;
    }
    output.flush()?;
    finish(
        family,
        &output,
        records.keys().cloned().collect(),
        &BTreeSet::new(),
        &BTreeMap::new(),
    )
}

/// Bytes of distinct ranges among `members`: records deduplicated within a
/// batch share one range.
fn live_bytes(members: &[(String, Entry)]) -> usize {
    let ranges: BTreeSet<(u64, u64)> = members
        .iter()
        .map(|(_, entry)| (entry.offset, entry.len))
        .collect();
    ranges.iter().fold(0usize, |total, (_, len)| {
        total.saturating_add(usize::try_from(*len).unwrap_or(usize::MAX))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segment::{TableRef, Tables};
    use graph_search_types::source::SourceFileUnits;

    const FAMILY: Family = Family {
        layout: Layout {
            directory: "test-records",
            pack_bytes: 1024,
        },
        paths: "test_paths",
        packs: "test_packs",
        small: "test_small",
    };

    fn record(text: &str) -> SourceFileUnits {
        SourceFileUnits {
            source_hash: text.repeat(8),
            version: 1,
            ..SourceFileUnits::default()
        }
    }

    fn publish(
        objects: &Path,
        refs: &mut Tables,
        writes: &BTreeMap<String, SourceFileUnits>,
        dropped: &BTreeSet<String>,
    ) {
        let opened: BTreeMap<String, Table> = refs
            .tables
            .iter()
            .map(|(name, reference)| (name.clone(), Table::open(objects, reference).unwrap()))
            .collect();
        let delta = update(
            objects,
            FAMILY,
            Records::open(FAMILY, &opened).as_ref(),
            writes,
            dropped,
        )
        .unwrap();
        let previous = (!refs.tables.is_empty()).then_some(objects);
        for table in FAMILY.tables() {
            let reference = crate::segment::publish(
                objects,
                table,
                previous,
                refs.tables.get(table).unwrap_or(&TableRef::default()),
                &delta.owners[table],
                delta.rows[table].clone(),
            )
            .unwrap();
            refs.tables.insert(table.to_owned(), reference);
        }
    }

    #[test]
    fn churn_bounds_dead_bytes_and_small_pack_count() {
        let root = tempfile::tempdir().unwrap();
        let objects = root.path();
        let mut files: BTreeMap<String, SourceFileUnits> = (0..32)
            .map(|i| (format!("{i:02}"), record(&i.to_string())))
            .collect();
        let mut refs = Tables::default();
        publish(objects, &mut refs, &files, &BTreeSet::new());
        for edit in 0..40 {
            let path = format!("{:02}", edit % 32);
            let mut writes = BTreeMap::new();
            let mut dropped = BTreeSet::new();
            if files.contains_key(&path) {
                let value = record(&format!("edit-{edit}"));
                files.insert(path.clone(), value.clone());
                writes.insert(path, value);
            }
            if edit == 20 {
                dropped = files
                    .keys()
                    .filter(|path| path.as_str() >= "08")
                    .cloned()
                    .collect();
                files.retain(|path, _| path.as_str() < "08");
                writes.retain(|path, _| !dropped.contains(path));
            }
            publish(objects, &mut refs, &writes, &dropped);
            let opened: BTreeMap<String, Table> = refs
                .tables
                .iter()
                .map(|(name, reference)| (name.clone(), Table::open(objects, reference).unwrap()))
                .collect();
            let records = Records::open(FAMILY, &opened).unwrap();
            assert_eq!(records.load::<SourceFileUnits>(objects).unwrap(), files);
            let mut packs: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
            for path in records.paths().unwrap() {
                let entry = records.get(&path).unwrap().unwrap();
                packs
                    .entry(entry.pack.clone())
                    .or_default()
                    .push((path, entry));
            }
            let small = packs
                .values()
                .filter(|members| members[0].1.size < FAMILY.layout.small_pack_bytes())
                .count();
            assert!(small <= 2, "edit {edit}: {small} small packs");
            for members in packs.values() {
                let size = usize::try_from(members[0].1.size).unwrap();
                if (size as u64) >= FAMILY.layout.small_pack_bytes() {
                    assert!(
                        live_bytes(members) >= crate::source_records::minimum_live(size),
                        "edit {edit}: a pack below 75% live bytes survived"
                    );
                }
            }
            assert_eq!(
                records.small_packs().unwrap().len(),
                small,
                "edit {edit}: the small table tracks the small packs"
            );
        }
    }
}
