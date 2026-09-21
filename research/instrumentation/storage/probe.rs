use std::{io::{self,Write}, path::PathBuf, time::Instant};
use graph_search::{Index,OpenOptions};
use graph_search_engine::{GrafeoStore,StoreOptions};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args:Vec<_> = std::env::args().collect();
    assert_eq!(args.len(),4,"usage: storage_composition_probe build|open ROOT STORE");
    let started = Instant::now();
    if args[1] == "build" {
        let index=Index::open(OpenOptions {root:PathBuf::from(&args[2]),store:Some(PathBuf::from(&args[3])),..Default::default()})?;
        index.reindex()?;
        println!("{}",json!({"build_ns":started.elapsed().as_nanos()}));
    } else {
        assert_eq!(args[1],"open");
        let store=GrafeoStore::open(&PathBuf::from(&args[3]),&StoreOptions::default())?;
        println!("{}",json!({"ready":true,"pid":std::process::id(),"open_ns":started.elapsed().as_nanos()}));
        io::stdout().flush()?;
        let mut line=String::new();
        io::stdin().read_line(&mut line)?;
        assert_eq!(line.trim(),"measure");
        println!("{}",store.research_storage());
    }
    Ok(())
}
