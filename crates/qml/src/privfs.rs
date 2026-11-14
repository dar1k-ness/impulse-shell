//! Files in shared, world-writable places (the original's `/tmp/rustshell/...` directories) written
//! without following anybody else's symlinks. The directory is reached by file descriptor one
//! component at a time; every component must belong to this user or to root (a root-owned
//! world-writable one only when sticky, like `/tmp`), so nobody else can swap a component
//! for a symlink, and the directory itself is made private (0700). Files are written to a
//! fresh `O_EXCL` temporary inside it and renamed into place.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// A private directory, reached safely (see the module docs).
pub struct Dir(OwnedFd);

fn euid() -> u32 {
    // SAFETY: geteuid(2) takes no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

fn denied(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{}: not a directory of this user", path.display()),
    )
}

fn bad_name(name: &OsStr) -> io::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.as_bytes().contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("bad file name {name:?}"),
        ));
    }
    Ok(())
}

/// An ancestor nobody else can change: ours, or root's and not writable by others unless
/// sticky.
fn trusted(fd: &OwnedFd) -> io::Result<bool> {
    let st = rustix::fs::fstat(fd)?;
    let mode = st.st_mode as u32;
    Ok(st.st_uid == euid() || (st.st_uid == 0 && (mode & 0o022 == 0 || mode & 0o1000 != 0)))
}

/// `name` in `parent` as a directory, made 0700 when missing and `create`. A symlink is
/// followed only when this user or root made it.
fn step(parent: &OwnedFd, name: &OsStr, create: bool) -> io::Result<OwnedFd> {
    match rustix::fs::openat(parent, name, DIR_FLAGS, Mode::empty()) {
        Ok(fd) => Ok(fd),
        Err(Errno::NOENT) if create => {
            match rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(e) => return Err(e.into()),
            }
            Ok(rustix::fs::openat(parent, name, DIR_FLAGS, Mode::empty())?)
        }
        Err(Errno::LOOP | Errno::NOTDIR) => {
            let st = rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)?;
            if FileType::from_raw_mode(st.st_mode) != FileType::Symlink
                || (st.st_uid != euid() && st.st_uid != 0)
            {
                return Err(Errno::NOTDIR.into());
            }
            Ok(rustix::fs::openat(
                parent,
                name,
                DIR_FLAGS.difference(OFlags::NOFOLLOW),
                Mode::empty(),
            )?)
        }
        Err(e) => Err(e.into()),
    }
}

impl Dir {
    /// The absolute `path`, its missing components made (0700) when `create`. It must be
    /// ours; group and other permissions are taken off it.
    pub fn open(path: &Path, create: bool) -> io::Result<Dir> {
        let mut names = vec![];
        for c in path.components() {
            match c {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(n) => names.push(n),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("{}: not a plain absolute path", path.display()),
                    ));
                }
            }
        }
        if !path.is_absolute() || names.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: not a plain absolute path", path.display()),
            ));
        }
        let mut cur = rustix::fs::open("/", DIR_FLAGS, Mode::empty())?;
        for n in names {
            if !trusted(&cur)? {
                return Err(denied(path));
            }
            cur = step(&cur, n, create)?;
        }
        let st = rustix::fs::fstat(&cur)?;
        if st.st_uid != euid() {
            return Err(denied(path));
        }
        let mode = st.st_mode as u32 & 0o7777;
        if mode & 0o077 != 0 {
            rustix::fs::fchmod(&cur, Mode::from_raw_mode(mode & !0o077))?;
        }
        Ok(Dir(cur))
    }

    /// The directory of `path` (opened as [`Dir::open`]) and its file name.
    pub fn parent_of(path: &Path, create: bool) -> io::Result<(Dir, &OsStr)> {
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{}: no file name", path.display()),
            )
        })?;
        let parent = path.parent().unwrap_or(Path::new("/"));
        Ok((Dir::open(parent, create)?, name))
    }

    /// `rm -rf PATH; mkdir -p PATH`, private.
    pub fn renew(path: &Path) -> io::Result<Dir> {
        let (parent, name) = Dir::parent_of(path, true)?;
        parent.remove_all(name);
        Dir::open(path, true)
    }

    /// `name` is a regular file (not followed).
    pub fn has_file(&self, name: &OsStr) -> bool {
        bad_name(name).is_ok()
            && rustix::fs::statat(&self.0, name, AtFlags::SYMLINK_NOFOLLOW)
                .is_ok_and(|st| FileType::from_raw_mode(st.st_mode) == FileType::RegularFile)
    }

    /// A new temporary file (0600) that [`Pending::commit`] puts in place as `name`; it is
    /// removed when the [`Pending`] is dropped uncommitted.
    pub fn create(&self, name: &OsStr) -> io::Result<(File, Pending<'_>)> {
        static N: AtomicU64 = AtomicU64::new(0);
        bad_name(name)?;
        let mut tmp = OsString::from(".");
        tmp.push(name);
        tmp.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let fd = rustix::fs::openat(
            &self.0,
            &tmp,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?;
        Ok((
            File::from(fd),
            Pending {
                dir: self,
                tmp,
                name: name.to_owned(),
                done: false,
            },
        ))
    }

    /// `data` into `name`, replacing what is there (a symlink itself, never its target).
    pub fn write(&self, name: &OsStr, data: &[u8]) -> io::Result<()> {
        let (mut f, p) = self.create(name)?;
        f.write_all(data)?;
        drop(f);
        p.commit()
    }

    /// `rm -rf DIR/NAME`: removes what it can, symlinks are removed, never followed.
    pub fn remove_all(&self, name: &OsStr) {
        if bad_name(name).is_ok() {
            remove_at(&self.0, name);
        }
    }
}

fn remove_at(dir: &OwnedFd, name: &OsStr) {
    let Ok(st) = rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return;
    };
    if FileType::from_raw_mode(st.st_mode) != FileType::Directory {
        let _ = rustix::fs::unlinkat(dir, name, AtFlags::empty());
        return;
    }
    if let Ok(sub) = rustix::fs::openat(dir, name, DIR_FLAGS, Mode::empty()) {
        let names: Vec<OsString> = rustix::fs::Dir::read_from(&sub)
            .map(|d| {
                d.filter_map(Result::ok)
                    .map(|e| OsStr::from_bytes(e.file_name().to_bytes()).to_owned())
                    .filter(|n| n != "." && n != "..")
                    .collect()
            })
            .unwrap_or_default();
        for n in names {
            remove_at(&sub, &n);
        }
    }
    let _ = rustix::fs::unlinkat(dir, name, AtFlags::REMOVEDIR);
}

/// A file being written by [`Dir::create`].
pub struct Pending<'a> {
    dir: &'a Dir,
    tmp: OsString,
    name: OsString,
    done: bool,
}

impl Pending<'_> {
    pub fn commit(mut self) -> io::Result<()> {
        rustix::fs::renameat(&self.dir.0, &self.tmp, &self.dir.0, &self.name)?;
        self.done = true;
        Ok(())
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = rustix::fs::unlinkat(&self.dir.0, &self.tmp, AtFlags::empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn writes_never_follow_symlinks() {
        let t = tmp();
        let victim = t.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        let d = t.path().join("shared");
        std::fs::create_dir(&d).unwrap();
        symlink(&victim, d.join("f")).unwrap();
        let dir = Dir::open(&d, false).unwrap();
        assert!(!dir.has_file(OsStr::new("f")));
        dir.write(OsStr::new("f"), b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(d.join("f")).unwrap(), "new");
        assert!(dir.has_file(OsStr::new("f")));
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    #[test]
    fn foreign_symlinked_component_is_refused() {
        let t = tmp();
        let target = t.path().join("elsewhere");
        std::fs::create_dir(&target).unwrap();
        // Ours here, so followed; a symlink of another user is refused (tested as root below).
        symlink(&target, t.path().join("link")).unwrap();
        let d = Dir::open(&t.path().join("link/sub"), true).unwrap();
        d.write(OsStr::new("a"), b"x").unwrap();
        assert!(target.join("sub/a").is_file());
        if euid() == 0 {
            let l = t.path().join("foreign");
            symlink(&target, &l).unwrap();
            rustix::fs::chownat(
                rustix::fs::CWD,
                &l,
                Some(rustix::fs::Uid::from_raw(65534)),
                None,
                AtFlags::SYMLINK_NOFOLLOW,
            )
            .unwrap();
            assert!(Dir::open(&l.join("sub"), true).is_err());
        }
    }

    #[test]
    fn made_private() {
        let t = tmp();
        let d = t.path().join("a/b");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        Dir::open(&d, false).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&d), 0o700);
        Dir::open(&t.path().join("c/d"), true).unwrap();
        assert_eq!(mode(&t.path().join("c/d")), 0o700);
        assert!(Dir::open(&t.path().join("missing"), false).is_err());
        assert!(Dir::open(Path::new("rel/dir"), true).is_err());
    }

    #[test]
    fn renew_removes_without_following() {
        let t = tmp();
        let out = t.path().join("outside");
        std::fs::create_dir(&out).unwrap();
        std::fs::write(out.join("keep"), "k").unwrap();
        let d = t.path().join("renewed");
        std::fs::create_dir_all(d.join("x/y")).unwrap();
        std::fs::write(d.join("x/y/z"), "z").unwrap();
        symlink(&out, d.join("link")).unwrap();
        Dir::renew(&d).unwrap();
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        assert!(out.join("keep").is_file());
        // A symlink in place of the directory is removed, not followed.
        std::fs::remove_dir(&d).unwrap();
        symlink(&out, &d).unwrap();
        Dir::renew(&d).unwrap();
        assert!(out.join("keep").is_file());
        assert!(std::fs::symlink_metadata(&d).unwrap().is_dir());
    }

    #[test]
    fn uncommitted_file_is_removed() {
        let t = tmp();
        let dir = Dir::open(t.path(), false).unwrap();
        let (f, p) = dir.create(OsStr::new("x")).unwrap();
        drop(f);
        drop(p);
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 0);
        assert!(dir.create(OsStr::new("../x")).is_err());
    }
}
