//! Durability for a publication: many flushes, then one barrier.
//!
//! A generation writes dozens of files and directories before its `CURRENT`
//! pointer. Each is [`flush`]ed as it is written, the pointer's own bytes
//! included, and one [`barrier`] makes all of them durable before the rename
//! that exposes the pointer; that rename's directory is then [`sync`]ed.
//!
//! On Apple platforms `fsync(2)` hands data to the drive without flushing the
//! drive's cache, and std's `sync_all` is `F_FULLFSYNC`, which flushes the whole
//! cache: correct, but it cost a quarter of a one-file sync when every file
//! paid it. There a flush is `fsync(2)` and the barrier is one `F_FULLFSYNC`,
//! which also covers every earlier flush. Elsewhere `fsync(2)` is already
//! durable, so a flush is a full sync and the barrier has nothing left to do.

use std::fs::File;
use std::io;
use std::path::Path;

/// Hands `file`'s written bytes to the device; durable after the next
/// [`barrier`].
pub(crate) fn flush(file: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        rustix::fs::fsync(file).map_err(io::Error::from)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        file.sync_all()
    }
}

/// [`flush`] for a directory's entries.
pub(crate) fn flush_dir(dir: &Path) -> io::Result<()> {
    flush(&File::open(dir)?)
}

/// Makes every earlier flush durable. `dir` is any directory on the store's
/// volume.
pub(crate) fn barrier(dir: &Path) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = dir;
        Ok(())
    }
}

/// A full, immediately durable sync of `file`, for the commit point itself.
pub(crate) fn sync(file: &File) -> io::Result<()> {
    file.sync_all()
}
