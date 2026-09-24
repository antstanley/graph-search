//! Immutable, content-addressed source packs with independently hashed file records.
//! CURRENT commits the index, which transitively commits pack and record bytes.

use crate::record_codec::{DecodeRecord, EncodeRecord};
#[cfg(test)]
use graph_search_types::source::SourceFileUnits;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};

#[cfg(test)]
type Files = BTreeMap<String, SourceFileUnits>;
const DIRECTORY: &str = "source-records";
pub(crate) const SOURCE_LAYOUT: Layout = Layout {
    index: crate::sidecar::SOURCE_FILE,
    directory: DIRECTORY,
    pack_bytes: PACK_BYTES,
};

/// Artifact names are fixed by the owning storage module, never read from disk.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    pub(crate) index: &'static str,
    pub(crate) directory: &'static str,
    /// Target uncompressed pack size. A selective read inflates one whole pack,
    /// so facts read one file at a time use small packs.
    pub(crate) pack_bytes: usize,
}
/// Default pack size for facts that are read in bulk.
pub(crate) const PACK_BYTES: usize = 8 * 1024 * 1024;

impl Layout {
    /// Packs below this size are combined when more than one is reused.
    fn small_pack_bytes(self) -> u64 {
        (self.pack_bytes >> 3) as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Packed {
    pack: String,
    hash: String,
    offset: u64,
    len: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum Reference {
    Single(String),
    Packed(Packed),
}

impl Reference {
    fn pack(&self) -> &str {
        match self {
            Self::Single(hash) => hash,
            Self::Packed(record) => &record.pack,
        }
    }
    fn hash(&self) -> &str {
        match self {
            Self::Single(hash) => hash,
            Self::Packed(record) => &record.hash,
        }
    }
    fn range(&self, pack_len: usize) -> io::Result<(usize, usize)> {
        match self {
            Self::Single(_) => Ok((0, pack_len)),
            Self::Packed(record) => {
                let start = usize::try_from(record.offset).map_err(io::Error::other)?;
                let len = usize::try_from(record.len).map_err(io::Error::other)?;
                let end = start
                    .checked_add(len)
                    .filter(|&end| end <= pack_len)
                    .ok_or_else(|| io::Error::other("source record outside pack"))?;
                Ok((start, end))
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Index {
    format: u32,
    records: BTreeMap<String, Reference>,
    /// Inflated size of every pack the writer produced or carried, so reuse
    /// decisions never open a pack.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    packs: BTreeMap<String, u64>,
}

#[cfg(test)]
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Index(Index),
    Legacy(Files),
}

#[cfg(test)]
fn read_index(dir: &Path) -> io::Result<Stored> {
    let bytes = std::fs::read(dir.join(crate::sidecar::SOURCE_FILE))?;
    decode_index(&bytes)
}

#[cfg(test)]
fn decode_index(bytes: &[u8]) -> io::Result<Stored> {
    let stored: Stored = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    if let Stored::Index(index) = &stored {
        index.validate()?;
    }
    Ok(stored)
}

impl Index {
    fn validate(&self) -> io::Result<()> {
        if !(1..=NATIVE_FORMAT).contains(&self.format)
            || self.records.values().any(|reference| {
                !valid_hash(reference.pack())
                    || !valid_hash(reference.hash())
                    || match reference {
                        Reference::Single(_) => self.format != 1,
                        Reference::Packed(record) => {
                            self.format < 2
                                || record.len == 0
                                || record.offset.checked_add(record.len).is_none()
                        }
                    }
            })
        {
            return Err(io::Error::other("invalid record index"));
        }
        Ok(())
    }

    pub(crate) fn decode_packed(bytes: &[u8]) -> io::Result<Self> {
        let index: Self = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        index.validate()?;
        if index.format < 2 {
            return Err(io::Error::other("packed records required"));
        }
        Ok(index)
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.records.keys().map(String::as_str)
    }

    pub(crate) fn matches<T: EncodeRecord>(&self, path: &str, record: &T) -> io::Result<bool> {
        let Some(reference) = self.records.get(path) else {
            return Ok(false);
        };
        if self.format < NATIVE_FORMAT {
            return Ok(false);
        }
        let mut bytes = Vec::new();
        record.encode_record(&mut bytes)?;
        Ok(graph_search_core::hash::content_hash(&bytes) == reference.hash())
    }

    /// Verify committed bytes without eagerly deserializing cold record values.
    #[cfg(test)]
    pub(crate) fn verify(&self, dir: &Path, layout: Layout) -> io::Result<()> {
        for (hash, group) in groups(self) {
            let pack = read_pack(&dir.join(layout.directory), hash, self.format)?;
            validate_ranges(pack.bytes(), &group)?;
        }
        Ok(())
    }

    pub(crate) fn load<T: DecodeRecord + DeserializeOwned>(
        &self,
        dir: &Path,
        layout: Layout,
    ) -> io::Result<BTreeMap<String, T>> {
        Self::load_groups(dir, layout, self.format, true, groups(self))
    }

    /// The index must be verified against its committed hash. Only requested
    /// records are decoded and each record hash is verified; packs without
    /// selected records are not opened. Uncompressed (format 1-2) packs are read
    /// by byte range; a compressed pack is inflated whole (at most one ~8 MiB
    /// frame) because a zstd frame has no random access.
    pub(crate) fn load_selected_verified<T: DecodeRecord + DeserializeOwned>(
        &self,
        dir: &Path,
        layout: Layout,
        paths: &BTreeSet<String>,
    ) -> io::Result<BTreeMap<String, T>> {
        let mut selected: BTreeMap<&str, Group<'_>> = BTreeMap::new();
        for path in paths {
            if let Some((path, reference)) = self.records.get_key_value(path) {
                selected
                    .entry(reference.pack())
                    .or_default()
                    .push((path, reference));
            }
        }
        let mut files = BTreeMap::new();
        for (hash, group) in selected {
            let path = dir.join(layout.directory).join(hash);
            if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
                return Err(io::Error::other("source pack is not a regular file"));
            }
            if self.format >= NATIVE_FORMAT {
                let bytes = crate::compress::inflate_pack(&std::fs::read(path)?)?;
                let len = bytes.len();
                let mut cursor = io::Cursor::new(bytes);
                files.extend(read_selected_group(&mut cursor, len, self.format, group)?);
            } else {
                let mut file = std::fs::File::open(path)?;
                let len = usize::try_from(file.metadata()?.len()).map_err(io::Error::other)?;
                files.extend(read_selected_group(&mut file, len, self.format, group)?);
            }
        }
        Ok(files)
    }

    fn load_groups<T: DecodeRecord + DeserializeOwned>(
        dir: &Path,
        layout: Layout,
        format: u32,
        verify_records: bool,
        grouped: BTreeMap<&str, Group<'_>>,
    ) -> io::Result<BTreeMap<String, T>> {
        let mut files = BTreeMap::new();
        for (hash, group) in grouped {
            let pack = read_pack(&dir.join(layout.directory), hash, format)?;
            let bytes = pack.bytes();
            if verify_records {
                validate_ranges(bytes, &group)?;
            } else {
                ranges(bytes.len(), &group)?;
            }
            for (path, reference) in group {
                let (start, end) = reference.range(bytes.len())?;
                let record = decode(format, bytes.get(start..end).unwrap_or_default())?;
                files.insert(path.clone(), record);
            }
        }
        Ok(files)
    }
}

/// The verified descriptor commits each record hash independently of the pack
/// hash. Unselected bytes need not be read to authenticate a returned record.
fn read_selected_group<T: DecodeRecord + DeserializeOwned>(
    reader: &mut (impl io::Read + io::Seek),
    pack_len: usize,
    format: u32,
    group: Group<'_>,
) -> io::Result<BTreeMap<String, T>> {
    ranges(pack_len, &group)?;
    let mut slices: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for (path, reference) in group {
        let (start, end) = reference.range(pack_len)?;
        slices
            .entry((start, end, reference.hash()))
            .or_default()
            .push(path);
    }
    let mut files = BTreeMap::new();
    for ((start, end, hash), paths) in slices {
        reader.seek(io::SeekFrom::Start(
            u64::try_from(start).map_err(io::Error::other)?,
        ))?;
        let mut bytes = vec![0; end.saturating_sub(start)];
        reader.read_exact(&mut bytes)?;
        if graph_search_core::hash::content_hash(&bytes) != hash {
            return Err(io::Error::other("source record checksum mismatch"));
        }
        for path in paths {
            files.insert(path.clone(), decode(format, &bytes)?);
        }
    }
    Ok(files)
}

/// Index format whose records use each fact type's native [`EncodeRecord`]
/// encoding. Formats 1 and 2 hold JSON records.
pub(crate) const NATIVE_FORMAT: u32 = 3;

fn decode<T: DecodeRecord + DeserializeOwned>(format: u32, bytes: &[u8]) -> io::Result<T> {
    if format >= NATIVE_FORMAT {
        T::decode_record(bytes)
    } else {
        serde_json::from_slice(bytes).map_err(io::Error::other)
    }
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Committed pack file bytes plus, for native packs, the inflated records.
/// Pack names hash the file bytes, so corruption is caught before inflation.
struct Pack {
    file: Vec<u8>,
    inflated: Option<Vec<u8>>,
}

impl Pack {
    /// Concatenated record bytes; record offsets index into this.
    fn bytes(&self) -> &[u8] {
        self.inflated.as_deref().unwrap_or(&self.file)
    }
}

/// Inflated size of a native pack from its frame header alone, so size-based
/// compaction policy keeps meaning record bytes rather than compressed bytes.
fn raw_len(path: &Path) -> io::Result<u64> {
    use io::Read as _;
    let mut header = Vec::with_capacity(18);
    std::fs::File::open(path)?
        .take(18)
        .read_to_end(&mut header)?;
    crate::compress::frame_size(&header)
}

fn read_pack(dir: &Path, hash: &str, format: u32) -> io::Result<Pack> {
    let path = dir.join(hash);
    if !std::fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(io::Error::other("source pack is not a regular file"));
    }
    let file = std::fs::read(path)?;
    if graph_search_core::hash::content_hash(&file) != hash {
        return Err(io::Error::other("source pack checksum mismatch"));
    }
    let inflated = if format >= NATIVE_FORMAT {
        Some(crate::compress::inflate_pack(&file)?)
    } else {
        None
    };
    Ok(Pack { file, inflated })
}

type Group<'a> = Vec<(&'a String, &'a Reference)>;
fn groups(index: &Index) -> BTreeMap<&str, Group<'_>> {
    let mut groups: BTreeMap<&str, Group<'_>> = BTreeMap::new();
    for (path, reference) in &index.records {
        groups
            .entry(reference.pack())
            .or_default()
            .push((path, reference));
    }
    groups
}

type Ranges<'a> = BTreeSet<(usize, usize, &'a str)>;

/// Duplicate references may share exactly one range; different records must not overlap.
fn ranges<'a>(pack_len: usize, group: &Group<'a>) -> io::Result<Ranges<'a>> {
    let mut ranges = BTreeSet::new();
    for (_, reference) in group {
        let (start, end) = reference.range(pack_len)?;
        ranges.insert((start, end, reference.hash()));
    }
    let mut previous_end = 0;
    for &(start, end, _) in &ranges {
        if start < previous_end {
            return Err(io::Error::other("overlapping source records"));
        }
        previous_end = end;
    }
    Ok(ranges)
}

fn validate_ranges(bytes: &[u8], group: &Group<'_>) -> io::Result<usize> {
    let mut live = 0usize;
    for (start, end, hash) in ranges(bytes.len(), group)? {
        if graph_search_core::hash::content_hash(&bytes[start..end]) != hash {
            return Err(io::Error::other("source record checksum mismatch"));
        }
        live = live.saturating_add(end.saturating_sub(start));
    }
    Ok(live)
}

fn live_bytes(pack_len: usize, group: &Group<'_>) -> io::Result<usize> {
    // Reuse receives the index validated on open or constructed by this writer.
    // Verifying the complete pack against that cached identity preserves all of
    // its record hashes; hashing each record again would repeat the same work.
    Ok(ranges(pack_len, group)?
        .iter()
        .fold(0usize, |total, &(start, end, _)| {
            total.saturating_add(end.saturating_sub(start))
        }))
}

#[cfg(test)]
pub(crate) fn load(dir: &Path) -> io::Result<Files> {
    load_cached(dir).map(|(files, _)| files)
}

#[cfg(test)]
/// Owns the exact source descriptor bytes authenticated by CURRENT, after parsing.
/// Pack bytes are validated once, when this descriptor is consumed by the loader.
pub(crate) struct Prepared(Stored);

#[cfg(test)]
pub(crate) fn prepare(bytes: &[u8], generation_format: u32) -> io::Result<Prepared> {
    let stored = decode_index(bytes)?;
    if let Stored::Index(index) = &stored {
        let minimum = match index.format {
            1 => 4,
            2 => 5,
            _ => 8,
        };
        if generation_format < minimum {
            return Err(io::Error::other(
                "source records require a newer generation format",
            ));
        }
    }
    Ok(Prepared(stored))
}

#[cfg(test)]
pub(crate) fn load_cached(dir: &Path) -> io::Result<(Files, Option<Index>)> {
    match read_index(dir) {
        Ok(stored) => load_prepared(dir, Prepared(stored)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && !dir.join(crate::sidecar::SOURCE_FILE).exists() =>
        {
            Ok((Files::new(), None))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub(crate) fn load_prepared(dir: &Path, prepared: Prepared) -> io::Result<(Files, Option<Index>)> {
    match prepared.0 {
        Stored::Legacy(files) => Ok((files, None)),
        Stored::Index(index) => {
            let files = index.load(dir, SOURCE_LAYOUT)?;
            Ok((files, Some(index)))
        }
    }
}

#[cfg(test)]
fn verify(dir: &Path, generation_format: u32) -> io::Result<()> {
    let bytes = std::fs::read(dir.join(crate::sidecar::SOURCE_FILE))?;
    load_prepared(dir, prepare(&bytes, generation_format)?).map(|_| ())
}

struct Pending {
    offset: u64,
    len: u64,
    paths: Vec<String>,
}
struct Writer<'a> {
    directory: &'a Path,
    /// Inflated sizes of the packs this index references.
    sizes: BTreeMap<String, u64>,
    records: BTreeMap<String, Reference>,
    known: BTreeMap<String, Reference>,
    written: BTreeSet<String>,
    bytes: Vec<u8>,
    pending: BTreeMap<String, Pending>,
    limit: usize,
}
impl<'a> Writer<'a> {
    fn new(directory: &'a Path, limit: usize) -> Self {
        Self {
            directory,
            sizes: BTreeMap::new(),
            records: BTreeMap::new(),
            known: BTreeMap::new(),
            written: BTreeSet::new(),
            bytes: Vec::new(),
            pending: BTreeMap::new(),
            limit,
        }
    }
    fn retain(&mut self, path: &str, reference: &Reference) {
        self.known
            .insert(reference.hash().to_owned(), reference.clone());
        self.records.insert(path.to_owned(), reference.clone());
    }
    fn add<T: EncodeRecord>(&mut self, path: &str, record: &T) -> io::Result<()> {
        // Encode directly into the pack buffer. Only a boundary-crossing
        // record needs a separate allocation while the preceding pack is flushed.
        let start = self.bytes.len();
        record.encode_record(&mut self.bytes)?;
        self.finish_record(path, start)
    }

    fn add_encoded(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let start = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        self.finish_record(path, start)
    }

    fn finish_record(&mut self, path: &str, start: usize) -> io::Result<()> {
        let hash = graph_search_core::hash::content_hash(&self.bytes[start..]);
        if let Some(reference) = self.known.get(&hash).cloned() {
            self.bytes.truncate(start);
            self.records.insert(path.to_owned(), reference);
            return Ok(());
        }
        if let Some(pending) = self.pending.get_mut(&hash) {
            self.bytes.truncate(start);
            pending.paths.push(path.to_owned());
            return Ok(());
        }
        let offset = if start > 0 && self.bytes.len() > self.limit {
            let record_bytes = self.bytes.split_off(start);
            self.flush()?;
            self.bytes = record_bytes;
            0
        } else {
            start
        };
        self.pending.insert(
            hash,
            Pending {
                offset: offset as u64,
                len: self.bytes.len().saturating_sub(offset) as u64,
                paths: vec![path.to_owned()],
            },
        );
        if self.bytes.len() >= self.limit {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let file = crate::compress::deflate(&self.bytes)?;
        let pack = graph_search_core::hash::content_hash(&file);
        self.sizes.insert(pack.clone(), self.bytes.len() as u64);
        // Content-named: a pack already present in a shared directory is the
        // same pack, committed by an earlier generation.
        let target = self.directory.join(&pack);
        if self.written.insert(pack.clone()) && !target.exists() {
            crate::generation::replace(&target, &file)?;
        }
        for (hash, pending) in std::mem::take(&mut self.pending) {
            let reference = Reference::Packed(Packed {
                pack: pack.clone(),
                hash: hash.clone(),
                offset: pending.offset,
                len: pending.len,
            });
            self.known.insert(hash, reference.clone());
            for path in pending.paths {
                self.records.insert(path, reference.clone());
            }
        }
        self.bytes.clear();
        Ok(())
    }
}

#[allow(clippy::integer_division)] // Round the allowed dead quarter down, retaining at least 75% live.
fn minimum_live(bytes: usize) -> usize {
    bytes.saturating_sub(bytes / 4)
}

/// Share a pack by hard link. Where links are unavailable, copy it, verifying
/// the copied bytes against the pack's content hash first.
fn link_or_copy(source: &Path, target: &Path, hash: &str) -> io::Result<()> {
    if std::fs::hard_link(source, target).is_err() {
        let bytes = std::fs::read(source)?;
        if graph_search_core::hash::content_hash(&bytes) != hash {
            return Err(io::Error::other("source pack checksum mismatch"));
        }
        crate::generation::replace(target, &bytes)?;
    }
    Ok(())
}

#[cfg(test)]
/// New generations share verified packs, compact those below 75% live bytes,
/// and combine multiple sub-MiB packs. New records are packed with bounded buffering.
/// `touched`, when supplied, must include every path whose facts may differ from
/// `old`; the store derives it from the only source-mutating batch operations.
/// Without that proof, compare every candidate record conservatively.
pub(crate) fn save_cached(
    dir: &Path,
    files: &Files,
    previous: &Path,
    old: &Files,
    old_index: Option<&Index>,
    touched: Option<&BTreeSet<&str>>,
) -> io::Result<Index> {
    save_records(
        dir,
        SOURCE_LAYOUT,
        files,
        previous,
        old_index,
        |path, record| {
            Ok(old.get(path).is_some_and(|old_record| {
                touched.is_some_and(|paths| !paths.contains(path)) || old_record == record
            }))
        },
    )
}

#[cfg(test)]
/// Shared immutable pack writer. Reuse eligibility belongs to each fact type.
pub(crate) fn save_records<T: EncodeRecord>(
    dir: &Path,
    layout: Layout,
    files: &BTreeMap<String, T>,
    previous: &Path,
    old_index: Option<&Index>,
    unchanged: impl Fn(&str, &T) -> io::Result<bool>,
) -> io::Result<Index> {
    save_records_retaining(
        dir,
        layout,
        files,
        previous,
        old_index,
        &BTreeSet::new(),
        unchanged,
    )
}

#[cfg(test)]
/// `retained` is an explicit identity-validated set with no in-memory payloads.
/// Repacking copies authenticated record bytes and does not deserialize them.
/// Writes the packs and the index into `dir`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_records_retaining<T: EncodeRecord>(
    dir: &Path,
    layout: Layout,
    files: &BTreeMap<String, T>,
    previous: &Path,
    old_index: Option<&Index>,
    retained: &BTreeSet<String>,
    unchanged: impl Fn(&str, &T) -> io::Result<bool>,
) -> io::Result<Index> {
    let index = save_packs(dir, layout, files, previous, old_index, retained, unchanged)?;
    write_index(dir, layout, &index)?;
    Ok(index)
}

/// Writes `index` as the layout's index artifact in `dir`.
pub(crate) fn write_index(dir: &Path, layout: Layout, index: &Index) -> io::Result<()> {
    let bytes = serde_json::to_vec(index).map_err(io::Error::other)?;
    crate::generation::replace(&dir.join(layout.index), &bytes)
}

/// Writes the packs of the next index into `dir` and returns the index. When
/// `previous` is `dir` (a directory every generation shares), a healthy pack is
/// carried forward by reference alone: nothing is read, linked or copied.
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_packs<T: EncodeRecord>(
    dir: &Path,
    layout: Layout,
    files: &BTreeMap<String, T>,
    previous: &Path,
    old_index: Option<&Index>,
    retained: &BTreeSet<String>,
    unchanged: impl Fn(&str, &T) -> io::Result<bool>,
) -> io::Result<Index> {
    if retained.iter().any(|path| {
        files.contains_key(path) || old_index.is_none_or(|index| !index.records.contains_key(path))
    }) {
        return Err(io::Error::other("invalid retained record set"));
    }
    let shared = dir == previous;
    let directory = dir.join(layout.directory);
    if shared {
        std::fs::create_dir_all(&directory)?;
    } else {
        std::fs::create_dir(&directory)?;
    }
    let mut writer = Writer::new(&directory, layout.pack_bytes);
    let mut reusable: BTreeMap<&str, Group<'_>> = BTreeMap::new();
    // Older JSON-record generations are rebuilt, never mixed into a native index.
    let old_index = old_index.filter(|index| index.format >= NATIVE_FORMAT);
    if !retained.is_empty() && old_index.is_none() {
        return Err(io::Error::other("retained records require a native index"));
    }
    if let Some(index) = old_index {
        for (path, reference) in &index.records {
            if retained.contains(path)
                || files
                    .get(path)
                    .map(|record| unchanged(path, record))
                    .transpose()?
                    .unwrap_or(false)
            {
                reusable
                    .entry(reference.pack())
                    .or_default()
                    .push((path, reference));
            }
        }
    }
    // Pack sizes come from the index; only an index that predates recording
    // them reads a frame header.
    let size = |hash: &str| -> io::Result<u64> {
        match old_index.and_then(|index| index.packs.get(hash)) {
            Some(size) => Ok(*size),
            None => raw_len(&previous.join(layout.directory).join(hash)),
        }
    };
    let mut small = 0usize;
    for hash in reusable.keys() {
        if size(hash)? < layout.small_pack_bytes() {
            small = small.saturating_add(1);
        }
    }
    let mut copied = BTreeSet::new();
    for (hash, group) in reusable {
        let source = previous.join(layout.directory).join(hash);
        let packed = group
            .iter()
            .all(|(_, reference)| matches!(reference, Reference::Packed(_)));
        // A healthy pack is carried over from its size and the index alone: its
        // name is its content hash, so every reader still verifies it before
        // use, and publishing never reads bytes it does not rewrite.
        let len = if packed {
            usize::try_from(size(hash)?).map_err(io::Error::other)?
        } else {
            0
        };
        let compact_small = small > 1 && (len as u64) < layout.small_pack_bytes();
        if !packed || compact_small || live_bytes(len, &group)? < minimum_live(len) {
            let pack = read_pack(&previous.join(layout.directory), hash, NATIVE_FORMAT)?;
            let bytes = pack.bytes();
            for (path, reference) in group {
                let (start, end) = reference.range(bytes.len())?;
                writer.add_encoded(path, bytes.get(start..end).unwrap_or_default())?;
                copied.insert(path.clone());
            }
            continue;
        }
        if !shared {
            link_or_copy(&source, &directory.join(hash), hash)?;
        }
        writer.written.insert(hash.to_owned());
        writer.sizes.insert(hash.to_owned(), len as u64);
        for (path, reference) in group {
            writer.retain(path, reference);
        }
    }
    for (path, record) in files {
        if !writer.records.contains_key(path) && !copied.contains(path) {
            writer.add(path, record)?;
        }
    }
    writer.flush()?;
    crate::generation::sync_dir(&directory)?;
    let referenced: BTreeSet<&str> = writer.records.values().map(Reference::pack).collect();
    let packs = writer
        .sizes
        .into_iter()
        .filter(|(hash, _)| referenced.contains(hash.as_str()))
        .collect();
    Ok(Index {
        format: NATIVE_FORMAT,
        records: writer.records,
        packs,
    })
}

impl Index {
    /// Every pack this index references, by file name.
    pub(crate) fn pack_names(&self) -> BTreeSet<&str> {
        self.records.values().map(Reference::pack).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(hash: &str) -> SourceFileUnits {
        SourceFileUnits {
            source_hash: hash.into(),
            version: 1,
            ..SourceFileUnits::default()
        }
    }
    fn index(dir: &Path) -> Index {
        let Stored::Index(index) = read_index(dir).unwrap() else {
            panic!("index expected")
        };
        index
    }
    fn write_index(dir: &Path, index: &Index) {
        std::fs::write(
            dir.join(crate::sidecar::SOURCE_FILE),
            serde_json::to_vec(index).unwrap(),
        )
        .unwrap();
    }
    fn save(dir: &Path, files: &Files, previous: &Path, old: &Files) -> Index {
        let previous_index = match read_index(previous) {
            Ok(Stored::Index(index)) => Some(index),
            Ok(Stored::Legacy(_)) => None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => panic!("{error}"),
        };
        save_cached(dir, files, previous, old, previous_index.as_ref(), None).unwrap()
    }

    #[test]
    fn selected_ranges_read_only_required_bytes_and_share_duplicate_slices() {
        struct Counted {
            cursor: io::Cursor<Vec<u8>>,
            read: usize,
        }
        impl io::Read for Counted {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let count = io::Read::read(&mut self.cursor, buffer)?;
                self.read += count;
                Ok(count)
            }
        }
        impl io::Seek for Counted {
            fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
                io::Seek::seek(&mut self.cursor, position)
            }
        }
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(DIRECTORY);
        std::fs::create_dir(&directory).unwrap();
        let mut writer = Writer::new(&directory, PACK_BYTES);
        writer.add("a", &"selected").unwrap();
        writer.add("a-copy", &"selected").unwrap();
        writer.add("b", &"x".repeat(4096)).unwrap();
        writer.flush().unwrap();
        let index = Index {
            format: NATIVE_FORMAT,
            records: writer.records,
            packs: BTreeMap::new(),
        };
        index.verify(root.path(), SOURCE_LAYOUT).unwrap();
        assert_eq!(index.records["a"].pack(), index.records["b"].pack());
        let bytes = read_pack(&directory, index.records["a"].pack(), NATIVE_FORMAT)
            .unwrap()
            .bytes()
            .to_vec();
        let pack_len = bytes.len();
        assert_eq!(pack_len, 4108);
        let mut reader = Counted {
            cursor: io::Cursor::new(bytes),
            read: 0,
        };
        let group = || {
            vec![
                (index.records.get_key_value("a").unwrap()),
                (index.records.get_key_value("a-copy").unwrap()),
            ]
        };
        let facts: BTreeMap<String, String> =
            read_selected_group(&mut reader, pack_len, NATIVE_FORMAT, group()).unwrap();
        assert_eq!(
            facts,
            BTreeMap::from([
                ("a".into(), "selected".into()),
                ("a-copy".into(), "selected".into())
            ])
        );
        assert_eq!(
            reader.read, 10,
            "read one unique slice, not the 4108-byte pack"
        );
        let (cold, _) = index.records["b"].range(pack_len).unwrap();
        reader.cursor.get_mut()[cold] = b'!';
        reader.read = 0;
        assert!(
            read_selected_group::<String>(&mut reader, pack_len, NATIVE_FORMAT, group()).is_ok()
        );
        assert_eq!(reader.read, 10);
        reader.cursor.get_mut()[0] = b'!';
        assert!(
            read_selected_group::<String>(&mut reader, pack_len, NATIVE_FORMAT, group()).is_err()
        );
    }

    #[test]
    fn selected_reads_skip_other_packs_but_verify_every_selected_record() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(DIRECTORY);
        std::fs::create_dir(&directory).unwrap();
        let mut writer = Writer::new(&directory, PACK_BYTES);
        writer.limit = 1;
        writer.add("a", &record("a")).unwrap();
        writer.add("a-copy", &record("a")).unwrap();
        writer.add("b", &record("b")).unwrap();
        writer.flush().unwrap();
        let index = Index {
            format: NATIVE_FORMAT,
            records: writer.records,
            packs: BTreeMap::new(),
        };
        index.verify(root.path(), SOURCE_LAYOUT).unwrap();
        assert_eq!(index.records["a"], index.records["a-copy"]);
        assert_ne!(index.records["a"].pack(), index.records["b"].pack());
        std::fs::remove_file(directory.join(index.records["b"].pack())).unwrap();
        let paths = BTreeSet::from(["a".into(), "a-copy".into(), "absent".into()]);
        let selected: Files = index
            .load_selected_verified(root.path(), SOURCE_LAYOUT, &paths)
            .unwrap();
        assert_eq!(
            selected,
            Files::from([("a".into(), record("a")), ("a-copy".into(), record("a"))])
        );
        assert!(
            index
                .load::<SourceFileUnits>(root.path(), SOURCE_LAYOUT)
                .is_err()
        );
        assert!(
            index
                .load_selected_verified::<SourceFileUnits>(
                    root.path(),
                    SOURCE_LAYOUT,
                    &BTreeSet::from(["b".into()])
                )
                .is_err()
        );
        std::fs::write(directory.join(index.records["a"].pack()), b"corrupt").unwrap();
        assert!(
            index
                .load_selected_verified::<SourceFileUnits>(root.path(), SOURCE_LAYOUT, &paths)
                .is_err()
        );
        assert!(
            index
                .load_selected_verified::<SourceFileUnits>(
                    root.path(),
                    SOURCE_LAYOUT,
                    &BTreeSet::new()
                )
                .unwrap()
                .is_empty()
        );
        assert!(
            index
                .load_selected_verified::<SourceFileUnits>(
                    root.path(),
                    SOURCE_LAYOUT,
                    &BTreeSet::from(["unknown".into()])
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn changed_records_are_isolated_and_shared_packs_survive_reclamation() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let old: Files = (0..10)
            .map(|i| (format!("{i}.rs"), record(&i.to_string())))
            .collect();
        let before = save(&first, &old, root.path(), &Files::new());
        let mut new = old.clone();
        new.remove("9.rs");
        new.insert("8.rs".into(), record("changed"));
        new.insert("duplicate.rs".into(), record("0"));
        let after = save(&second, &new, &first, &old);
        assert_eq!(before.records["0.rs"], after.records["0.rs"]);
        assert_ne!(before.records["8.rs"], after.records["8.rs"]);
        assert_eq!(after.records["0.rs"], after.records["duplicate.rs"]);
        assert_eq!(groups(&after).len(), 2);
        assert_eq!(
            std::fs::read_dir(second.join(DIRECTORY)).unwrap().count(),
            2
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(first.join(DIRECTORY).join(before.records["0.rs"].pack()))
                    .unwrap()
                    .ino(),
                std::fs::metadata(second.join(DIRECTORY).join(after.records["0.rs"].pack()))
                    .unwrap()
                    .ino()
            );
        }
        assert_eq!(load(&first).unwrap(), old);
        std::fs::remove_dir_all(&first).unwrap();
        assert_eq!(load(&second).unwrap(), new);
        verify(&second, 8).unwrap();
        assert!(verify(&second, 7).is_err());
    }

    #[test]
    fn corrupt_missing_and_escaping_packs_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        let files = Files::from([("a.rs".into(), record("a"))]);
        let mut index = save(root.path(), &files, empty.path(), &Files::new());
        let blob = root
            .path()
            .join(DIRECTORY)
            .join(index.records["a.rs"].pack());
        std::fs::write(&blob, b"{}").unwrap();
        assert!(load(root.path()).is_err());
        assert!(verify(root.path(), 5).is_err());
        // A pack carried forward is not read when publishing: the corrupt pack
        // reaches the new generation and its first reader fails.
        let destination = tempfile::tempdir().unwrap();
        save_cached(
            destination.path(),
            &files,
            root.path(),
            &files,
            Some(&index),
            None,
        )
        .unwrap();
        assert!(load(destination.path()).is_err());
        std::fs::remove_file(blob).unwrap();
        assert!(load(root.path()).is_err());
        let Reference::Packed(reference) = index.records.get_mut("a.rs").unwrap() else {
            panic!()
        };
        reference.pack = "../escape".into();
        write_index(root.path(), &index);
        assert!(load(root.path()).is_err());
    }

    #[test]
    fn ranges_hashes_overlap_and_version_mismatches_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let files = Files::from([("a.rs".into(), record("a")), ("b.rs".into(), record("b"))]);
        let original = save(root.path(), &files, root.path(), &Files::new());
        let encoded = serde_json::to_vec(&original).unwrap();
        for corruption in 0..5 {
            let mut index: Index = serde_json::from_slice(&encoded).unwrap();
            let Reference::Packed(reference) = index.records.get_mut("a.rs").unwrap() else {
                panic!()
            };
            match corruption {
                0 => reference.offset = u64::MAX,
                1 => reference.len = 0,
                2 => reference.hash = "0".repeat(64),
                3 => reference.len = reference.len.saturating_add(1),
                _ => index.format = 1,
            }
            write_index(root.path(), &index);
            assert!(load(root.path()).is_err(), "corruption {corruption}");
        }
        // Individually correct overlapping substrings still violate the pack layout.
        let a = Reference::Packed(Packed {
            pack: "0".repeat(64),
            hash: graph_search_core::hash::content_hash(b"abc"),
            offset: 0,
            len: 3,
        });
        let b = Reference::Packed(Packed {
            pack: "0".repeat(64),
            hash: graph_search_core::hash::content_hash(b"bc"),
            offset: 1,
            len: 2,
        });
        let paths = ["a".to_owned(), "b".to_owned()];
        assert!(validate_ranges(b"abc", &vec![(&paths[0], &a), (&paths[1], &b)]).is_err());
    }

    #[test]
    fn reuse_uses_the_opened_index_not_later_disk_index_edits() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let files = Files::from([("a.rs".into(), record("a")), ("b.rs".into(), record("b"))]);
        let cached = save_cached(
            first.path(),
            &files,
            first.path(),
            &Files::new(),
            None,
            None,
        )
        .unwrap();
        let mut tampered = index(first.path());
        tampered
            .records
            .insert("a.rs".into(), tampered.records["b.rs"].clone());
        write_index(first.path(), &tampered);
        save_cached(
            second.path(),
            &files,
            first.path(),
            &files,
            Some(&cached),
            None,
        )
        .unwrap();
        assert_eq!(load(second.path()).unwrap(), files);
    }

    #[test]
    fn prepared_descriptor_is_pinned_but_pack_bytes_still_require_verification() {
        let root = tempfile::tempdir().unwrap();
        let files = Files::from([("a.rs".into(), record("a"))]);
        let index =
            save_cached(root.path(), &files, root.path(), &Files::new(), None, None).unwrap();
        let descriptor = std::fs::read(root.path().join(crate::sidecar::SOURCE_FILE)).unwrap();
        let prepared = prepare(&descriptor, 8).unwrap();
        std::fs::write(
            root.path().join(crate::sidecar::SOURCE_FILE),
            b"invalid replacement",
        )
        .unwrap();
        assert_eq!(load_prepared(root.path(), prepared).unwrap().0, files);
        let prepared = prepare(&descriptor, 8).unwrap();
        std::fs::write(
            root.path()
                .join(DIRECTORY)
                .join(index.records["a.rs"].pack()),
            b"corrupt",
        )
        .unwrap();
        assert!(load_prepared(root.path(), prepared).is_err());
        let legacy = prepare(&serde_json::to_vec(&files).unwrap(), 3).unwrap();
        assert_eq!(load_prepared(root.path(), legacy).unwrap().0, files);
    }

    #[test]
    fn touched_records_compare_all_fields_even_when_source_hash_is_unchanged() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let old: Files = (0..10)
            .map(|i| (format!("{i}.rs"), record(&i.to_string())))
            .collect();
        let cached =
            save_cached(first.path(), &old, first.path(), &Files::new(), None, None).unwrap();
        let mut files = old.clone();
        files.get_mut("3.rs").unwrap().version = 2;
        files.remove("9.rs");
        let touched = BTreeSet::from(["3.rs", "9.rs"]);
        let updated = save_cached(
            second.path(),
            &files,
            first.path(),
            &old,
            Some(&cached),
            Some(&touched),
        )
        .unwrap();
        assert_eq!(cached.records["0.rs"], updated.records["0.rs"]);
        assert_ne!(cached.records["3.rs"], updated.records["3.rs"]);
        assert!(!updated.records.contains_key("9.rs"));
        assert_eq!(load(second.path()).unwrap(), files);
    }

    #[test]
    fn pack_rollover_dedup_and_oversized_records_preserve_exact_bytes() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(DIRECTORY);
        std::fs::create_dir(&directory).unwrap();
        let mut writer = Writer::new(&directory, PACK_BYTES);
        writer.limit = 150;
        let files = Files::from([
            ("a".into(), record("a")),
            ("a-copy".into(), record("a")),
            ("b".into(), record("b")),
            ("c".into(), record(&"x".repeat(200))),
            ("duplicate".into(), record("a")),
        ]);
        for (path, record) in &files {
            writer.add(path, record).unwrap();
        }
        writer.flush().unwrap();
        assert!(writer.bytes.is_empty());
        let index = Index {
            format: NATIVE_FORMAT,
            records: writer.records,
            packs: BTreeMap::new(),
        };
        assert_eq!(groups(&index).len(), 2);
        assert_eq!(index.records["a"], index.records["duplicate"]);
        assert_eq!(index.records["a"], index.records["a-copy"]);
        for (hash, group) in groups(&index) {
            let pack = read_pack(&root.path().join(DIRECTORY), hash, NATIVE_FORMAT).unwrap();
            let bytes = pack.bytes();
            assert_eq!(validate_ranges(bytes, &group).unwrap(), bytes.len());
        }
        write_index(root.path(), &index);
        assert_eq!(load(root.path()).unwrap(), files);
    }

    #[test]
    fn unsupported_links_fall_back_to_a_verified_synced_copy() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("pack");
        let bytes = b"pack bytes named by their hash";
        std::fs::write(&source, bytes).unwrap();
        let hash = graph_search_core::hash::content_hash(bytes);
        // An existing target makes the hard link fail, as on a filesystem
        // without links.
        let target = root.path().join("copy");
        std::fs::write(&target, b"stale").unwrap();
        link_or_copy(&source, &target, &hash).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), bytes);
        assert!(!root.path().join("copy.tmp").exists());
        // A pack whose bytes no longer match its name is never copied.
        std::fs::write(&source, b"corrupted").unwrap();
        std::fs::write(&target, b"stale").unwrap();
        assert!(link_or_copy(&source, &target, &hash).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"stale");
    }

    #[test]
    fn churn_bounds_dead_bytes_and_small_pack_count() {
        let root = tempfile::tempdir().unwrap();
        let mut files: Files = (0..32)
            .map(|i| (format!("{i:02}"), record(&i.to_string())))
            .collect();
        let mut previous = root.path().join("initial");
        std::fs::create_dir(&previous).unwrap();
        let mut index = save(&previous, &files, root.path(), &Files::new());
        for edit in 0..40 {
            let old = files.clone();
            files.insert(format!("{:02}", edit % 32), record(&format!("edit-{edit}")));
            if edit == 20 {
                files.retain(|path, _| path.as_str() < "08");
            }
            let next = root.path().join(format!("generation-{edit}"));
            std::fs::create_dir(&next).unwrap();
            index = save_cached(&next, &files, &previous, &old, Some(&index), None).unwrap();
            let groups = groups(&index);
            assert!(groups.len() <= 2);
            for (hash, group) in groups {
                let pack = read_pack(&next.join(DIRECTORY), hash, NATIVE_FORMAT).unwrap();
                let bytes = pack.bytes();
                let live = validate_ranges(bytes, &group).unwrap();
                assert!(live.saturating_mul(4) >= bytes.len().saturating_mul(3));
            }
            std::fs::remove_dir_all(&previous).unwrap();
            assert_eq!(load(&next).unwrap(), files);
            previous = next;
        }
    }
}
