//! The advisory writer lock at `<store>/index.lock` (`SPEC.md` §6.6).
//!
//! One writer at a time; readers never lock. A refused writer is told who
//! holds the lock, the same posture as `nanus`'s session claim.

use graph_search_core::Result;
use graph_search_core::error::Error;
use std::fs::File;
use std::path::Path;

/// A held lock; released on drop.
pub struct IndexLock {
    _file: File,
}

/// Tries to take the lock at `store_dir/index.lock`.
///
/// # Errors
/// [`Error::Locked`] when another writer holds it; io errors surface as
/// [`Error::Io`].
pub fn try_lock(store_dir: &Path) -> Result<IndexLock> {
    std::fs::create_dir_all(store_dir).map_err(|source| Error::io(store_dir, source))?;
    let path = store_dir.join("index.lock");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| Error::io(&path, source))?;
    if file.try_lock().is_err() {
        let holder = std::fs::read_to_string(&path)
            .map_or_else(|_| String::from("pid ?"), |text| text.trim().to_owned());
        return Err(Error::Locked { holder });
    }
    let pid = std::process::id();
    file.set_len(0).map_err(|source| Error::io(&path, source))?;
    std::io::Write::write_all(&mut file, format!("pid {pid}").as_bytes())
        .map_err(|source| Error::io(&path, source))?;
    std::io::Write::flush(&mut file).map_err(|source| Error::io(&path, source))?;
    Ok(IndexLock { _file: file })
}
