use std::io::Write;
use std::path::{Path, PathBuf};

/// Write through a unique temporary file in the same directory + rename, so readers never
/// see a half-written file: concurrent writers never share a temporary, the data and the
/// rename are fsynced (no empty file after a power loss), and the old file's mode is kept.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let mode = match std::fs::metadata(path) {
        Ok(m) => m.permissions().mode() & 0o7777,
        Err(_) => 0o644,
    };
    let (tmp, mut f) = loop {
        let mut n = std::ffi::OsString::from(".");
        n.push(name);
        n.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let tmp = dir.join(n);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let res = (|| {
        f.write_all(data)?;
        // `mode()` above is filtered by the umask; the old file's mode wins.
        f.set_permissions(std::fs::Permissions::from_mode(mode))?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return res;
    }
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// `$(cat file)` / `head -n1 file`: the first line, without its newline.
pub fn first_line(path: &Path) -> Option<String> {
    let t = std::fs::read_to_string(path).ok()?;
    Some(t.split('\n').next().unwrap_or("").to_string())
}

/// The shell's state in `$XDG_STATE_HOME` (`base`): what it keeps is under `user` there.
pub fn state_dir(base: &Path) -> PathBuf {
    named_dir(base, "user")
}

/// The shell's cache in `$XDG_CACHE_HOME` (`base`).
pub fn cache_dir(base: &Path) -> PathBuf {
    named_dir(base, "")
}

/// The shell's directory in a base directory: `impulse-shell`. An older release has it as
/// `rustshell`, and tools/install.sh moves that over. A build that is run without that
/// install reads and writes the older release's directory, while the one name holds none of
/// the shell's own (`holds`, a path every such directory has in it): the shell is not new to
/// that home.
fn named_dir(base: &Path, holds: &str) -> PathBuf {
    let (new, old) = (base.join("impulse-shell"), base.join("rustshell"));
    if !new.join(holds).exists() && old.join(holds).is_dir() {
        old
    } else {
        new
    }
}

#[cfg(test)]
mod tests {
    use super::{cache_dir, state_dir, write_atomic};
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("colorgen-fsutil-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_atomic_keeps_mode() {
        let d = dir("mode");
        let p = d.join("config.json");
        std::fs::write(&p, "{}").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic(&p, b"{\"a\": 1}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"a\": 1}");
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o7777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_atomic_leaves_other_files_alone() {
        let d = dir("tmp");
        let p = d.join("colors.json");
        let other = d.join("colors.json.tmp");
        std::fs::write(&other, "mine").unwrap();
        write_atomic(&p, b"new").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"new");
        assert_eq!(std::fs::read(&other).unwrap(), b"mine");
        let left: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn write_atomic_concurrent_writers() {
        let d = dir("race");
        let p = d.join("config.json");
        let bodies: Vec<Vec<u8>> = (0..8).map(|i| vec![b'a' + i as u8; 64 * 1024]).collect();
        std::thread::scope(|s| {
            for b in &bodies {
                let p = &p;
                s.spawn(move || {
                    for _ in 0..50 {
                        write_atomic(p, b).unwrap();
                    }
                });
            }
        });
        let got = std::fs::read(&p).unwrap();
        assert!(bodies.contains(&got), "torn file of {} bytes", got.len());
        let n = std::fs::read_dir(&d).unwrap().count();
        assert_eq!(n, 1, "temporary files left behind");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A build run without an install finds an older release's state and cache under the
    /// name they had, and the one name's as soon as that holds the shell's own. It took an
    /// older release's home for a new one: the shell greeted, and wrote its colours over.
    #[test]
    fn an_older_releases_directories_are_read_until_an_install_moves_them() {
        let d = dir("named");
        let (state, cache) = (d.join("state"), d.join("cache"));
        assert_eq!(state_dir(&state), state.join("impulse-shell"));
        assert_eq!(cache_dir(&cache), cache.join("impulse-shell"));
        // What an older release's install keeps under the one name: the configs it replaced.
        std::fs::create_dir_all(state.join("impulse-shell/replaced-1")).unwrap();
        std::fs::create_dir_all(state.join("rustshell/user")).unwrap();
        std::fs::create_dir_all(cache.join("rustshell")).unwrap();
        assert_eq!(state_dir(&state), state.join("rustshell"));
        assert_eq!(cache_dir(&cache), cache.join("rustshell"));
        // An install moved them, and a shell from before, still running, wrote its own again.
        std::fs::create_dir_all(state.join("impulse-shell/user")).unwrap();
        std::fs::create_dir_all(cache.join("impulse-shell")).unwrap();
        assert_eq!(state_dir(&state), state.join("impulse-shell"));
        assert_eq!(cache_dir(&cache), cache.join("impulse-shell"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
