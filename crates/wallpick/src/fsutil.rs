use crate::error::{Result, io};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Writes `data` to a temporary sibling, syncs it and renames it over `path`, so readers see the
/// old or the new contents, never a torn file (rename(2) is atomic within a filesystem).
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let tmp = part_path(path);
    let res = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res.map_err(io(path))?;
    sync_dir(dir);
    Ok(())
}

/// `<name>.<pid>.<n>.part` next to `path`: unique per process and call, so concurrent writers never
/// share a temporary file.
pub(crate) fn part_path(path: &Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let n = N.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".{name}.{}.{n}.part", std::process::id()))
}

/// Makes a completed rename durable (fsync of the directory entry); best effort.
pub(crate) fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}
