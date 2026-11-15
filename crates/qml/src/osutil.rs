//! The PATH search and /proc reads the native ports of shell commands share (`which`,
//! `command -v`, execvp, `pidof`, `pkill`, `pgrep`).

use std::ffi::{CString, OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// `access(p, X_OK)`: what execve's permission check and bash's `executable_file` ask.
pub fn access_x(p: &Path) -> bool {
    CString::new(p.as_os_str().as_bytes())
        // SAFETY: `c` is a NUL-terminated path, alive for the call.
        .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0)
}

/// An executable regular file (what execvp, `which` and `command -v` accept).
pub fn is_executable(p: &Path) -> bool {
    p.is_file() && access_x(p)
}

/// `DIR/NAME` for every entry of `path`, built as bash, which and execvp build them: an
/// empty entry is `.`, a trailing `/` is kept (`/usr/bin//NAME`).
pub fn candidates<'a>(name: &'a str, path: &'a OsStr) -> impl Iterator<Item = PathBuf> + 'a {
    path.as_bytes().split(|&b| b == b':').map(move |dir| {
        let dir = if dir.is_empty() { b"." as &[u8] } else { dir };
        let mut f = Vec::with_capacity(dir.len() + 1 + name.len());
        f.extend_from_slice(dir);
        f.push(b'/');
        f.extend_from_slice(name.as_bytes());
        PathBuf::from(OsString::from_vec(f))
    })
}

/// The first executable `DIR/NAME` of `path`.
pub fn find_executable(name: &str, path: &OsStr) -> Option<PathBuf> {
    candidates(name, path).find(|p| is_executable(p))
}

/// `command -v NAME` succeeds for a plain program on this process's PATH.
pub fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| find_executable(name, &p).is_some())
}

/// bash's PATH when the variable is unset.
const DEFAULT_PATH: &str = "/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin:.";

/// bash's `command -v NAME` for a plain NAME that is no builtin, alias, function or keyword
/// (`find_user_command`): the first executable non-directory `DIR/NAME` of $PATH, else the
/// first non-directory one. An empty DIR is `.`; the path is printed as built.
pub fn command_v(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_else(|| DEFAULT_PATH.into());
    let mut lose = None;
    for f in candidates(name, &path) {
        let Ok(m) = std::fs::metadata(&f) else {
            continue;
        };
        if m.is_dir() {
            continue;
        }
        if access_x(&f) {
            return Some(f);
        }
        lose.get_or_insert(f);
    }
    lose
}

/// GNU `which NAME` with `path` as PATH: the first executable `DIR/NAME`, made absolute
/// with `.`, `..` and repeated `/` resolved.
pub fn which(name: &str, path: &OsStr) -> Option<PathBuf> {
    let p = find_executable(name, path)?;
    let mut abs = PathBuf::new();
    for c in std::env::current_dir()
        .unwrap_or_default()
        .join(p)
        .components()
    {
        match c {
            std::path::Component::ParentDir => {
                abs.pop();
            }
            c => abs.push(c),
        }
    }
    Some(abs)
}

/// Every PID in /proc, ascending (the order procps reads them in).
pub fn pids() -> Vec<i32> {
    let mut v: Vec<i32> = std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    v.sort_unstable();
    v
}

/// `/proc/PID/stat`'s comm (procps' `PIDS_CMD`).
pub fn comm(pid: i32) -> Option<Vec<u8>> {
    let s = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    let a = s.iter().position(|&b| b == b'(')?;
    let b = s.iter().rposition(|&b| b == b')')?;
    Some(s.get(a + 1..b)?.to_vec())
}

/// `/proc/PID/cmdline` split at its NULs (empty for a kernel thread or a zombie).
pub fn cmdline(pid: i32) -> Vec<Vec<u8>> {
    let Ok(c) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return vec![];
    };
    let c = c.strip_suffix(&[0]).unwrap_or(&c);
    if c.is_empty() {
        return vec![];
    }
    c.split(|&b| b == 0).map(<[u8]>::to_vec).collect()
}

/// `/proc/PID/exe`'s target, empty when unreadable.
pub fn exe(pid: i32) -> Vec<u8> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.into_os_string().into_vec())
        .unwrap_or_default()
}

pub fn basename(s: &[u8]) -> &[u8] {
    s.rsplit(|&b| b == b'/').next().unwrap_or(s)
}

/// procps-ng `pidof NAME` (`select_procs`): the matching PIDs newest first, never this
/// process (pidof leaves itself out).
pub fn pidof(name: &str) -> Vec<i32> {
    let program = name.as_bytes();
    let program_base = basename(program);
    let me = std::process::id() as i32;
    let mut out = vec![];
    for pid in pids().into_iter().filter(|&p| p != me) {
        let argv = cmdline(pid);
        let Some(arg0) = argv.first() else {
            continue;
        };
        let arg0 = arg0.strip_prefix(b"-").unwrap_or(arg0);
        let exe = exe(pid);
        let mut hit = program == basename(arg0)
            || program_base == arg0
            || program == arg0
            || program == basename(&exe)
            || program == exe.as_slice();
        if !hit && arg0.contains(&b' ') {
            hit = comm(pid).is_some_and(|c| c == program);
        }
        if hit {
            out.push(pid);
        }
    }
    out.reverse();
    out
}

/// procps-ng `pgrep PATTERN` for a PATTERN without regex metacharacters: every process whose
/// comm contains it, ascending.
pub fn pgrep(pattern: &str) -> Vec<i32> {
    let pat = pattern.as_bytes();
    pids()
        .into_iter()
        .filter(|&pid| comm(pid).is_some_and(|c| c.windows(pat.len()).any(|w| w == pat)))
        .collect()
}

/// procps-ng `pkill -SIG PATTERN` (literal PATTERN): true when a process was signalled
/// (exit status 0).
pub fn pkill(sig: i32, pattern: &str) -> bool {
    let mut killed = false;
    for pid in pgrep(pattern) {
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        killed |= unsafe { libc::kill(pid, sig) } == 0;
    }
    killed
}

/// Send `sig` to `ch` once `stop` is requested (at once if it already was) while the returned
/// guard lives, so a thread can block in `wait`/`read` on the child instead of polling the
/// flag. The signal goes through a pidfd, never to a pid the kernel has since reused.
pub fn signal_on_stop<'a>(
    ch: &std::process::Child,
    stop: &'a crate::svc::Stop,
    sig: i32,
) -> Option<crate::svc::OnStop<'a>> {
    use std::os::fd::AsRawFd;
    let fd = match workpool::pidfd_open(ch.id() as i32) {
        Ok(fd) => fd,
        Err(e) => {
            elog::eprintln!("impulse-shell: child {}: pidfd_open: {e}", ch.id());
            return None;
        }
    };
    // SAFETY: `fd` is the pidfd the closure owns, open for the call; pidfd_send_signal(2) takes
    // a null siginfo and flags 0 for a plain kill(2)-like signal.
    Some(stop.on_stop(move || unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            sig,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        );
    }))
}

/// `p` opened for reading if it is a regular file. A FIFO is opened with `O_NONBLOCK`, so it
/// is refused instead of blocking until a writer appears; devices and directories are refused
/// too (`/dev/zero` never ends).
pub fn open_regular(p: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(p)?;
    let m = f.metadata()?;
    if m.is_dir() {
        return Err(std::io::ErrorKind::IsADirectory.into());
    }
    if !m.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(f)
}

/// Copies `r` to `w` up to `max` bytes; `Err(FileTooLarge)` once there is more.
pub fn copy_max(
    r: &mut dyn std::io::Read,
    w: &mut dyn std::io::Write,
    max: u64,
) -> std::io::Result<u64> {
    use std::io::Read;
    let n = std::io::copy(&mut r.take(max.saturating_add(1)), w)?;
    if n > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::FileTooLarge,
            format!("over {max} bytes"),
        ));
    }
    Ok(n)
}

/// The regular file `p` ([`open_regular`]), at most `max` bytes ([`copy_max`]).
pub fn read_regular(p: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let mut f = open_regular(p)?;
    let mut v = Vec::new();
    copy_max(&mut f, &mut v, max)?;
    Ok(v)
}

/// Which passwd entry [`passwd`] looks up.
#[derive(Clone, Copy)]
pub enum PwKey<'a> {
    Uid(libc::uid_t),
    Name(&'a std::ffi::CStr),
}

/// A passwd entry [`passwd`] read; its strings live in the lookup's buffer, which it borrows.
pub struct PwEntry<'a> {
    pw: libc::passwd,
    _buf: std::marker::PhantomData<&'a [u8]>,
}

impl PwEntry<'_> {
    pub fn name(&self) -> Option<&std::ffi::CStr> {
        self.field(self.pw.pw_name)
    }

    pub fn dir(&self) -> Option<&std::ffi::CStr> {
        self.field(self.pw.pw_dir)
    }

    pub fn gecos(&self) -> Option<&std::ffi::CStr> {
        self.field(self.pw.pw_gecos)
    }

    fn field(&self, p: *const libc::c_char) -> Option<&std::ffi::CStr> {
        // SAFETY: getpw*_r(3) points each non-null string field at a NUL-terminated string in
        // the buffer the entry was read into, which `self` borrows.
        (!p.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(p) })
    }
}

/// `f` of a passwd entry read with getpwuid_r(3)/getpwnam_r(3) into a buffer of ours, grown
/// while the entry does not fit (ERANGE): getpwuid/getpwnam return static storage another
/// thread's lookup may overwrite meanwhile. `Ok(None)`: no such entry; `Err`: the lookup's error.
pub fn passwd<T>(key: PwKey, f: impl FnOnce(&PwEntry) -> T) -> Result<Option<T>, i32> {
    // _SC_GETPW_R_SIZE_MAX is only a suggested start (getpwnam_r(3)); 1 MiB bounds the growth.
    let mut buf = vec![0u8; 16384];
    loop {
        // SAFETY: `passwd` is a C struct of integers and pointers: all-zero is a valid value.
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut res: *mut libc::passwd = std::ptr::null_mut();
        let (p, n) = (buf.as_mut_ptr().cast(), buf.len());
        let r = match key {
            // SAFETY: `pw`, `res` and the `n` bytes at `p` are writable for the call.
            PwKey::Uid(uid) => unsafe { libc::getpwuid_r(uid, &mut pw, p, n, &mut res) },
            // SAFETY: as above, and `name` is NUL-terminated.
            PwKey::Name(name) => unsafe {
                libc::getpwnam_r(name.as_ptr(), &mut pw, p, n, &mut res)
            },
        };
        match r {
            0 if res.is_null() => return Ok(None),
            0 => {
                let e = PwEntry {
                    pw,
                    _buf: std::marker::PhantomData::<&[u8]>,
                };
                return Ok(Some(f(&e)));
            }
            libc::ERANGE if n < 1 << 20 => buf.resize(n * 2, 0),
            e => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidates_are_built_like_execvp() {
        let got: Vec<_> = super::candidates("x", ":/a/:/b".as_ref()).collect();
        assert_eq!(got, ["./x", "/a//x", "/b/x"].map(std::path::PathBuf::from));
    }

    #[test]
    fn finds_own_process_tables() {
        let me = std::process::id() as i32;
        assert!(super::pids().contains(&me));
        assert!(!super::cmdline(me).is_empty());
        assert!(super::comm(me).is_some_and(|c| !c.is_empty()));
        assert!(
            !super::pidof(std::str::from_utf8(&super::comm(me).unwrap()).unwrap()).contains(&me)
        );
    }

    #[test]
    fn read_regular_refuses_fifos_devices_and_oversize() {
        let d = tempfile::tempdir().unwrap();
        let fifo = d.path().join("fifo");
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `c` is a NUL-terminated path, alive for the call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // Returns at once instead of blocking for a writer.
        let e = super::read_regular(&fifo, 1 << 20).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        let e = super::read_regular("/dev/zero".as_ref(), 1 << 20).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        let e = super::read_regular(d.path(), 1 << 20).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::IsADirectory);
        let f = d.path().join("f");
        std::fs::write(&f, [7u8; 100]).unwrap();
        assert_eq!(super::read_regular(&f, 100).unwrap(), [7u8; 100]);
        let e = super::read_regular(&f, 99).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn uid_and_name_lookups_agree() {
        // SAFETY: getuid(2) takes no arguments and always succeeds.
        let uid = unsafe { libc::getuid() };
        let name = super::passwd(super::PwKey::Uid(uid), |e| e.name().map(|n| n.to_owned()))
            .unwrap()
            .flatten()
            .expect("this user's passwd entry");
        let back = super::passwd(super::PwKey::Name(&name), |e| e.pw.pw_uid).unwrap();
        assert_eq!(back, Some(uid));
    }

    #[test]
    fn missing_name_is_none() {
        let r = super::passwd(super::PwKey::Name(c"ri-no-such-user"), |_| ());
        assert_eq!(r, Ok(None));
    }
}

/// QDir::cleanPath of an absolute path: without empty and `.` parts, and without each `..`
/// and the part before it.
fn clean_path(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => drop(parts.pop()),
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The name of the system's zone as the files under /etc give it, and the device and inode
/// of the file it was last read from (Qt's ZoneNameReader).
#[derive(Default)]
struct ZoneNameReader {
    name: String,
    last: Option<(u64, u64)>,
}

impl ZoneNameReader {
    /// ZoneNameReader::name where /etc is `etc` and `TZDIR` is `tzdir`: the part of the
    /// localtime link's target after `tzdir` or "/zoneinfo/", else the content of TZ, else
    /// that of timezone. The
    /// name read last stands for as long as one of the three files is the file it was read
    /// from, whatever that file or the link holds now.
    fn name(&mut self, etc: &Path, tzdir: &str) -> String {
        use std::os::unix::fs::MetadataExt;
        let identify = |file: &str| {
            let file = std::fs::metadata(etc.join(file)).ok()?;
            Some((file.dev(), file.ino()))
        };
        let [local, tz, timezone] = ["localtime", "TZ", "timezone"].map(identify);
        let read_from = [local, tz, timezone].contains(&self.last);
        if !self.name.is_empty() && self.last.is_some() && read_from {
            return self.name.clone();
        }
        let content = |file: &str| {
            let text = std::fs::read(etc.join(file)).unwrap_or_default();
            String::from_utf8_lossy(text.trim_ascii()).into_owned()
        };
        (self.name, self.last) = (Self::link_name(etc, tzdir), local);
        if self.name.is_empty() {
            (self.name, self.last) = (content("TZ"), tz);
        }
        if self.name.is_empty() {
            self.name = content("timezone");
            self.last = timezone.filter(|_| !self.name.is_empty());
        }
        self.name.clone()
    }

    /// etcLocalTime: the localtime link is followed until a target has `tzdir` or
    /// "/zoneinfo/" in it, through at most 20 links (MAXSYMLINKS). Each target is cleaned
    /// as QFile::symLinkTarget cleans it.
    fn link_name(etc: &Path, tzdir: &str) -> String {
        let mut path = etc.join("localtime");
        for _ in 0..20 {
            let Ok(target) = std::fs::read_link(&path) else {
                break;
            };
            let target = path
                .parent()
                .map_or(target.clone(), |dir| dir.join(&target));
            let text = clean_path(&target.to_string_lossy());
            if let Some(at) = (!tzdir.is_empty()).then(|| text.find(tzdir)).flatten() {
                let tail = &text[at + tzdir.len()..];
                return tail.strip_prefix('/').unwrap_or(tail).to_string();
            }
            if let Some(at) = text.find("/zoneinfo/") {
                return text[at + "/zoneinfo/".len()..].to_string();
            }
            path = text.into();
        }
        String::new()
    }
}

/// The IANA id of the system time zone as Qt finds it
/// (QTzTimeZonePrivate::staticSystemTimeZoneId): `TZ` without its leading colon, else what
/// the files under /etc name ([`ZoneNameReader::name`]). Empty when none names a zone.
pub fn system_zone_id() -> String {
    thread_local! {
        static READER: std::cell::RefCell<ZoneNameReader> = Default::default();
    }
    let tz = std::env::var("TZ").unwrap_or_default();
    let tzdir = std::env::var("TZDIR").unwrap_or_default();
    match tz.as_str() {
        "" | ":/etc/localtime" => READER.with_borrow_mut(|r| r.name(Path::new("/etc"), &tzdir)),
        tz => tz.strip_prefix(':').unwrap_or(tz).to_string(),
    }
}

#[cfg(test)]
mod zone_name_tests {
    use super::ZoneNameReader;
    use std::os::unix::fs::symlink;

    fn fresh(etc: &std::path::Path, tzdir: &str) -> String {
        ZoneNameReader::default().name(etc, tzdir)
    }

    /// Qt 6.12 answers the same for each of these targets of /etc/localtime.
    #[test]
    fn the_links_target_is_cleaned_before_the_name_is_cut_from_it() {
        let dir = tempfile::tempdir().unwrap();
        let (etc, local) = (dir.path(), dir.path().join("localtime"));
        for (target, tzdir, name) in [
            ("/usr/share/zoneinfo/Europe/Moscow", "", "Europe/Moscow"),
            ("../usr/share/zoneinfo/Asia/Tokyo", "", "Asia/Tokyo"),
            (
                "/usr/share/zoneinfo/../zoneinfo/Europe/Paris",
                "",
                "Europe/Paris",
            ),
            ("/usr/share/zoneinfo/./Asia/Kolkata", "", "Asia/Kolkata"),
            (
                "/usr/share//zoneinfo//America/New_York",
                "",
                "America/New_York",
            ),
            ("/usr/share/zoneinfo/Asia/Tokyo/", "", "Asia/Tokyo"),
            (
                "/usr/share/zoneinfo/posix/Asia/Tokyo",
                "",
                "posix/Asia/Tokyo",
            ),
            ("/zones/Asia/Tokyo", "", ""),
            ("/zones/Asia/Tokyo", "/zones", "Asia/Tokyo"),
            ("/zones/Asia/Tokyo", "/zones/", "Asia/Tokyo"),
            ("/usr/share/zoneinfo/Asia/Tokyo", "/zones", "Asia/Tokyo"),
        ] {
            let _ = std::fs::remove_file(&local);
            symlink(target, &local).unwrap();
            assert_eq!(fresh(etc, tzdir), name, "{target} with TZDIR={tzdir}");
        }
    }

    #[test]
    fn a_chain_of_links_is_followed_through_twenty() {
        let dir = tempfile::tempdir().unwrap();
        let etc = dir.path();
        for i in 1..=20 {
            symlink(
                etc.join(format!("link{}", i + 1)),
                etc.join(format!("link{i}")),
            )
            .unwrap();
        }
        symlink("/usr/share/zoneinfo/Asia/Tokyo", etc.join("link21")).unwrap();
        // localtime, link3 .. link21: 20 links.
        symlink(etc.join("link3"), etc.join("localtime")).unwrap();
        assert_eq!(fresh(etc, ""), "Asia/Tokyo");
        std::fs::remove_file(etc.join("localtime")).unwrap();
        symlink(etc.join("link2"), etc.join("localtime")).unwrap();
        assert_eq!(fresh(etc, ""), "");
    }

    #[test]
    fn a_file_names_the_zone_when_the_link_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let etc = dir.path();
        assert_eq!(fresh(etc, ""), "");
        std::fs::write(etc.join("localtime"), "a zone's data").unwrap();
        assert_eq!(fresh(etc, ""), "");
        std::fs::write(etc.join("timezone"), "Asia/Kolkata\n").unwrap();
        assert_eq!(fresh(etc, ""), "Asia/Kolkata");
        std::fs::write(etc.join("TZ"), "\n").unwrap();
        assert_eq!(fresh(etc, ""), "Asia/Kolkata");
        std::fs::write(etc.join("TZ"), " Asia/Tokyo\n").unwrap();
        assert_eq!(fresh(etc, ""), "Asia/Tokyo");
        std::fs::remove_file(etc.join("localtime")).unwrap();
        symlink("/usr/share/zoneinfo/Europe/Paris", etc.join("localtime")).unwrap();
        assert_eq!(fresh(etc, ""), "Europe/Paris");
    }

    /// As in Qt 6.12, one reader in turn: a file written in place still names the zone it
    /// named, a new file in its place names its own, and while that file is there a link
    /// that appears is not read.
    #[test]
    fn the_name_stands_while_the_file_it_was_read_from_is_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let etc = dir.path();
        let mut reader = ZoneNameReader::default();
        std::fs::write(etc.join("timezone"), "Asia/Tokyo\n").unwrap();
        assert_eq!(reader.name(etc, ""), "Asia/Tokyo");
        std::fs::write(etc.join("timezone"), "Asia/Kolkata\n").unwrap();
        assert_eq!(reader.name(etc, ""), "Asia/Tokyo");
        std::fs::write(etc.join("new"), "Europe/Paris\n").unwrap();
        std::fs::rename(etc.join("new"), etc.join("timezone")).unwrap();
        assert_eq!(reader.name(etc, ""), "Europe/Paris");
        symlink("/usr/share/zoneinfo/Europe/Moscow", etc.join("localtime")).unwrap();
        assert_eq!(reader.name(etc, ""), "Europe/Paris");
        std::fs::remove_file(etc.join("timezone")).unwrap();
        assert_eq!(reader.name(etc, ""), "Europe/Moscow");
    }

    /// Two names of one file of the database: the first one read stands.
    #[test]
    fn another_name_of_the_same_zone_file_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let etc = dir.path();
        let zones = etc.join("zoneinfo/Europe");
        std::fs::create_dir_all(&zones).unwrap();
        std::fs::write(zones.join("Berlin"), "a zone's data").unwrap();
        std::fs::hard_link(zones.join("Berlin"), zones.join("Oslo")).unwrap();
        std::fs::write(zones.join("Moscow"), "another zone's data").unwrap();
        let mut reader = ZoneNameReader::default();
        let mut link = |zone: &str| {
            let _ = std::fs::remove_file(etc.join("localtime"));
            symlink(zones.join(zone), etc.join("localtime")).unwrap();
            reader.name(etc, "")
        };
        let names = ["Oslo", "Berlin", "Moscow", "Berlin"].map(&mut link);
        let read = [
            "Europe/Oslo",
            "Europe/Oslo",
            "Europe/Moscow",
            "Europe/Berlin",
        ];
        assert_eq!(names, read);
    }
}

/// The system time zone's abbreviation at `secs` since the epoch and whether daylight-saving
/// time is in effect then (libc's `tm_zone` and `tm_isdst`).
pub fn zone_abbreviation(secs: i64) -> (String, bool) {
    // SAFETY: `tm` is plain integers and a pointer, for which all zeroes is a value.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // As Qt's qLocalTime: localtime_r need not look whether the zone has changed.
    // SAFETY: `tzset` takes no arguments.
    unsafe { tzset() };
    // SAFETY: both pointers are valid for the call.
    let filled = unsafe { libc::localtime_r(&(secs as libc::time_t), &mut tm) };
    if filled.is_null() || tm.tm_zone.is_null() {
        return (String::new(), false);
    }
    // SAFETY: `tm_zone` points at a string libc keeps for the life of the process.
    let abbr = unsafe { std::ffi::CStr::from_ptr(tm.tm_zone) };
    (abbr.to_string_lossy().into_owned(), tm.tm_isdst > 0)
}

unsafe extern "C" {
    fn tzset();
    static tzname: [*const libc::c_char; 2];
}

/// The C library's names for the local zone's standard and daylight time (`tzname`).
pub fn zone_names() -> [String; 2] {
    // SAFETY: `tzset` takes no arguments and fills `tzname`.
    unsafe { tzset() };
    // SAFETY: `tzname` is two pointers, read at once before anything sets the zone again.
    let names = unsafe { std::ptr::addr_of!(tzname).read() };
    names.map(|p| match p.is_null() {
        true => String::new(),
        // SAFETY: a non-null `tzname` entry is a NUL-terminated string libc owns.
        false => unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned(),
    })
}
