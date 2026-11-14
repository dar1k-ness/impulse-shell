//! Minimal KConfig INI editing that keeps the file as it was (order, comments, unknown keys,
//! `[$Version]`, nested `[a][b]` groups, bytes that are not UTF-8) and only touches the entries
//! it sets; the original rewrote kdeglobals and the Konsole files through Python's configparser,
//! dropping comments.
//!
//! A file is changed the way KConfig syncs one (kconfig 6.31 `KConfig::sync`,
//! `KConfigIniBackend::writeConfig`, kconfiginibackendreader_p.h): a symlink is resolved to the
//! file it names, `<file>.lock` is taken as a `QLockFile`, the file is read from disk under that
//! lock, changed, and replaced by an atomic rename. A file that is there but cannot be read, or
//! whose lock cannot be had, is left as it is.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bytes, like KConfig's own parser: a config file need not be UTF-8.
#[derive(Clone, Debug)]
enum Line {
    Group(Vec<u8>),
    Entry { key: Vec<u8>, value: Vec<u8> },
    Other(Vec<u8>),
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    lines: Vec<Line>,
}

fn parse_line(l: &[u8]) -> Line {
    let t = l.trim_ascii();
    if t.len() >= 2 && t[0] == b'[' && t[t.len() - 1] == b']' {
        return Line::Group(t[1..t.len() - 1].to_vec());
    }
    if !matches!(t.first(), Some(b'#' | b';'))
        && let Some(eq) = l.iter().position(|&b| b == b'=')
        && !l[..eq].trim_ascii().is_empty()
    {
        return Line::Entry {
            key: l[..eq].trim_ascii().to_vec(),
            value: l[eq + 1..].trim_ascii().to_vec(),
        };
    }
    Line::Other(l.to_vec())
}

/// `group` itself or one of its `[group][sub]` children.
fn within(name: &[u8], group: &str) -> bool {
    name.strip_prefix(group.as_bytes())
        .is_some_and(|r| r.is_empty() || r.starts_with(b"]["))
}

impl Doc {
    pub fn parse(text: &str) -> Doc {
        Doc::from_bytes(text.as_bytes())
    }

    /// The lines as `str::lines` splits them (`\n` or `\r\n`), whatever their encoding.
    pub fn from_bytes(data: &[u8]) -> Doc {
        Doc {
            lines: data
                .split_inclusive(|&b| b == b'\n')
                .map(|l| {
                    let l = l.strip_suffix(b"\n").unwrap_or(l);
                    parse_line(l.strip_suffix(b"\r").unwrap_or(l))
                })
                .collect(),
        }
    }

    /// Opens `path` for a change: its lock is taken, then it is read. Fails, touching nothing,
    /// where KConfig gives the write up: the file is there but cannot be read or may not be
    /// written, it is a link to a file that is not there, or its lock cannot be had. A file
    /// that is not there yet is an empty document.
    pub fn edit(path: &Path) -> std::io::Result<Edit> {
        let place = Place::of(path)?;
        if place.dangling {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "a link to a file that is not there",
            ));
        }
        writable(&place.dest)?;
        let lock = Lock::take(&place.file)?;
        let disk = match std::fs::read(&place.dest) {
            Ok(data) => Some(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Edit {
            doc: Doc::from_bytes(disk.as_deref().unwrap_or_default()),
            dest: place.dest,
            disk,
            _lock: lock,
        })
    }

    /// (group, line index) for every line; entries before any header are in group "".
    fn grouped(&self) -> impl Iterator<Item = (&[u8], usize, &Line)> {
        let mut g: &[u8] = &[];
        self.lines.iter().enumerate().map(move |(i, l)| {
            if let Line::Group(n) = l {
                g = n;
            }
            (g, i, l)
        })
    }

    /// The value, when it is text (UTF-8).
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.grouped()
            .filter_map(|(g, _, l)| match l {
                Line::Entry { key: k, value } if g == group.as_bytes() && k == key.as_bytes() => {
                    Some(value)
                }
                _ => None,
            })
            .last()
            .and_then(|v| std::str::from_utf8(v).ok())
    }

    /// Every group whose name is text (UTF-8), first occurrence order.
    pub fn groups(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for (g, ..) in self.grouped() {
            if let Ok(g) = std::str::from_utf8(g)
                && !out.contains(&g)
            {
                out.push(g);
            }
        }
        out
    }

    pub fn has_group(&self, group: &str) -> bool {
        self.grouped().any(|(g, ..)| g == group.as_bytes())
    }

    /// The group's entries that are text (UTF-8), in file order (later duplicates win, like
    /// KConfig).
    pub fn entries(&self, group: &str) -> Vec<(&str, &str)> {
        let mut out: Vec<(&str, &str)> = Vec::new();
        for (g, _, l) in self.grouped() {
            if let Line::Entry { key, value } = l
                && g == group.as_bytes()
                && let (Ok(key), Ok(value)) = (std::str::from_utf8(key), std::str::from_utf8(value))
            {
                match out.iter_mut().find(|(k, _)| *k == key) {
                    Some(e) => e.1 = value,
                    None => out.push((key, value)),
                }
            }
        }
        out
    }

    /// Sets `key`; returns whether the value changed.
    pub fn set(&mut self, group: &str, key: &str, value: &str) -> bool {
        let mut hit = None;
        let mut last_in_group = None;
        for (g, i, l) in self.grouped() {
            if g != group.as_bytes() {
                continue;
            }
            match l {
                Line::Entry { key: k, .. } => {
                    last_in_group = Some(i);
                    if k == key.as_bytes() {
                        hit = Some(i);
                    }
                }
                Line::Group(_) => last_in_group = Some(i),
                Line::Other(_) => {}
            }
        }
        if let Some(i) = hit {
            if let Line::Entry { value: v, .. } = &mut self.lines[i] {
                if v == value.as_bytes() {
                    return false;
                }
                *v = value.as_bytes().to_vec();
            }
            return true;
        }
        let entry = Line::Entry {
            key: key.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
        };
        match last_in_group {
            Some(i) => self.lines.insert(i + 1, entry),
            None if group.is_empty() => self.lines.insert(0, entry),
            None => {
                if self
                    .lines
                    .last()
                    .is_some_and(|l| !matches!(l, Line::Other(s) if s.trim_ascii().is_empty()))
                {
                    self.lines.push(Line::Other(Vec::new()));
                }
                self.lines.push(Line::Group(group.as_bytes().to_vec()));
                self.lines.push(entry);
            }
        }
        true
    }

    /// Removes `key` from `group`; returns whether it was there.
    pub fn remove(&mut self, group: &str, key: &str) -> bool {
        let drop: Vec<usize> = self
            .grouped()
            .filter_map(|(g, i, l)| {
                matches!(l, Line::Entry { key: k, .. }
                    if g == group.as_bytes() && k == key.as_bytes())
                .then_some(i)
            })
            .collect();
        for &i in drop.iter().rev() {
            self.lines.remove(i);
        }
        !drop.is_empty()
    }

    /// Removes the entries of `group` and its subgroups (headers stay, so the groups keep their
    /// place).
    pub fn clear_group(&mut self, group: &str) {
        let mut inside = false;
        self.lines.retain(|l| match l {
            Line::Group(n) => {
                inside = within(n, group);
                true
            }
            Line::Entry { .. } => !inside,
            Line::Other(_) => true,
        });
    }

    /// The file's content: every line as it was read, or as it was set.
    pub fn bytes(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for l in &self.lines {
            match l {
                Line::Group(n) => {
                    s.push(b'[');
                    s.extend_from_slice(n);
                    s.push(b']');
                }
                Line::Entry { key, value } => {
                    s.extend_from_slice(key);
                    s.push(b'=');
                    s.extend_from_slice(value);
                }
                Line::Other(o) => s.extend_from_slice(o),
            }
            s.push(b'\n');
        }
        s
    }
}

/// A config file open for a change ([`Doc::edit`]): the document is the file as it is on disk
/// now, and nobody who honours KConfig's lock changes the file until this is saved or dropped.
pub struct Edit {
    doc: Doc,
    dest: PathBuf,
    disk: Option<Vec<u8>>,
    _lock: Lock,
}

impl std::ops::Deref for Edit {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.doc
    }
}

impl std::ops::DerefMut for Edit {
    fn deref_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }
}

impl Edit {
    /// There was no file.
    pub fn is_new(&self) -> bool {
        self.disk.is_none()
    }

    /// Writes the file unless nothing in it changed (or there is neither a file nor anything
    /// to put in one); returns whether it was written.
    pub fn save(self) -> std::io::Result<bool> {
        let data = self.doc.bytes();
        // Against the file as this writes it: one whose last line has no newline, or whose
        // lines end in `\r\n`, is not rewritten for that alone.
        if Doc::from_bytes(self.disk.as_deref().unwrap_or_default()).bytes() == data {
            return Ok(false);
        }
        replace(&self.dest, &data)?;
        Ok(true)
    }
}

/// Says that `path` was not written, and why; the file is as it was.
pub(super) fn left(path: &Path, why: &std::io::Error) {
    elog::eprintln!("kde recolour: {} left as it is: {why}", path.display());
}

/// Whether a write happened; one that failed is logged ([`left`]).
pub(super) fn written(path: &Path, result: std::io::Result<bool>) -> bool {
    result.unwrap_or_else(|e| {
        left(path, &e);
        false
    })
}

/// Replaces the whole file with `data` under its KConfig lock, for a file that is ours from the
/// first byte to the last; skips the write when the content is unchanged. Returns whether it
/// was written.
pub fn write_locked(path: &Path, data: &[u8]) -> std::io::Result<bool> {
    let place = Place::of(path)?;
    if std::fs::read(&place.dest).is_ok_and(|old| old == data) {
        return Ok(false);
    }
    writable(&place.dest)?;
    let _lock = Lock::take(&place.file)?;
    replace(&place.dest, data)?;
    Ok(true)
}

/// What KConfig makes of a config file's path: the lock goes next to `file`, the data into
/// `dest`.
struct Place {
    file: PathBuf,
    dest: PathBuf,
    /// `file` is a link to a file that is not there: KConfig cannot write such a config
    /// ("Couldn't write to config") and leaves the link as it is.
    dangling: bool,
}

/// `QFileInfo::isWritable` on a file that exists, which KConfig (`KConfig::sync`) and `QSaveFile`
/// both ask before they write: a file the user made read-only is not replaced, though a rename
/// over it would work.
fn writable(dest: &Path) -> std::io::Result<()> {
    let Ok(path) = std::ffi::CString::new(dest.as_os_str().as_bytes()) else {
        return Ok(());
    };
    // SAFETY: access(2) on a NUL-terminated path that outlives the call.
    if unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error() {
        e if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        e => Err(e),
    }
}

impl Place {
    /// `KConfigIniBackendFileDevice::setFilePath`: a file that exists is its canonical path, so
    /// a link in `~/.config` (dotfiles, stow, home-manager) names the file it points to and
    /// stays a link. One that does not is the canonical directory plus the name; if that name
    /// is a link to a file not there yet, `QSaveFile` still writes the file the link names
    /// (qsavefile.cpp `QSaveFile::open`, at most 128 links).
    fn of(path: &Path) -> std::io::Result<Place> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if let Ok(real) = std::fs::canonicalize(path) {
            return Ok(Place {
                file: real.clone(),
                dest: real,
                dangling: false,
            });
        }
        let file = match (
            path.parent().and_then(|d| std::fs::canonicalize(d).ok()),
            path.file_name(),
        ) {
            (Some(dir), Some(name)) => dir.join(name),
            _ => path.to_path_buf(),
        };
        let mut dest = file.clone();
        let mut links = 0;
        while let Ok(to) = std::fs::read_link(&dest) {
            links += 1;
            if links == 128 {
                dest = file.clone();
                break;
            }
            dest = dest.parent().unwrap_or(Path::new("/")).join(to);
        }
        Ok(Place {
            dangling: links > 0,
            file,
            dest,
        })
    }
}

/// `QSaveFile::commit`: the data goes to a new file next to `dest`, is synced to disk and
/// renamed over it, so `dest` is the old content or the new, never part of either. A file that
/// was there keeps its permissions, and until the rename the new one is the user's alone; a new
/// file gets what the umask leaves of 0666.
fn replace(dest: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(format!(".{}.new", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let old = std::fs::metadata(dest).map(|m| m.permissions()).ok();
    // A leftover of a process that had this pid would keep its own mode.
    let _ = std::fs::remove_file(&tmp);
    let write = || {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(if old.is_some() { 0o600 } else { 0o666 })
            .open(&tmp)?;
        f.write_all(data)?;
        if let Some(old) = old {
            f.set_permissions(old)?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, dest)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// How long KConfig waits for a file's lock before it gives the write up
/// (`KConfigIniBackend::lock`).
const LOCK_TIMEOUT: Duration = Duration::from_secs(45);

/// `QLockFile`'s stale time: past this age a lock nobody holds a `flock` on is nobody's.
const STALE_AFTER: Duration = Duration::from_secs(30);

/// Qt's `QLockFile` on `<file>.lock` (qlockfile.cpp, qlockfile_unix.cpp of Qt 6.12), which
/// KConfig holds while it rereads and writes `<file>`: the file is created with `O_EXCL`,
/// `flock`ed, and names its owner. Qt and this follow the same rules both ways: whoever finds
/// the file waits, and removes it only once it looks abandoned ([`apparently_stale`]) and
/// carries no `flock`. Dropping it unlocks as Qt does: unlink, then close.
struct Lock {
    path: PathBuf,
    file: std::fs::File,
}

impl Lock {
    fn take(file: &Path) -> std::io::Result<Lock> {
        Lock::take_until(file, &deadline(LOCK_TIMEOUT)?)
    }

    /// `QLockFile::tryLock(timeout)`, sleeping until the lock file is removed, its owner exits,
    /// it reaches the stale age or `deadline` becomes readable ([`fired`]), whichever comes
    /// first, where Qt retries on a timer.
    fn take_until(file: &Path, deadline: &OwnedFd) -> std::io::Result<Lock> {
        let mut lp = file.as_os_str().to_owned();
        lp.push(".lock");
        let lp = PathBuf::from(lp);
        let mut removal: Option<Option<OwnedFd>> = None;
        loop {
            if let Some(lock) = Lock::create(&lp)? {
                return Ok(lock);
            }
            let Some(watch) = &removal else {
                // Armed before the next attempt, so a release in between is not missed.
                removal = Some(watch_removal(&lp));
                continue;
            };
            let seen = Seen::of(&lp);
            if seen.stale && remove_stale(&lp) {
                continue;
            }
            if fired(deadline) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("{} is held by another program", lp.display()),
                ));
            }
            wait_release(&lp, &seen, watch.as_ref(), deadline);
        }
    }

    /// `QLockFilePrivate::tryLock_sys`: `None` when somebody else has the lock.
    fn create(lp: &Path) -> std::io::Result<Option<Lock>> {
        let mut file = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o666)
            .open(lp)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(None),
            Err(e) => return Err(e),
        };
        // Nobody may delete the file while we have it: a QLockFile that finds it past the stale
        // age tries this `flock` first and leaves a file it cannot lock alone.
        if !flock(&file) {
            return Ok(None);
        }
        if let Err(e) = file.write_all(&contents()) {
            let _ = std::fs::remove_file(lp);
            return Err(e);
        }
        // Someone who took the still unlocked file for a stale one may have removed it, and a
        // third may have made its own since: then the name is not our file.
        if !same_file(lp, &file) {
            return Ok(None);
        }
        Ok(Some(Lock {
            path: lp.to_path_buf(),
            file,
        }))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Only our own file: had someone removed it by hand, the name may be another's lock now.
        if same_file(&self.path, &self.file) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// `setNativeLocks`: false only when someone else holds the `flock`; a file system without
/// `flock` counts as locked, as it does for Qt.
fn flock(file: &std::fs::File) -> bool {
    loop {
        // SAFETY: flock(2) on a file this function's caller keeps open.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return true;
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => {}
            e => return e != Some(libc::EWOULDBLOCK),
        }
    }
}

/// The name `lp` is the open `file`.
fn same_file(lp: &Path, file: &std::fs::File) -> bool {
    match (std::fs::metadata(lp), file.metadata()) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// `QLockFilePrivate::lockFileContents`: pid, executable name, host name, machine id, boot id,
/// a line each. Qt takes a lock for abandoned when its pid runs an executable of another name
/// (the pid was reused), so the name is this process's executable, whatever it is.
fn contents() -> Vec<u8> {
    let pid = std::process::id();
    let mut c = pid.to_string().into_bytes();
    c.push(b'\n');
    for part in [
        process_name(pid.into()),
        host_name(),
        machine_id(),
        boot_id(),
    ] {
        c.extend_from_slice(&part);
        c.push(b'\n');
    }
    c
}

/// `QLockFilePrivate::processNameByPid`: the file name of the executable `pid` runs; a name no
/// executable has when there is none to read (the process is gone, or a zombie).
fn process_name(pid: i64) -> Vec<u8> {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else {
        return b"/ERROR/".to_vec();
    };
    let exe = exe.as_os_str().as_bytes();
    let exe = exe.strip_suffix(b" (deleted)").unwrap_or(exe);
    exe.rsplit(|&b| b == b'/').next().unwrap_or(exe).to_vec()
}

/// `QSysInfo::machineHostName`: uname(2)'s node name.
fn host_name() -> Vec<u8> {
    let mut name = std::fs::read("/proc/sys/kernel/hostname").unwrap_or_default();
    name.truncate(name.trim_ascii_end().len());
    name
}

/// The first `n` bytes of `file`, nothing when it cannot be read.
fn head(file: std::io::Result<std::fs::File>, n: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    match file.and_then(|f| f.take(n).read_to_end(&mut buf)) {
        Ok(_) => buf,
        Err(_) => Vec::new(),
    }
}

/// `QSysInfo::machineUniqueId`: D-Bus's machine id, 32 hex digits.
fn machine_id() -> Vec<u8> {
    let file = std::fs::File::open("/var/lib/dbus/machine-id").or_else(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            std::fs::File::open("/usr/local/var/lib/dbus/machine-id")
        } else {
            Err(e)
        }
    });
    head(file, 32)
}

/// `QSysInfo::bootUniqueId`: the kernel's boot UUID.
fn boot_id() -> Vec<u8> {
    let id = head(std::fs::File::open("/proc/sys/kernel/random/boot_id"), 36);
    if id.len() == 36 { id } else { Vec::new() }
}

/// What a lock file says of its owner (`QLockFilePrivate::getLockInfo_helper`).
struct Owner {
    pid: i64,
    app: Vec<u8>,
    host: Vec<u8>,
    host_id: Vec<u8>,
    boot_id: Vec<u8>,
}

impl Owner {
    fn read(lp: &Path) -> Option<Owner> {
        let data = std::fs::read(lp).ok()?;
        // Qt reads a line and drops its last byte, the newline.
        let mut lines = data
            .split_inclusive(|&b| b == b'\n')
            .map(|l| l[..l.len() - 1].to_vec());
        let pid = lines.next()?;
        let pid: i64 = std::str::from_utf8(&pid).ok()?.trim().parse().ok()?;
        if pid <= 0 {
            return None;
        }
        let mut line = || lines.next().unwrap_or_default();
        Some(Owner {
            pid,
            app: line(),
            host: line(),
            // Before Qt 5.10 only the three lines above were written.
            host_id: line(),
            boot_id: line(),
        })
    }

    /// The lock was taken on this machine (the first half of `isApparentlyStale`).
    fn same_host(&self) -> bool {
        let ours = machine_id();
        if !self.host_id.is_empty() && !ours.is_empty() {
            return self.host_id == ours;
        }
        self.host.is_empty() || self.host == host_name()
    }

    /// The owner's pid as a process id of this machine.
    fn local_pid(&self) -> Option<libc::pid_t> {
        libc::pid_t::try_from(self.pid).ok()
    }

    /// `QLockFilePrivate::isProcessRunning`: the pid exists and still runs the executable the
    /// lock names (an exited owner's pid may belong to another program by now).
    fn running(&self) -> bool {
        let Some(pid) = self.local_pid() else {
            return false;
        };
        // SAFETY: kill(2) with signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return false;
        }
        // Qt reads the name through a link of that name, if there is one.
        let app = Path::new(std::ffi::OsStr::from_bytes(&self.app));
        let app = std::fs::read_link(app).unwrap_or_else(|_| app.to_path_buf());
        app.file_name()
            .is_some_and(|n| n.as_bytes() == process_name(self.pid))
    }
}

/// Milliseconds since the lock file was last written, negative for a time yet to come.
fn age_ms(lp: &Path) -> Option<i128> {
    let written = std::fs::metadata(lp).and_then(|m| m.modified()).ok()?;
    Some(match std::time::SystemTime::now().duration_since(written) {
        Ok(age) => age.as_millis() as i128,
        Err(ahead) => -(ahead.duration().as_millis() as i128),
    })
}

/// `QLockFilePrivate::isApparentlyStale`: the lock is from before this boot, or its owner is no
/// longer running here, or it is more than [`STALE_AFTER`] away from now.
fn apparently_stale(lp: &Path) -> bool {
    Seen::of(lp).stale
}

/// One look at somebody else's lock file.
struct Seen {
    stale: bool,
    /// The owner, while it is a running process of this machine.
    owner: Option<libc::pid_t>,
    /// How long until the lock reaches the stale age, if it has not.
    stale_in: Option<Duration>,
}

impl Seen {
    fn of(lp: &Path) -> Seen {
        let owner = Owner::read(lp).filter(Owner::same_host);
        let rebooted = owner
            .as_ref()
            .is_some_and(|o| !o.boot_id.is_empty() && o.boot_id != boot_id());
        let running = owner.as_ref().map(|o| !rebooted && o.running());
        let stale_at = STALE_AFTER.as_millis() as i128;
        let age = age_ms(lp);
        let aged = age.is_some_and(|a| a.abs() > stale_at);
        Seen {
            stale: running == Some(false) || aged,
            owner: owner
                .filter(|_| running == Some(true))
                .and_then(|o| o.local_pid()),
            stale_in: age
                .filter(|_| !aged)
                .map(|a| Duration::from_millis((stale_at - a + 1) as u64)),
        }
    }
}

/// What `QLockFile::tryLock` does about a lock that looks stale: under `<lock>.rmlock`, so that
/// two processes do not remove it at once (the second would remove the first's new lock), look
/// again and remove it. False when it is not removed: its owner still holds the `flock`, or
/// someone else is removing it right now.
fn remove_stale(lp: &Path) -> bool {
    let mut rm = lp.as_os_str().to_owned();
    rm.push(".rmlock");
    let rm = PathBuf::from(rm);
    // `QLockFile::tryLock(0)`: one attempt, removing a stale `.rmlock` the same way.
    let rmlock = loop {
        match Lock::create(&rm) {
            Ok(Some(lock)) => break lock,
            Ok(None) if apparently_stale(&rm) && remove_stale(&rm) => {}
            _ => return false,
        }
    };
    let removed = apparently_stale(lp) && unlink_unlocked(lp);
    drop(rmlock);
    removed
}

/// `QLockFilePrivate::removeStaleLock`: a lock that carries a `flock` has a live owner, whatever
/// the file says, and stays.
fn unlink_unlocked(lp: &Path) -> bool {
    match std::fs::OpenOptions::new().write(true).open(lp) {
        Ok(file) => flock(&file) && std::fs::remove_file(lp).is_ok(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// A one-shot CLOCK_MONOTONIC timerfd that becomes readable `limit` from now.
pub(crate) fn deadline(limit: Duration) -> std::io::Result<OwnedFd> {
    use std::os::fd::FromRawFd;
    // SAFETY: timerfd_create(2) takes only a clock id and flags.
    let fd = unsafe {
        libc::timerfd_create(
            libc::CLOCK_MONOTONIC,
            libc::TFD_CLOEXEC | libc::TFD_NONBLOCK,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned by timerfd_create and nothing else owns it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // An all-zero it_value disarms the timer instead of firing it at once.
    let limit = limit.max(Duration::from_nanos(1));
    let spec = libc::itimerspec {
        it_interval: libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        it_value: libc::timespec {
            tv_sec: libc::time_t::try_from(limit.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: limit.subsec_nanos().into(),
        },
    };
    // SAFETY: a live timerfd and an itimerspec that outlives the call; no old value is asked for.
    if unsafe { libc::timerfd_settime(fd.as_raw_fd(), 0, &spec, std::ptr::null_mut()) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(fd)
}

/// Whether `fd` is readable (or hung up) right now: a [`deadline`] that has expired.
pub(crate) fn fired(fd: &OwnedFd) -> bool {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll(2) with a zero timeout on one fd the caller keeps open.
    (unsafe { libc::poll(&mut p, 1, 0) }) == 1
}

/// inotify on the lock's directory for a file going away (unlink or rename).
fn watch_removal(lp: &Path) -> Option<OwnedFd> {
    use std::os::fd::FromRawFd;
    let dir = std::ffi::CString::new(lp.parent()?.as_os_str().as_bytes()).ok()?;
    // SAFETY: inotify_init1(2) takes only flags.
    let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if fd < 0 {
        return None;
    }
    // SAFETY: `fd` was just opened by inotify_init1 and nothing else owns it.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mask = libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_ONLYDIR;
    // SAFETY: `fd` is a live inotify fd and `dir` a NUL-terminated path that outlives the call.
    let wd = unsafe { libc::inotify_add_watch(fd.as_raw_fd(), dir.as_ptr(), mask) };
    (wd >= 0).then_some(fd)
}

/// One blocking wait for the lock `lp`, last `seen`, to change hands: its removal, its owner's
/// exit (pidfd), the lock reaching the stale age, or `deadline` firing. A lock that is stale
/// already and was not removed is waited on for its removal or its owner's exit only.
fn wait_release(lp: &Path, seen: &Seen, removal: Option<&OwnedFd>, deadline: &OwnedFd) {
    let mut owner = None;
    if let Some(pid) = seen.owner {
        match pidfd(pid) {
            Ok(fd) if !fired(&fd) => owner = Some(fd),
            // Exited since it was seen running: the caller looks again.
            Ok(_) | Err(Ok(Gone)) => return,
            Err(Err(_)) => {}
        }
    }
    let mut fds: Vec<libc::pollfd> = [removal, owner.as_ref(), Some(deadline)]
        .into_iter()
        .flatten()
        .map(|f| libc::pollfd {
            fd: f.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let ms = match seen.stale_in {
        Some(d) if !seen.stale => d.as_millis().clamp(1, i32::MAX as u128) as i32,
        _ => -1,
    };
    loop {
        // SAFETY: poll(2) on fds this function keeps open for the call.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
        let Some(f) = removal else { return };
        // Other files leave the directory too, the `.rmlock` of our own attempt to remove a
        // stale lock among them: with the lock still there, they are nothing to look again for.
        let other = fds[1..].iter().any(|p| p.revents != 0);
        if lock_went(f, lp) || ready <= 0 || other || !seen.stale {
            return;
        }
    }
}

/// Empties the inotify fd of [`watch_removal`]; true when one of its events is about the lock
/// file `lp` itself, or events were lost.
fn lock_went(watch: &OwnedFd, lp: &Path) -> bool {
    let name = lp.file_name().map(|n| n.as_bytes());
    let head = std::mem::size_of::<libc::inotify_event>();
    let mut went = false;
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: reading a non-blocking inotify fd into a local buffer.
        let n = unsafe { libc::read(watch.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        let Ok(n @ 1..) = usize::try_from(n) else {
            return went;
        };
        let mut events = &buf[..n];
        while events.len() >= head {
            // SAFETY: the kernel wrote a whole inotify_event here; read unaligned out of bytes.
            let e: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(events.as_ptr().cast()) };
            let end = (head + e.len as usize).min(events.len());
            let file = events[head..end].split(|b| *b == 0).next();
            went |= file == name || e.mask & libc::IN_Q_OVERFLOW != 0;
            events = &events[end..];
        }
    }
}

/// pidfd_open(2) failed because the process is gone: ESRCH once it is reaped, EINVAL while it is
/// being reaped (its pid no longer leads a thread group).
struct Gone;

fn pidfd(pid: libc::pid_t) -> Result<OwnedFd, Result<Gone, std::io::Error>> {
    use std::os::fd::FromRawFd;
    // SAFETY: pidfd_open(2); the fd is owned right after it is created.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd >= 0 {
        // SAFETY: `fd` was just returned by pidfd_open and nothing else owns it.
        return Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) });
    }
    let e = std::io::Error::last_os_error();
    Err(match e.raw_os_error() {
        Some(libc::ESRCH | libc::EINVAL) => Ok(Gone),
        _ => Err(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_are_not_utf8_stay_as_they_are() {
        let file = b"# caf\xe9\n[General]\nName=caf\xe9\nColorScheme=Old\n[B\xfcro]\nKey=1\n";
        let mut d = Doc::from_bytes(file);
        assert_eq!(d.bytes(), file);
        assert_eq!(d.get("General", "Name"), None, "not text");
        assert_eq!(d.get("General", "ColorScheme"), Some("Old"));
        assert!(d.set("General", "ColorScheme", "New"));
        assert!(d.set("General", "Added", "1"));
        assert_eq!(
            d.bytes(),
            b"# caf\xe9\n[General]\nName=caf\xe9\nColorScheme=New\nAdded=1\n[B\xfcro]\nKey=1\n"
        );
        assert_eq!(d.groups(), ["", "General"]);
        assert_eq!(
            d.entries("General"),
            [("ColorScheme", "New"), ("Added", "1")]
        );
        // The entry after the header that is not text is that group's, not `[General]`'s.
        assert_eq!(d.get("General", "Key"), None);
    }

    #[test]
    fn clear_group_removes_the_entries_of_the_group_and_its_subgroups() {
        let mut d = Doc::parse("[A]\nx=1\n# kept\n[A][B]\ny=2\n[AB]\nz=3\n");
        d.clear_group("A");
        assert_eq!(d.bytes(), b"[A]\n# kept\n[A][B]\n[AB]\nz=3\n");
    }

    #[test]
    fn edit_of_an_unreadable_file_fails_and_leaves_it() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        std::fs::write(&file, "[KDE]\nSingleClick=false\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&file).is_ok() {
            return testenv::unavailable("a user that file modes apply to (not root)");
        }
        let e = Doc::edit(&file)
            .err()
            .expect("an unreadable file is not an empty one");
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!d.path().join("kdeglobals.lock").exists(), "lock released");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"[KDE]\nSingleClick=false\n");
    }

    #[test]
    fn edit_of_a_missing_file_is_new_and_writes_nothing_until_something_is_set() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("sub/konsolerc");
        let e = Doc::edit(&file).unwrap();
        assert!(e.is_new());
        assert!(
            d.path().join("sub/konsolerc.lock").exists(),
            "held while open"
        );
        assert!(!e.save().unwrap());
        assert!(!file.exists());
        let mut e = Doc::edit(&file).unwrap();
        e.set("Desktop Entry", "DefaultProfile", "a.profile");
        assert!(e.save().unwrap());
        assert_eq!(
            std::fs::read(&file).unwrap(),
            b"[Desktop Entry]\nDefaultProfile=a.profile\n"
        );
        assert!(
            !d.path().join("sub/konsolerc.lock").exists(),
            "lock released"
        );
        let e = Doc::edit(&file).unwrap();
        assert!(!e.is_new());
        assert!(!e.save().unwrap(), "unchanged");
    }

    #[test]
    fn a_link_is_written_through_and_locked_at_its_target() {
        let d = tempfile::tempdir().unwrap();
        let (dot, config) = (d.path().join("dotfiles"), d.path().join("config"));
        std::fs::create_dir_all(&dot).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(dot.join("kdeglobals"), "[KDE]\nSingleClick=false\n").unwrap();
        let link = config.join("kdeglobals");
        std::os::unix::fs::symlink("../dotfiles/kdeglobals", &link).unwrap();
        let mut e = Doc::edit(&link).unwrap();
        assert_eq!(e.get("KDE", "SingleClick"), Some("false"));
        assert!(
            dot.join("kdeglobals.lock").exists(),
            "KConfig locks the real file"
        );
        assert!(!config.join("kdeglobals.lock").exists());
        e.set("General", "ColorScheme", "X");
        assert!(e.save().unwrap());
        assert!(write_locked(&link, b"[General]\nColorScheme=Y\n").unwrap());
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            Path::new("../dotfiles/kdeglobals")
        );
        assert_eq!(
            std::fs::read(dot.join("kdeglobals")).unwrap(),
            b"[General]\nColorScheme=Y\n"
        );
        let left: Vec<_> = std::fs::read_dir(&config)
            .unwrap()
            .chain(std::fs::read_dir(&dot).unwrap())
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            left,
            ["kdeglobals", "kdeglobals"],
            "no lock or temporary file left"
        );
    }

    /// A file of ours is written as `QSaveFile` (and Python's `open(.., "w")` of the original)
    /// writes one: through the link.
    #[test]
    fn a_link_to_a_file_not_there_yet_creates_that_file() {
        let d = tempfile::tempdir().unwrap();
        let link = d.path().join("MaterialYouDark.colors");
        std::os::unix::fs::symlink("real", &link).unwrap();
        assert!(write_locked(&link, b"[A]\nb=c\n").unwrap());
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("real"));
        assert_eq!(std::fs::read(d.path().join("real")).unwrap(), b"[A]\nb=c\n");
    }

    /// KConfig 6.31 on a config that is a link to nothing: "Couldn't write to config", and the
    /// link stays.
    #[test]
    fn edit_of_a_link_to_a_file_not_there_fails_and_leaves_the_link() {
        let d = tempfile::tempdir().unwrap();
        let link = d.path().join("kdeglobals");
        std::os::unix::fs::symlink("real", &link).unwrap();
        let e = Doc::edit(&link).err().expect("KConfig does not write it");
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("real"));
        let left: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["kdeglobals"]);
    }

    /// KConfig and `QSaveFile` do not replace a file the user may not write.
    #[test]
    fn a_read_only_file_is_not_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        std::fs::write(&file, "[KDE]\nSingleClick=false\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
        if writable(&file).is_ok() {
            return testenv::unavailable("a user that file modes apply to (not root)");
        }
        let e = Doc::edit(&file).err().expect("read-only");
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        let e = write_locked(&file, b"[A]\nb=c\n").expect_err("read-only");
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(!write_locked(&file, b"[KDE]\nSingleClick=false\n").unwrap());
        assert_eq!(std::fs::read(&file).unwrap(), b"[KDE]\nSingleClick=false\n");
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o444
        );
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_file_is_not_rewritten_for_its_line_endings_alone() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        for same in ["[KDE]\nSingleClick=false", "[KDE]\r\nSingleClick=false\r\n"] {
            std::fs::write(&file, same).unwrap();
            let mut e = Doc::edit(&file).unwrap();
            assert!(!e.set("KDE", "SingleClick", "false"));
            assert!(!e.save().unwrap());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), same);
        }
    }

    #[test]
    fn a_new_file_is_never_wider_open_than_the_one_it_replaces() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        // A leftover of an earlier process with our pid, open to all.
        let mut tmp = file.clone().into_os_string();
        tmp.push(format!(".{}.new", std::process::id()));
        std::fs::write(&tmp, "old").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(write_locked(&file, b"[A]\nb=c\n").unwrap());
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!Path::new(&tmp).exists());
    }

    #[test]
    fn a_replaced_file_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        std::fs::write(&file, "").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(write_locked(&file, b"[A]\nb=c\n").unwrap());
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    use std::time::Instant;

    fn voluntary_switches() -> u64 {
        std::fs::read_to_string("/proc/thread-self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    fn set_age(lp: &Path, age: Duration) {
        std::fs::File::options()
            .append(true)
            .open(lp)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - age)
            .unwrap();
    }

    /// The lock of a live process other than us, as Qt writes it but for the lines `with`
    /// replaces (1: executable, 2: host, 3: machine id, 4: boot id). Written just now, it is
    /// 30 s from stale; nobody holds a `flock` on it.
    /// A lock's owner, ended when a test is over however it ends: left running, it would keep
    /// the test run's output open.
    struct Sleeper(std::process::Child);

    impl std::ops::Deref for Sleeper {
        type Target = std::process::Child;
        fn deref(&self) -> &std::process::Child {
            &self.0
        }
    }

    impl std::ops::DerefMut for Sleeper {
        fn deref_mut(&mut self) -> &mut std::process::Child {
            &mut self.0
        }
    }

    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn held_with(dir: &Path, with: &[(usize, &[u8])]) -> (PathBuf, Sleeper) {
        let owner = Sleeper(
            std::process::Command::new("sleep")
                .arg("infinity")
                .spawn()
                .unwrap(),
        );
        // `spawn` returns once the child's exec is under way; for a moment after that the
        // child still shows the executable it was forked from, ours.
        let ours = process_name(std::process::id().into());
        let mut name = process_name(owner.id().into());
        while name == ours {
            std::thread::yield_now();
            name = process_name(owner.id().into());
        }
        let mut lines = [
            owner.id().to_string().into_bytes(),
            name,
            host_name(),
            machine_id(),
            boot_id(),
        ];
        for (i, line) in with {
            lines[*i] = line.to_vec();
        }
        let mut data = lines.join(&b'\n');
        data.push(b'\n');
        std::fs::write(dir.join("kdeglobals.lock"), data).unwrap();
        (dir.join("kdeglobals"), owner)
    }

    fn held(dir: &Path) -> (PathBuf, Sleeper) {
        held_with(dir, &[])
    }

    /// A deadline that never fires while its writer lives.
    fn never() -> (OwnedFd, std::io::PipeWriter) {
        let (r, w) = std::io::pipe().unwrap();
        (r.into(), w)
    }

    /// inotify on the lock file for reads ending (IN_CLOSE_NOWRITE). The taker arms its removal
    /// watch before it first reads the owner out of the file, so after one read a removal or
    /// the owner's exit is not missed. IN_OPEN is watched too because inotify merges an event
    /// identical to the unread one before it, so back-to-back closes alone could count as one.
    struct Reads(OwnedFd);

    impl Reads {
        fn watch(lp: &Path) -> Reads {
            use std::os::fd::FromRawFd;
            let path = std::ffi::CString::new(lp.as_os_str().as_bytes()).unwrap();
            // SAFETY: inotify_init1(2) takes only flags.
            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
            assert!(fd >= 0, "{}", std::io::Error::last_os_error());
            // SAFETY: `fd` was just opened by inotify_init1 and nothing else owns it.
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            // SAFETY: `fd` is a live inotify fd and `path` a NUL-terminated path that outlives
            // the call.
            let wd = unsafe {
                libc::inotify_add_watch(
                    fd.as_raw_fd(),
                    path.as_ptr(),
                    libc::IN_OPEN | libc::IN_CLOSE_NOWRITE,
                )
            };
            assert!(wd >= 0, "{}", std::io::Error::last_os_error());
            Reads(fd)
        }

        /// Blocks until the file has been read `n` times since [`Reads::watch`].
        fn wait(&self, n: usize) {
            let head = std::mem::size_of::<libc::inotify_event>();
            let mut buf = [0u8; 4096];
            let mut seen = 0;
            while seen < n {
                // SAFETY: a blocking read of the inotify fd into a local buffer.
                let got =
                    unsafe { libc::read(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                assert!(got > 0, "{}", std::io::Error::last_os_error());
                let mut at = 0;
                while at + head <= got as usize {
                    // SAFETY: the kernel wrote a whole inotify_event at `at`; read unaligned.
                    let ev = unsafe {
                        std::ptr::read_unaligned(buf[at..].as_ptr().cast::<libc::inotify_event>())
                    };
                    if ev.mask & libc::IN_CLOSE_NOWRITE != 0 {
                        seen += 1;
                    }
                    at += head + ev.len as usize;
                }
            }
        }
    }

    struct Took {
        taken: bool,
        returned: Instant,
        switches: u64,
        /// Processor time the call took.
        busy: Duration,
        deadline: OwnedFd,
    }

    fn thread_cpu_time() -> Duration {
        let mut t = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime(2) into a local timespec.
        let r = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
        assert_eq!(r, 0);
        Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
    }

    /// Takes the lock on another thread, recording when the call returned; the lock is released
    /// there.
    fn take_in_thread(file: PathBuf, deadline: OwnedFd) -> std::thread::JoinHandle<Took> {
        std::thread::spawn(move || {
            let cs = voluntary_switches();
            let cpu = thread_cpu_time();
            let l = Lock::take_until(&file, &deadline);
            let returned = Instant::now();
            Took {
                taken: l.is_ok(),
                returned,
                switches: voluntary_switches() - cs,
                busy: thread_cpu_time() - cpu,
                deadline,
            }
        })
    }

    fn end(owner: Sleeper) {
        drop(owner);
    }

    #[test]
    fn the_lock_file_is_what_qt_writes() {
        let d = tempfile::tempdir().unwrap();
        let lock = Lock::take(&d.path().join("kdeglobals")).unwrap();
        let lp = d.path().join("kdeglobals.lock");
        let exe = std::env::current_exe().unwrap();
        let text = std::fs::read_to_string(&lp).unwrap();
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 6, "five lines, each ended: {text:?}");
        assert_eq!(lines[0], std::process::id().to_string());
        assert_eq!(lines[1], exe.file_name().unwrap().to_str().unwrap());
        assert_eq!(
            lines[2],
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .unwrap()
                .trim()
        );
        assert_eq!(
            lines[4],
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
        );
        let other = std::fs::File::open(&lp).unwrap();
        assert!(!flock(&other), "the owner holds a flock on its lock");
        assert!(!Seen::of(&lp).stale, "our own lock is a live one to us");
        drop(lock);
        assert!(!lp.exists(), "released by removing the file");
    }

    #[test]
    fn a_lock_that_became_someone_elses_is_not_removed_on_release() {
        let d = tempfile::tempdir().unwrap();
        let lock = Lock::take(&d.path().join("kdeglobals")).unwrap();
        let lp = d.path().join("kdeglobals.lock");
        std::fs::remove_file(&lp).unwrap();
        std::fs::write(&lp, "1\nkwrite\nhost\n").unwrap();
        drop(lock);
        assert_eq!(std::fs::read(&lp).unwrap(), b"1\nkwrite\nhost\n");
    }

    #[test]
    fn sleeps_until_the_lock_file_is_removed() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held(d.path());
        let reads = Reads::watch(&d.path().join("kdeglobals.lock"));
        let (deadline, _alive) = never();
        let h = take_in_thread(file.clone(), deadline);
        reads.wait(1);
        let removing = Instant::now();
        std::fs::remove_file(d.path().join("kdeglobals.lock")).unwrap();
        let t = h.join().unwrap();
        end(owner);
        assert!(t.taken);
        assert!(
            t.returned > removing,
            "taken while the owner's lock was still there"
        );
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
        assert!(
            !d.path().join("kdeglobals.lock").exists(),
            "our lock is released on drop"
        );
    }

    #[test]
    fn takes_over_as_soon_as_the_owner_exits() {
        let d = tempfile::tempdir().unwrap();
        let (file, mut owner) = held(d.path());
        let reads = Reads::watch(&d.path().join("kdeglobals.lock"));
        let (deadline, _alive) = never();
        let h = take_in_thread(file, deadline);
        reads.wait(1);
        let killing = Instant::now();
        owner.kill().unwrap();
        owner.wait().unwrap();
        let t = h.join().unwrap();
        assert!(t.taken);
        assert!(t.returned > killing, "taken while the owner was alive");
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
        assert!(
            !d.path().join("kdeglobals.lock.rmlock").exists(),
            "the lock for removing a stale lock is released"
        );
    }

    #[test]
    fn takes_over_from_an_exited_owner_nobody_reaped() {
        let d = tempfile::tempdir().unwrap();
        let (file, mut owner) = held(d.path());
        owner.kill().unwrap();
        // SAFETY: siginfo_t is plain C data, valid all-zero.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(
            // SAFETY: waitid(2) on our own child into a live siginfo_t; WNOWAIT leaves it a
            // zombie for `owner.wait()` below.
            unsafe {
                libc::waitid(
                    libc::P_PID,
                    owner.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            },
            0
        );
        let (deadline, _alive) = never();
        let t = take_in_thread(file, deadline).join().unwrap();
        owner.wait().unwrap();
        assert!(t.taken, "a zombie owner holds nothing");
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
    }

    /// Qt's rule against a reused pid: the process is there, but it is another program.
    #[test]
    fn takes_over_when_the_pid_runs_another_program() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held_with(d.path(), &[(1, b"kwrite")]);
        let (deadline, _alive) = never();
        let t = take_in_thread(file, deadline).join().unwrap();
        end(owner);
        assert!(t.taken);
    }

    #[test]
    fn takes_over_a_lock_from_before_this_boot() {
        let d = tempfile::tempdir().unwrap();
        let before = b"00000000-0000-0000-0000-000000000000";
        let (file, owner) = held_with(d.path(), &[(4, before)]);
        let (deadline, _alive) = never();
        let t = take_in_thread(file, deadline).join().unwrap();
        end(owner);
        assert!(t.taken);
    }

    /// Without a `flock` on it (a file system that has none), a lock 30 s old is nobody's,
    /// whoever wrote it. No event marks a file reaching an age, so the wait ends on the clock,
    /// once, at that moment.
    #[test]
    fn takes_over_an_unlocked_lock_when_it_turns_stale() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held(d.path());
        let left = Duration::from_millis(300);
        set_age(&d.path().join("kdeglobals.lock"), STALE_AFTER - left);
        let (deadline, _alive) = never();
        let started = Instant::now();
        let t = take_in_thread(file, deadline).join().unwrap();
        // The owner (`sleep infinity`) lives and nobody removed its lock: only the lock's age
        // ended the wait.
        end(owner);
        assert!(t.taken);
        assert!(
            t.returned - started >= left - Duration::from_millis(50),
            "taken {:?} into a lock with {left:?} left",
            t.returned - started
        );
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
    }

    /// A process of another machine cannot be looked up here: only the lock's age counts.
    #[test]
    fn waits_for_the_age_of_another_machines_lock() {
        let d = tempfile::tempdir().unwrap();
        let elsewhere: [(usize, &[u8]); 2] =
            [(2, b"elsewhere"), (3, b"0123456789abcdef0123456789abcdef")];
        let (file, owner) = held_with(d.path(), &elsewhere);
        // Here its pid is nobody's.
        end(owner);
        let left = Duration::from_millis(300);
        set_age(&d.path().join("kdeglobals.lock"), STALE_AFTER - left);
        let (deadline, _alive) = never();
        let started = Instant::now();
        let t = take_in_thread(file, deadline).join().unwrap();
        assert!(t.taken);
        assert!(
            t.returned - started >= left - Duration::from_millis(50),
            "taken {:?} into a lock with {left:?} left",
            t.returned - started
        );
    }

    /// Qt's rule for a lock past the stale age: whoever can `flock` it removes it, and a live
    /// owner holds that `flock`.
    #[test]
    fn never_takes_a_flocked_lock_however_old() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held(d.path());
        let lp = d.path().join("kdeglobals.lock");
        let owners = std::fs::File::open(&lp).unwrap();
        assert!(flock(&owners));
        set_age(&lp, Duration::from_secs(3600));
        let before = std::fs::read(&lp).unwrap();
        let t = take_in_thread(file.clone(), deadline(Duration::from_millis(500)).unwrap())
            .join()
            .unwrap();
        assert!(!t.taken);
        assert!(fired(&t.deadline), "gave up before the deadline");
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
        // Each attempt to remove it makes and removes a `.rmlock` next to it: not a reason to
        // look again.
        assert!(
            t.busy < Duration::from_millis(100),
            "{:?} of processor time in a 500 ms wait",
            t.busy
        );
        assert_eq!(
            std::fs::read(&lp).unwrap(),
            before,
            "the owner's lock stays"
        );
        assert!(!d.path().join("kdeglobals.lock.rmlock").exists());
        // Its owner gone (the flock released), it is stale and taken at once.
        drop(owners);
        end(owner);
        let (deadline, _alive) = never();
        assert!(take_in_thread(file, deadline).join().unwrap().taken);
    }

    #[test]
    fn gives_up_on_a_live_lock_at_the_deadline() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held(d.path());
        let t = take_in_thread(file, deadline(Duration::from_millis(500)).unwrap())
            .join()
            .unwrap();
        end(owner);
        // Not taken, and returned once the 500 ms timer expired while the owner still lived: the
        // deadline ended the wait, and nothing woke it in between.
        assert!(!t.taken);
        assert!(fired(&t.deadline), "gave up before the deadline");
        assert!(t.switches < 10, "{} wakeups while waiting", t.switches);
        assert!(
            d.path().join("kdeglobals.lock").exists(),
            "a live owner's lock stays"
        );
    }

    /// Nothing is written without the lock: KConfig gives the write up too.
    #[test]
    fn a_file_whose_lock_cannot_be_had_is_not_written() {
        let d = tempfile::tempdir().unwrap();
        let (file, owner) = held(d.path());
        std::fs::write(&file, "[KDE]\nSingleClick=false\n").unwrap();
        let e = Lock::take_until(&file, &deadline(Duration::from_millis(100)).unwrap())
            .err()
            .expect("held");
        end(owner);
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    }

    /// A limit of zero is over at once: the timer is armed for 1 ns, not disarmed by an
    /// all-zero value, which would leave the caller waiting for good.
    #[test]
    fn a_deadline_of_zero_fires() {
        let d = deadline(Duration::ZERO).unwrap();
        let mut p = libc::pollfd {
            fd: d.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll(2) on one fd that outlives the call; no limit, the timer is the event.
        assert_eq!(unsafe { libc::poll(&mut p, 1, -1) }, 1);
        assert!(fired(&d));
    }

    #[test]
    fn deadline_fires_after_its_limit() {
        let d = deadline(Duration::from_secs(3600)).unwrap();
        assert!(!fired(&d));
        let mut now = libc::itimerspec {
            it_interval: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        // SAFETY: timerfd_gettime(2) on a live timerfd into a local itimerspec.
        assert_eq!(unsafe { libc::timerfd_gettime(d.as_raw_fd(), &mut now) }, 0);
        assert!(
            (3500..=3600).contains(&now.it_value.tv_sec),
            "{:?}",
            now.it_value.tv_sec
        );
        assert_eq!(now.it_interval.tv_sec, 0, "one-shot");
    }
}

/// Our lock against a real `QLockFile` (Qt 6.12, through the PySide6 of ri-testenv), both ways.
#[cfg(test)]
mod qt_tests {
    use super::*;

    /// tools/testenv/Containerfile installs PySide6 here.
    const PYTHON: &str = "/opt/qv4-venv/bin/python";

    fn set_age(lp: &Path, age: Duration) {
        std::fs::File::options()
            .append(true)
            .open(lp)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - age)
            .unwrap();
    }

    /// `QLockFile(argv[1]).tryLock(100 ms)`, and what Qt says of the owner when it is refused.
    const QT_TRY: &str = r#"
import sys
from PySide6.QtCore import QLockFile
lock = QLockFile(sys.argv[1])
if lock.tryLock(100):
    print("locked")
else:
    print("refused", lock.error().name, *lock.getLockInfo())
"#;

    fn qt_try(lp: &Path) -> String {
        let out = std::process::Command::new(PYTHON)
            .args(["-c", QT_TRY])
            .arg(lp)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// Qt takes our lock for a live one, by its own rules: the pid runs the executable the lock
    /// names, and past the stale age the lock still carries our `flock`.
    #[test]
    fn a_real_qlockfile_waits_for_our_lock() {
        if !Path::new(PYTHON).exists() {
            return testenv::unavailable(format_args!("{PYTHON} (PySide6)"));
        }
        let d = tempfile::tempdir().unwrap();
        let lp = d.path().join("kdeglobals.lock");
        let lock = Lock::take(&d.path().join("kdeglobals"));
        let exe = std::env::current_exe().unwrap();
        let refused = format!(
            "refused LockFailedError {} {} {}",
            std::process::id(),
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .unwrap()
                .trim(),
            exe.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(qt_try(&lp), refused);
        set_age(&lp, Duration::from_secs(3600));
        assert_eq!(qt_try(&lp), refused, "an hour old, and still held");
        drop(lock);
        assert_eq!(qt_try(&lp), "locked");
    }

    /// A real `QLockFile` held on argv[1] until a line arrives on stdin.
    const QT_HOLD: &str = r#"
import sys
from PySide6.QtCore import QLockFile
lock = QLockFile(sys.argv[1])
assert lock.tryLock(0), lock.error()
print("locked", flush=True)
sys.stdin.readline()
lock.unlock()
print("unlocked", flush=True)
"#;

    /// But for the pid and the executable's name, our lock file is the one this Qt writes here.
    #[test]
    fn our_lock_file_has_the_lines_of_a_real_qlockfile() {
        use std::io::BufRead;
        if !Path::new(PYTHON).exists() {
            return testenv::unavailable(format_args!("{PYTHON} (PySide6)"));
        }
        let d = tempfile::tempdir().unwrap();
        let mut qt = std::process::Command::new(PYTHON)
            .args(["-c", QT_HOLD])
            .arg(d.path().join("qt.lock"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = std::io::BufReader::new(qt.stdout.take().unwrap()).lines();
        assert_eq!(said.next().unwrap().unwrap(), "locked");
        let qts = std::fs::read_to_string(d.path().join("qt.lock")).unwrap();
        let lock = Lock::take(&d.path().join("kdeglobals"));
        let ours = std::fs::read_to_string(d.path().join("kdeglobals.lock")).unwrap();
        drop(lock);
        let (qts, ours): (Vec<&str>, Vec<&str>) =
            (qts.split('\n').collect(), ours.split('\n').collect());
        assert_eq!(qts[0], qt.id().to_string());
        assert_eq!(ours[0], std::process::id().to_string());
        assert_eq!(ours[2..], qts[2..], "host name, machine id, boot id");
        qt.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert!(qt.wait().unwrap().success());
    }

    /// We take Qt's lock for a live one for as long as Qt holds it, however old the file is,
    /// and have ours once Qt unlocks.
    #[test]
    fn we_wait_for_a_real_qlockfile() {
        use std::io::BufRead;
        if !Path::new(PYTHON).exists() {
            return testenv::unavailable(format_args!("{PYTHON} (PySide6)"));
        }
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("kdeglobals");
        let lp = d.path().join("kdeglobals.lock");
        let mut qt = std::process::Command::new(PYTHON)
            .args(["-c", QT_HOLD])
            .arg(&lp)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = std::io::BufReader::new(qt.stdout.take().unwrap()).lines();
        assert_eq!(said.next().unwrap().unwrap(), "locked");
        let qts = std::fs::read(&lp).unwrap();
        for age in [Duration::ZERO, Duration::from_secs(3600)] {
            set_age(&lp, age);
            let ours = Lock::take_until(&file, &deadline(Duration::from_millis(300)).unwrap());
            assert_eq!(
                std::fs::read(&lp).unwrap(),
                qts,
                "the lock Qt holds, {age:?} old, was taken over"
            );
            drop(ours);
            assert_eq!(std::fs::read(&lp).unwrap(), qts, "and removed, {age:?} old");
        }
        qt.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert_eq!(said.next().unwrap().unwrap(), "unlocked");
        let ours = Lock::take_until(&file, &deadline(Duration::from_secs(5)).unwrap());
        let pid = std::fs::read_to_string(&lp).unwrap();
        assert_eq!(pid.lines().next(), Some(&*std::process::id().to_string()));
        drop(ours);
        assert!(qt.wait().unwrap().success());
    }
}
