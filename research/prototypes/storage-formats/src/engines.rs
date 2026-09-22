//! Storage engines holding the same encoded record bytes, keyed by path.
//!
//! * `packs`: the current design, simplified — immutable ~8 MiB files plus a
//!   JSON offset index, every file written tmp + fsync + rename. A one-record
//!   update allocates a new generation directory, hard-links the unchanged packs,
//!   writes one new pack and a complete new index (as `save_records_retaining`).
//! * `sqlite`: one table, WAL, `synchronous=FULL`, `fullfsync=ON` for durability
//!   parity with `File::sync_all` on macOS.
//! * `redb`: pure-Rust copy-on-write B-tree, default (immediate) durability.
//!
//! Only storage is measured here: record *decode* cost is measured separately.

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Serialize)]
pub struct EngineRow {
    pub engine: String,
    pub payload: String,
    pub disk_bytes: u64,
    pub bulk_write_ms: f64,
    pub update_one_ms: f64,
    pub read_all_ms: f64,
    pub read_50_ms: f64,
}

type Res<T> = Result<T, Box<dyn std::error::Error>>;

trait Engine {
    fn name(&self) -> &'static str;
    fn bulk_write(&mut self, dir: &Path, paths: &[String], records: &[Vec<u8>]) -> Res<()>;
    fn update_one(&mut self, dir: &Path, path: &str, record: &[u8]) -> Res<()>;
    fn read_all(&mut self, dir: &Path) -> Res<Vec<(String, Vec<u8>)>>;
    fn read_selected(&mut self, dir: &Path, paths: &[&str]) -> Res<Vec<Vec<u8>>>;
}

fn disk_usage(dir: &Path) -> u64 {
    // Unique inodes only: hard-linked packs are shared, not duplicated.
    use std::os::unix::fs::MetadataExt;
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            } else if seen.insert(meta.ino()) {
                total += meta.len();
            }
        }
    }
    total
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn timed<R>(f: impl FnOnce() -> R) -> (Duration, R) {
    let start = Instant::now();
    let r = f();
    (start.elapsed(), r)
}

pub fn run(scratch: &Path, paths: &[String], gsr1: &[Vec<u8>], json: &[Vec<u8>]) -> Vec<EngineRow> {
    let mut rows = Vec::new();
    for (payload, records) in [("GSR1", gsr1), ("json", json)] {
        let engines: Vec<Box<dyn Engine>> = vec![
            Box::new(Packs::default()),
            Box::new(Sqlite),
            Box::new(Redb),
        ];
        for mut engine in engines {
            let dir = scratch.join(format!("{}-{payload}", engine.name()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch");
            match measure(engine.as_mut(), &dir, paths, records) {
                Ok(mut row) => {
                    row.payload = payload.into();
                    println!(
                        "  {:<7} {:<5} disk {:>8.2} MB  bulk {:>8.1} ms  update-1 {:>7.1} ms  read-all {:>7.1} ms  read-50 {:>6.2} ms",
                        row.engine,
                        payload,
                        row.disk_bytes as f64 / 1e6,
                        row.bulk_write_ms,
                        row.update_one_ms,
                        row.read_all_ms,
                        row.read_50_ms
                    );
                    rows.push(row);
                }
                Err(error) => println!("  {} {payload}: failed: {error}", engine.name()),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    rows
}

fn measure(engine: &mut dyn Engine, dir: &Path, paths: &[String], records: &[Vec<u8>]) -> Res<EngineRow> {
    let (bulk, r) = timed(|| engine.bulk_write(dir, paths, records));
    r?;
    // Median of three single-record updates of different files.
    let mut updates = Vec::new();
    for i in 0..3 {
        let at = (i + 1) * paths.len() / 4;
        let mut changed = records[at].clone();
        changed.push(b' ');
        let (d, r) = timed(|| engine.update_one(dir, &paths[at], &changed));
        r?;
        updates.push(d);
    }
    updates.sort();
    let disk = disk_usage(dir);
    let mut reads = Vec::new();
    for _ in 0..3 {
        let (d, r) = timed(|| engine.read_all(dir));
        let all = r?;
        if all.len() != paths.len() {
            return Err(format!("read {} of {} records", all.len(), paths.len()).into());
        }
        reads.push(d);
    }
    reads.sort();
    let step = (paths.len() / 50).max(1);
    let selected: Vec<&str> = paths.iter().step_by(step).take(50).map(String::as_str).collect();
    let mut sel = Vec::new();
    for _ in 0..3 {
        let (d, r) = timed(|| engine.read_selected(dir, &selected));
        let got = r?;
        if got.len() != selected.len() {
            return Err("selected read incomplete".into());
        }
        sel.push(d);
    }
    sel.sort();
    Ok(EngineRow {
        engine: engine.name().into(),
        payload: String::new(),
        disk_bytes: disk,
        bulk_write_ms: ms(bulk),
        update_one_ms: ms(updates[1]),
        read_all_ms: ms(reads[1]),
        read_50_ms: ms(sel[1]),
    })
}

// ------------------------------------------------------------ packs

#[derive(Default)]
struct Packs {
    generation: usize,
}

#[derive(Serialize, serde::Deserialize, Clone)]
struct Ref {
    pack: String,
    offset: usize,
    len: usize,
}

fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

impl Packs {
    fn current(&self, dir: &Path) -> std::path::PathBuf {
        dir.join(format!("g{}", self.generation))
    }
    fn index(&self, dir: &Path) -> Res<BTreeMap<String, Ref>> {
        Ok(serde_json::from_slice(&std::fs::read(self.current(dir).join("index.json"))?)?)
    }
}

impl Engine for Packs {
    fn name(&self) -> &'static str {
        "packs"
    }
    fn bulk_write(&mut self, dir: &Path, paths: &[String], records: &[Vec<u8>]) -> Res<()> {
        let g = self.current(dir);
        std::fs::create_dir_all(&g)?;
        let mut index = BTreeMap::new();
        let mut buffer: Vec<u8> = Vec::new();
        let mut pending = Vec::new();
        let flush = |buffer: &mut Vec<u8>, pending: &mut Vec<(String, usize, usize)>, index: &mut BTreeMap<String, Ref>| -> Res<()> {
            if buffer.is_empty() {
                return Ok(());
            }
            let name = blake3::hash(buffer).to_hex().to_string();
            replace(&g.join(&name), buffer)?;
            for (path, offset, len) in pending.drain(..) {
                index.insert(path, Ref { pack: name.clone(), offset, len });
            }
            buffer.clear();
            Ok(())
        };
        for (path, record) in paths.iter().zip(records) {
            pending.push((path.clone(), buffer.len(), record.len()));
            buffer.extend_from_slice(record);
            if buffer.len() >= 8 << 20 {
                flush(&mut buffer, &mut pending, &mut index)?;
            }
        }
        flush(&mut buffer, &mut pending, &mut index)?;
        replace(&g.join("index.json"), &serde_json::to_vec(&index)?)?;
        sync_dir(&g)?;
        Ok(())
    }
    fn update_one(&mut self, dir: &Path, path: &str, record: &[u8]) -> Res<()> {
        let old = self.current(dir);
        let mut index = self.index(dir)?;
        self.generation += 1;
        let new = self.current(dir);
        std::fs::create_dir_all(&new)?;
        let live: std::collections::BTreeSet<String> = index
            .iter()
            .filter(|(p, _)| p.as_str() != path)
            .map(|(_, r)| r.pack.clone())
            .collect();
        for pack in &live {
            std::fs::hard_link(old.join(pack), new.join(pack))?;
        }
        let name = blake3::hash(record).to_hex().to_string();
        replace(&new.join(&name), record)?;
        index.insert(path.to_owned(), Ref { pack: name, offset: 0, len: record.len() });
        replace(&new.join("index.json"), &serde_json::to_vec(&index)?)?;
        sync_dir(&new)?;
        sync_dir(dir)?;
        std::fs::remove_dir_all(old)?;
        Ok(())
    }
    fn read_all(&mut self, dir: &Path) -> Res<Vec<(String, Vec<u8>)>> {
        let index = self.index(dir)?;
        let mut packs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let mut out = Vec::with_capacity(index.len());
        for (path, r) in index {
            if !packs.contains_key(&r.pack) {
                packs.insert(r.pack.clone(), std::fs::read(self.current(dir).join(&r.pack))?);
            }
            out.push((path, packs[&r.pack][r.offset..r.offset + r.len].to_vec()));
        }
        Ok(out)
    }
    fn read_selected(&mut self, dir: &Path, paths: &[&str]) -> Res<Vec<Vec<u8>>> {
        use std::io::{Read, Seek, SeekFrom};
        let index = self.index(dir)?;
        let mut out = Vec::new();
        for path in paths {
            let r = &index[*path];
            let mut f = std::fs::File::open(self.current(dir).join(&r.pack))?;
            f.seek(SeekFrom::Start(r.offset as u64))?;
            let mut buf = vec![0; r.len];
            f.read_exact(&mut buf)?;
            out.push(buf);
        }
        Ok(out)
    }
}

// ------------------------------------------------------------ sqlite

struct Sqlite;

fn sqlite(dir: &Path) -> Res<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(dir.join("records.sqlite"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "fullfsync", "ON")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS records(path TEXT PRIMARY KEY, body BLOB NOT NULL)")?;
    Ok(conn)
}

impl Engine for Sqlite {
    fn name(&self) -> &'static str {
        "sqlite"
    }
    fn bulk_write(&mut self, dir: &Path, paths: &[String], records: &[Vec<u8>]) -> Res<()> {
        let mut conn = sqlite(dir)?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("INSERT INTO records(path, body) VALUES (?1, ?2)")?;
            for (path, record) in paths.iter().zip(records) {
                stmt.execute(rusqlite::params![path, record])?;
            }
        }
        tx.commit()?;
        // Fold the WAL into the main file so disk size and read paths are steady-state.
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        Ok(())
    }
    fn update_one(&mut self, dir: &Path, path: &str, record: &[u8]) -> Res<()> {
        let conn = sqlite(dir)?;
        conn.execute("UPDATE records SET body = ?2 WHERE path = ?1", rusqlite::params![path, record])?;
        Ok(())
    }
    fn read_all(&mut self, dir: &Path) -> Res<Vec<(String, Vec<u8>)>> {
        let conn = sqlite(dir)?;
        let mut stmt = conn.prepare("SELECT path, body FROM records ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
    fn read_selected(&mut self, dir: &Path, paths: &[&str]) -> Res<Vec<Vec<u8>>> {
        let conn = sqlite(dir)?;
        let mut stmt = conn.prepare("SELECT body FROM records WHERE path = ?1")?;
        let mut out = Vec::new();
        for path in paths {
            out.push(stmt.query_row([path], |r| r.get(0))?);
        }
        Ok(out)
    }
}

// ------------------------------------------------------------ redb

struct Redb;
const TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("records");

impl Engine for Redb {
    fn name(&self) -> &'static str {
        "redb"
    }
    fn bulk_write(&mut self, dir: &Path, paths: &[String], records: &[Vec<u8>]) -> Res<()> {
        let db = redb::Database::create(dir.join("records.redb"))?;
        let tx = db.begin_write()?;
        {
            let mut table = tx.open_table(TABLE)?;
            for (path, record) in paths.iter().zip(records) {
                table.insert(path.as_str(), record.as_slice())?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    fn update_one(&mut self, dir: &Path, path: &str, record: &[u8]) -> Res<()> {
        let db = redb::Database::open(dir.join("records.redb"))?;
        let tx = db.begin_write()?;
        tx.open_table(TABLE)?.insert(path, record)?;
        tx.commit()?;
        Ok(())
    }
    fn read_all(&mut self, dir: &Path) -> Res<Vec<(String, Vec<u8>)>> {
        use redb::ReadableTable;
        let db = redb::Database::open(dir.join("records.redb"))?;
        let tx = db.begin_read()?;
        let table = tx.open_table(TABLE)?;
        let mut out = Vec::new();
        for entry in table.iter()? {
            let (k, v) = entry?;
            out.push((k.value().to_owned(), v.value().to_vec()));
        }
        Ok(out)
    }
    fn read_selected(&mut self, dir: &Path, paths: &[&str]) -> Res<Vec<Vec<u8>>> {
        let db = redb::Database::open(dir.join("records.redb"))?;
        let tx = db.begin_read()?;
        let table = tx.open_table(TABLE)?;
        let mut out = Vec::new();
        for path in paths {
            out.push(table.get(*path)?.ok_or("missing")?.value().to_vec());
        }
        Ok(out)
    }
}
