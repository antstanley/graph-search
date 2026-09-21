//! Cross-process generation retention and crash-release contracts.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use graph_search_core::{conformance, ports::GraphStore};
use graph_search_engine::{GrafeoStore, StoreOptions};
use graph_search_types::{
    EdgeKind, WriteBatch,
    extraction::{Extraction, ReferenceFact},
    manifest::FileEntry,
};
use std::{
    io::{BufRead, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn batch(stamp: u64) -> WriteBatch {
    let mut batch = conformance::fixture_batch();
    batch.upserts[0].symbols[0].name = Some(format!("generation{stamp}"));
    batch.manifest.indexed_at_ms = stamp;
    batch.manifest.entries.insert(
        "src/a.rs".into(),
        FileEntry {
            size: 10,
            mtime_ns: 1,
            content_hash: "ha".into(),
            parser_version: 1,
            schema_version: 1,
            quarantine: None,
            extraction: Some(
                Extraction {
                    references: vec![ReferenceFact::file_level(
                        EdgeKind::Calls,
                        format!("generation{stamp}"),
                        1,
                    )],
                    ..Default::default()
                }
                .into(),
            ),
        },
    );
    batch
}

#[test]
#[allow(clippy::exit, clippy::print_stdout)] // Child protocol and deliberate exit without Drop.
fn reader_child() {
    let Ok(root) = std::env::var("GRAPH_SEARCH_READER_ROOT") else {
        return;
    };
    let reader = GrafeoStore::open(Path::new(&root), &StoreOptions::default()).unwrap();
    let original = reader.manifest().unwrap();
    let nodes = reader.snapshot().unwrap().all_nodes().unwrap();
    println!("READER_READY");
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "check" => {
                assert_eq!(reader.manifest().unwrap(), original);
                assert_eq!(reader.snapshot().unwrap().all_nodes().unwrap(), nodes);
                println!("READER_CHECKED");
                std::io::stdout().flush().unwrap();
            }
            "exit" => return,
            "crash" => std::process::exit(87),
            other => panic!("unexpected reader command: {other}"),
        }
    }
}

struct Reader {
    child: Child,
    lines: mpsc::Receiver<String>,
    output: Option<std::thread::JoinHandle<()>>,
}
impl Reader {
    fn open(root: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "reader_child", "--nocapture"])
            .env("GRAPH_SEARCH_READER_ROOT", root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        let output = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.starts_with("READER_") && sender.send(line).is_err() {
                    break;
                }
            }
        });
        let reader = Self {
            child,
            lines,
            output: Some(output),
        };
        reader.expect("READER_READY");
        reader
    }
    fn expect(&self, expected: &str) {
        assert_eq!(
            self.lines.recv_timeout(Duration::from_secs(60)).unwrap(),
            expected
        );
    }
    fn send(&mut self, command: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}

#[test]
fn multiple_process_readers_survive_churn_and_exit_releases_retention() {
    let root = tempfile::tempdir().unwrap();
    let mut writer = GrafeoStore::open(root.path(), &StoreOptions::default()).unwrap();
    writer.publish(batch(0)).unwrap();
    let retained = root
        .path()
        .join("generations")
        .join(writer.generation().unwrap().unwrap());
    let mut first = Reader::open(root.path());
    let mut second = Reader::open(root.path());
    for stamp in 1..=8 {
        writer.publish(batch(stamp)).unwrap();
        for reader in [&mut first, &mut second] {
            reader.send("check");
            reader.expect("READER_CHECKED");
        }
        assert!(retained.exists());
        assert!(
            std::fs::read_dir(root.path().join("generations"))
                .unwrap()
                .count()
                <= 3
        );
    }
    first.send("exit");
    assert!(first.child.wait().unwrap().success());
    writer.publish(batch(9)).unwrap();
    assert!(retained.exists(), "second reader still holds the lease");
    second.send("check");
    second.expect("READER_CHECKED");
    second.send("crash");
    assert_eq!(second.child.wait().unwrap().code(), Some(87));
    writer.publish(batch(10)).unwrap();
    assert!(
        !retained.exists(),
        "OS releases the lease even without Rust Drop"
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("generations"))
            .unwrap()
            .count(),
        2
    );
}
