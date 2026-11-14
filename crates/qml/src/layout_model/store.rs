//! The layout file. Saving writes a temporary file in the same directory and renames it
//! over the old one (fsynced, [`colorgen::fsutil::write_atomic`]), so a crash leaves either
//! the old or the new layout, and keeps the previous good file as `layout.toml.prev`. A
//! file that can't be read at startup falls back to that copy and is moved aside to
//! `layout.toml.broken-<time>` on the next save instead of being lost; a file from a newer
//! build is never overwritten, nor is one changed by someone else since it was read.

use super::migrate::{self, Inputs};
use super::{Layout, LoadError, format};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadSource {
    Main,
    /// The main file was unusable; this is `layout.toml.prev`.
    Previous,
    /// Neither file was usable or there is none yet: the layout is empty and the shell
    /// builds its default.
    Empty,
}

#[derive(Debug)]
pub struct Loaded {
    pub layout: Layout,
    pub source: LoadSource,
    /// Why the main file (and the previous one) were not used, in that order.
    pub problems: Vec<LoadError>,
    /// The schema version of the file read, when it was older and got migrated.
    pub migrated_from: Option<u32>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    /// The main file exists but is unusable: kept aside before it is replaced.
    broken: bool,
    /// Saving would destroy a layout this build can't read (newer version, unreadable file).
    read_only: Option<String>,
    /// The main file's bytes as last read or written (`None`: there was none). A file that
    /// no longer holds them was changed by someone else.
    disk: Option<Vec<u8>>,
    /// The main file's settled [`Stamp`] when it held `disk`: while it still matches, the
    /// file holds `disk` and need not be read again.
    stamp: Option<Stamp>,
    /// The older schema version the main file was migrated from, until it is first replaced:
    /// the original is kept as `layout.toml.v<version>` then.
    migrated_from: Option<u32>,
    /// Where the last unusable file was moved aside to.
    kept_aside: Option<PathBuf>,
    /// What migrating an old file reads besides it.
    inputs: Inputs,
}

/// What [`Store::poll`] found in the main file.
#[derive(Debug)]
pub enum Polled {
    /// It still holds what was last read or written.
    Unchanged,
    /// It holds a different, valid layout.
    Changed(Layout),
    /// It is gone.
    Missing,
    /// It holds something unusable; the layout in memory stays.
    Invalid(LoadError),
}

/// What identifies one version of a file without reading it: replacing it (a new inode) or
/// writing to it (size, modification time) changes it, as editors and `make` rely on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: std::time::SystemTime,
}

/// File systems keep modification times at a coarse granularity (jiffies, up to 2 s on
/// FAT): a write within that window of the last one can leave the stamp as it was.
const MTIME_GRANULARITY: std::time::Duration = std::time::Duration::from_secs(2);

/// The file's stamp, and whether it can stand for its content: like git's "racily clean"
/// index entries, one modified within [`MTIME_GRANULARITY`] of `seen` (just before the
/// stamp was taken) could still change without the stamp changing, so it is not trusted.
fn stamp(path: &Path, seen: std::time::SystemTime) -> io::Result<Option<(Stamp, bool)>> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m.modified()?;
            let settled = mtime + MTIME_GRANULARITY < seen;
            Ok(Some((
                Stamp {
                    dev: m.dev(),
                    ino: m.ino(),
                    len: m.len(),
                    mtime,
                },
                settled,
            )))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The main file's bytes, and its stamp if that can stand for them.
type Current = (Option<Vec<u8>>, Option<Stamp>);

fn sibling(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

fn read_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// A file that is not UTF-8 is as broken as one that is not TOML: it falls back to the
/// previous copy like any other unparseable file instead of making the store read-only.
fn parse(bytes: &[u8], inputs: &Inputs) -> Result<(Layout, u32), LoadError> {
    let text =
        std::str::from_utf8(bytes).map_err(|e| LoadError::Syntax(format!("not UTF-8: {e}")))?;
    format::parse(text, migrate::MIGRATIONS, inputs)
}

fn read(path: &Path, inputs: &Inputs) -> Result<Option<(Layout, u32)>, LoadError> {
    match read_bytes(path) {
        Ok(Some(b)) => parse(&b, inputs).map(Some),
        Ok(None) => Ok(None),
        Err(e) => Err(LoadError::Io(e.to_string())),
    }
}

/// Errors that keep the file from being replaced: losing it would lose data this build
/// can't see. Everything else is a broken file, kept aside on the next save.
fn protects(e: &LoadError) -> bool {
    matches!(e, LoadError::FutureVersion { .. } | LoadError::Io(_))
}

impl Store {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn previous_path(&self) -> PathBuf {
        sibling(&self.path, ".prev")
    }

    /// Where the original of a layout migrated from schema `version` is kept.
    pub fn migrated_path(&self, version: u32) -> PathBuf {
        sibling(&self.path, &format!(".v{version}"))
    }

    /// Where the last unusable main file was moved aside to by [`Store::save`].
    pub fn kept_aside(&self) -> Option<&Path> {
        self.kept_aside.as_deref()
    }

    /// Why [`Store::save`] refuses to write, if it does.
    pub fn read_only(&self) -> Option<&str> {
        self.read_only.as_deref()
    }

    /// Opens the layout at `path`; an old one is migrated with the user's options
    /// ([`Inputs::ri`]).
    pub fn open(path: impl Into<PathBuf>) -> (Store, Loaded) {
        Store::open_with(path, Inputs::ri())
    }

    /// [`Store::open`] migrating with `inputs`.
    pub fn open_with(path: impl Into<PathBuf>, inputs: Inputs) -> (Store, Loaded) {
        let mut store = Store {
            path: path.into(),
            broken: false,
            read_only: None,
            disk: None,
            stamp: None,
            migrated_from: None,
            kept_aside: None,
            inputs,
        };
        let mut problems = Vec::new();
        let main = match store.current() {
            Ok((None, _)) => None,
            Ok((Some(bytes), stamp)) => {
                let parsed = parse(&bytes, &store.inputs);
                store.disk = Some(bytes);
                store.stamp = stamp;
                match parsed {
                    Ok(l) => Some((l, LoadSource::Main)),
                    Err(e) => {
                        store.refuse(&e);
                        problems.push(e);
                        None
                    }
                }
            }
            Err(e) => {
                let e = LoadError::Io(e.to_string());
                store.refuse(&e);
                problems.push(e);
                None
            }
        };
        let found = match main {
            Some(m) => Some(m),
            None if problems.is_empty() => None,
            None => match read(&store.previous_path(), &store.inputs) {
                Ok(Some(l)) => Some((l, LoadSource::Previous)),
                Ok(None) => None,
                Err(e) => {
                    problems.push(e);
                    None
                }
            },
        };
        let loaded = match found {
            Some(((layout, version), source)) => {
                let migrated_from = (version < super::VERSION).then_some(version);
                if source == LoadSource::Main {
                    store.migrated_from = migrated_from;
                }
                Loaded {
                    layout,
                    source,
                    problems,
                    migrated_from,
                }
            }
            None => Loaded {
                layout: Layout::default(),
                source: LoadSource::Empty,
                problems,
                migrated_from: None,
            },
        };
        (store, loaded)
    }

    fn refuse(&mut self, e: &LoadError) {
        if protects(e) {
            self.read_only = Some(format!("{}: {e}", self.path.display()));
        } else {
            self.broken = true;
        }
    }

    /// Reads the main file again after it changed on disk. Unlike [`Store::open`] this never
    /// falls back to `layout.toml.prev`: a file caught half-written by an editor, or saved
    /// with a typo, leaves the running layout as it is, and the file unwritten, until it is
    /// valid again. The shell's own writes come back as [`Polled::Unchanged`] unparsed.
    pub fn poll(&mut self) -> Polled {
        let (bytes, stamp) = match self.current() {
            Ok(r) => r,
            Err(e) => return Polled::Invalid(LoadError::Io(e.to_string())),
        };
        if bytes == self.disk {
            return Polled::Unchanged;
        }
        self.stamp = stamp;
        let Some(bytes) = bytes else {
            self.disk = None;
            self.broken = false;
            self.read_only = None;
            self.migrated_from = None;
            return Polled::Missing;
        };
        let parsed = parse(&bytes, &self.inputs);
        self.disk = Some(bytes);
        self.broken = false;
        self.read_only = None;
        match parsed {
            Ok((layout, version)) => {
                self.migrated_from = (version < super::VERSION).then_some(version);
                Polled::Changed(layout)
            }
            Err(e) => {
                self.read_only = Some(format!("{}: {e}", self.path.display()));
                Polled::Invalid(e)
            }
        }
    }

    /// Writes `layout` unless the file already holds exactly it. `Ok(false)`: unchanged.
    /// A file changed by someone else since it was last read is not overwritten
    /// ([`io::ErrorKind::AlreadyExists`]): the change reaches the shell through
    /// [`Store::poll`] instead of being lost.
    pub fn save(&mut self, layout: &Layout) -> io::Result<bool> {
        if let Some(why) = &self.read_only {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("not overwriting the layout: {why}"),
            ));
        }
        let text = layout.to_toml();
        let (old, _) = self.current()?;
        if old != self.disk {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "not overwriting the layout: {} changed on disk since it was read",
                    self.path.display()
                ),
            ));
        }
        if old.as_deref() == Some(text.as_bytes()) {
            return Ok(false);
        }
        if self.broken {
            let aside = self.aside_path();
            std::fs::rename(&self.path, &aside)?;
            self.kept_aside = Some(aside);
            self.broken = false;
        } else if let Some(old) = &old {
            if let Some(v) = self.migrated_from {
                let kept = self.migrated_path(v);
                if !kept.exists() {
                    colorgen::fsutil::write_atomic(&kept, old)?;
                }
            }
            self.keep_previous(old)?;
        }
        colorgen::fsutil::write_atomic(&self.path, text.as_bytes())?;
        self.stamp = None;
        self.disk = Some(text.into_bytes());
        self.migrated_from = None;
        Ok(true)
    }

    /// The main file's bytes: `disk` while its settled [`Stamp`] is unchanged, else read
    /// again. The stamp of bytes equal to `disk` is kept (once settled, later calls skip the
    /// read); a caller taking different bytes as `disk` takes the returned stamp with them.
    fn current(&mut self) -> io::Result<Current> {
        let seen = std::time::SystemTime::now();
        let now = stamp(&self.path, seen)?;
        if let Some((s, _)) = now
            && Some(s) == self.stamp
        {
            return Ok((self.disk.clone(), self.stamp));
        }
        let bytes = read_bytes(&self.path)?;
        let settled = now.filter(|(_, settled)| *settled).map(|(s, _)| s);
        if bytes == self.disk {
            self.stamp = settled;
        }
        Ok((bytes, settled))
    }

    /// Makes the main file, holding `old`, the previous copy before it is replaced: a hard
    /// link renamed over `layout.toml.prev` keeps the same data without writing or syncing it
    /// again; the main file's own atomic write then syncs the directory for both. On a file
    /// system without hard links it is copied.
    fn keep_previous(&self, old: &[u8]) -> io::Result<()> {
        let prev = self.previous_path();
        let tmp = sibling(&prev, &format!(".{}.link", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        match std::fs::hard_link(&self.path, &tmp) {
            Ok(()) => std::fs::rename(&tmp, &prev).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            }),
            Err(_) => colorgen::fsutil::write_atomic(&prev, old),
        }
    }

    /// A name for the unusable main file that no earlier one took: each broken file is kept.
    fn aside_path(&self) -> PathBuf {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        (0..)
            .map(|n| match n {
                0 => sibling(&self.path, &format!(".broken-{secs}")),
                n => sibling(&self.path, &format!(".broken-{secs}-{n}")),
            })
            .find(|p| !p.exists())
            .expect("an unused name")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_migrated_file_is_kept_once_under_its_version() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("layout.toml");
        let (mut s, _) = Store::open(&p);
        s.save(&Layout::default()).unwrap();
        let original = std::fs::read(&p).unwrap();
        // As if the file had been read from schema version 0.
        s.migrated_from = Some(0);
        let mut l = Layout::default();
        l.monitors.push(super::super::MonitorLayout {
            connector: "DP-1".into(),
            edid: None,
            primary: true,
            containers: vec![],
        });
        assert!(s.save(&l).unwrap());
        assert_eq!(std::fs::read(s.migrated_path(0)).unwrap(), original);
        l.monitors[0].connector = "DP-2".into();
        assert!(s.save(&l).unwrap());
        assert_eq!(std::fs::read(s.migrated_path(0)).unwrap(), original);
    }

    fn one_monitor(connector: &str) -> Layout {
        let mut l = Layout::default();
        l.monitors.push(super::super::MonitorLayout {
            connector: connector.into(),
            edid: None,
            primary: true,
            containers: vec![],
        });
        l
    }

    #[test]
    fn a_racy_same_size_rewrite_is_still_seen() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("layout.toml");
        let (mut s, _) = Store::open(&p);
        s.save(&one_monitor("DP-1")).unwrap();
        // Same length, written in place right after the save: the stamp may not move.
        std::fs::write(&p, one_monitor("DP-2").to_toml()).unwrap();
        assert!(matches!(s.poll(), Polled::Changed(l) if l == one_monitor("DP-2")));
    }

    #[test]
    fn a_settled_file_is_not_read_again() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("layout.toml");
        std::fs::write(&p, one_monitor("DP-1").to_toml()).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let (mut s, loaded) = Store::open(&p);
        assert_eq!(loaded.layout, one_monitor("DP-1"));
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&p).is_ok() {
            return; // root reads anything
        }
        assert!(matches!(s.poll(), Polled::Unchanged));
        assert!(!s.save(&one_monitor("DP-1")).unwrap());
    }
}
