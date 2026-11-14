//! inotify watches behind FileView `watchChanges` and FolderListModel, in place of
//! QFileSystemWatcher. As in QInotifyFileSystemWatcherEngine, the inotify fd (IN_NONBLOCK) is
//! read where the watches live: it sits in the World's epoll set (`native::EP_FSWATCH`), so the
//! main loop wakes when it turns readable, and each `poll_native` reads everything queued with
//! one FIONREAD-sized read() as one batch. No thread, no lock; nothing is stat'ed while idle.
//!
//! QFileSystemWatcher (inotify engine) watches a file for IN_ATTRIB/IN_MODIFY/IN_MOVE_SELF/
//! IN_DELETE_SELF and drops it after the latter two; a directory for everything but its
//! children's IN_MODIFY. Every watch here subscribes to the union, so events are classified here.

use crate::runtime::World;
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};
use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The inode an inotify watch is on: a watch follows the inode, not the path.
type Ino = (u64, u64);

fn ino(p: &Path) -> Option<Ino> {
    std::fs::metadata(p).ok().map(|m| (m.dev(), m.ino()))
}

const MASK: WatchMask = WatchMask::ATTRIB
    .union(WatchMask::CREATE)
    .union(WatchMask::DELETE)
    .union(WatchMask::MODIFY)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MOVE_SELF);

/// What an inotify event did at its paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// IN_CREATE.
    Create,
    /// IN_DELETE (a child) or IN_DELETE_SELF.
    Remove,
    /// IN_MOVED_FROM, or IN_MOVE_SELF: the name left the path.
    MovedFrom,
    /// IN_MOVED_TO.
    MovedTo,
    /// An IN_MOVED_TO whose cookie matches the last IN_MOVED_FROM: paths are [from, to].
    Renamed,
    /// IN_MODIFY.
    Data,
    /// IN_ATTRIB.
    Attrib,
    /// IN_IGNORED: the kernel dropped the watch (inode deleted, filesystem unmounted).
    Gone,
    /// IN_Q_OVERFLOW: events were lost, every watch is checked again (no paths).
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Event {
    kind: Kind,
    paths: Vec<PathBuf>,
}

impl Event {
    fn need_rescan(&self) -> bool {
        self.kind == Kind::Overflow
    }
}

/// Which events of a batch are kept: an anchor's other children's events are dropped, so
/// following a missing path costs nothing past the read.
#[derive(Default)]
struct Filter {
    wanted: HashSet<PathBuf>,
    /// Anchor that is not itself a wanted path -> its children on the way to a parked path.
    quiet: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl Filter {
    fn forwards(&self, p: &Path) -> bool {
        let Some(parent) = p.parent() else {
            return true;
        };
        match self.quiet.get(parent) {
            Some(awaited) => {
                awaited.contains(p) || self.wanted.contains(p) || self.quiet.contains_key(p)
            }
            None => true,
        }
    }
}

/// One per `World`.
///
/// Every wanted path is either watched itself or, while it (or a directory above it) does not
/// exist, parked on its nearest existing ancestor ("anchor"), as inotify-based watchers do: a
/// watch on a deleted or moved-away inode never sees the path again. Structural events
/// (create/delete/rename at or above a watched path) make the watches be checked against the
/// inodes now at their paths; nothing is stat'ed while idle.
#[derive(Default)]
pub struct Watches {
    /// The World's epoll set the inotify fd joins once created.
    ep: Option<i32>,
    ino: Option<Inotify>,
    /// Watch descriptor -> the watched paths on its inode (inotify hands out one descriptor
    /// per inode; QInotifyFileSystemWatcherEngine's `idToPath` is a multi-map too).
    wds: HashMap<WatchDescriptor, Vec<PathBuf>>,
    /// Cookie and path of the last IN_MOVED_FROM, for [`Kind::Renamed`].
    moved_from: Option<(u32, PathBuf)>,
    filter: Filter,
    /// Wanted path -> users.
    paths: HashMap<PathBuf, usize>,
    /// inotify watches: path -> the inode watched there.
    kernel: HashMap<PathBuf, Ino>,
    /// inotify watches: path -> its descriptor.
    wd: HashMap<PathBuf, WatchDescriptor>,
    /// Wanted paths that do not exist -> the anchor watched in their place.
    parked: HashMap<PathBuf, PathBuf>,
    /// Wanted paths watched when this batch arrived.
    was: HashSet<PathBuf>,
    /// Wanted paths this batch watches at a new inode without having seen it change there: a
    /// parked path that reappeared, or a path whose directory was replaced.
    renewed: HashSet<PathBuf>,
    /// Targets whose fileChanged this batch comes only from `renewed`.
    renewed_only: HashSet<PathBuf>,
    /// FileView item -> (target, its directory).
    files: HashMap<usize, (PathBuf, Option<PathBuf>)>,
    /// FolderListModel item -> listed directory.
    folders: HashMap<usize, PathBuf>,
    /// DesktopEntries' directories.
    desktop: Vec<PathBuf>,
    /// Native service watches by name -> (target, its directory).
    named: HashMap<&'static str, (PathBuf, Option<PathBuf>)>,
    batch: Vec<Event>,
    /// fileChanged verdicts for this batch, per target: FileViews sharing a path agree.
    verdicts: HashMap<PathBuf, bool>,
    /// Non-empty event batches taken so far (`Runtime::file_events`).
    batches: u64,
}

fn watches(w: &mut World) -> &mut Watches {
    if w.fswatch.ep.is_none() {
        w.fswatch.ep = crate::native::proc_ep(w);
    }
    &mut w.fswatch
}

/// A name created, deleted or renamed at the event's path, its watch gone, or the queue
/// overflowed: the inode behind that path and every path below it may have changed.
fn structural(e: &Event) -> bool {
    !matches!(e.kind, Kind::Data | Kind::Attrib)
}

impl Watches {
    fn inotify(&mut self) -> Option<&mut Inotify> {
        if self.ino.is_none() {
            let ino = Inotify::init()
                .map_err(|e| elog::eprintln!("file watcher: {e}"))
                .ok()?;
            if let Some(ep) = self.ep
                && !crate::native::ep_add(ep, ino.as_raw_fd(), crate::native::EP_FSWATCH)
            {
                elog::eprintln!("file watcher: epoll: {}", std::io::Error::last_os_error());
            }
            self.ino = Some(ino);
        }
        self.ino.as_mut()
    }

    /// Puts an inotify watch on the inode now at `p`.
    fn watch(&mut self, p: &Path) -> bool {
        if self.kernel.contains_key(p) {
            return true;
        }
        let wd = {
            let Some(n) = self.inotify() else {
                return false;
            };
            let Ok(wd) = n.watches().add(p, MASK) else {
                return false;
            };
            wd
        };
        let Some(i) = ino(p) else {
            if !self.wds.contains_key(&wd)
                && let Some(n) = &mut self.ino
            {
                let _ = n.watches().remove(wd);
            }
            return false;
        };
        self.kernel.insert(p.to_path_buf(), i);
        self.wd.insert(p.to_path_buf(), wd.clone());
        self.wds.entry(wd).or_default().push(p.to_path_buf());
        true
    }

    fn unwatch(&mut self, p: &Path) {
        self.kernel.remove(p);
        let Some(wd) = self.wd.remove(p) else {
            return;
        };
        let Some(v) = self.wds.get_mut(&wd) else {
            return;
        };
        v.retain(|x| x != p);
        if v.is_empty() {
            self.wds.remove(&wd);
            if let Some(n) = &mut self.ino {
                // Fails (EINVAL) for a watch the kernel already dropped with its inode.
                let _ = n.watches().remove(wd);
            }
        }
    }

    /// Everything queued on the inotify fd, read at once as QInotifyFileSystemWatcherEngine
    /// does (FIONREAD bytes, one read()), mapped to paths and filtered.
    fn read(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        let Some(ino) = self.ino.as_mut() else {
            return out;
        };
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int.
        if unsafe { libc::ioctl(ino.as_raw_fd(), libc::FIONREAD, &mut n) } < 0 || n <= 0 {
            return out;
        }
        let mut buf = vec![0u8; n as usize];
        let Ok(evs) = ino.read_events(&mut buf) else {
            return out;
        };
        for e in evs {
            if e.mask.contains(EventMask::Q_OVERFLOW) {
                out.push(Event {
                    kind: Kind::Overflow,
                    paths: vec![],
                });
                continue;
            }
            let Some(roots) = self.wds.get(&e.wd) else {
                continue;
            };
            for root in roots {
                let path = match e.name {
                    Some(n) => root.join(n),
                    None => root.clone(),
                };
                let mut push = |kind, paths: Vec<PathBuf>| {
                    if paths.iter().any(|p| self.filter.forwards(p)) {
                        out.push(Event { kind, paths });
                    }
                };
                if e.mask.contains(EventMask::MOVED_FROM) {
                    push(Kind::MovedFrom, vec![path.clone()]);
                    self.moved_from = Some((e.cookie, path.clone()));
                } else if e.mask.contains(EventMask::MOVED_TO) {
                    push(Kind::MovedTo, vec![path.clone()]);
                    if self
                        .moved_from
                        .as_ref()
                        .is_some_and(|(c, _)| *c == e.cookie)
                    {
                        let (_, from) = self.moved_from.take().unwrap();
                        push(Kind::Renamed, vec![from, path.clone()]);
                    }
                }
                for (m, kind) in [
                    (EventMask::MOVE_SELF, Kind::MovedFrom),
                    (EventMask::CREATE, Kind::Create),
                    (EventMask::DELETE, Kind::Remove),
                    (EventMask::DELETE_SELF, Kind::Remove),
                    (EventMask::MODIFY, Kind::Data),
                    (EventMask::ATTRIB, Kind::Attrib),
                    (EventMask::IGNORED, Kind::Gone),
                ] {
                    if e.mask.contains(m) {
                        push(kind, vec![path.clone()]);
                    }
                }
            }
        }
        out
    }

    /// Parks each missing wanted path on its nearest existing ancestor and drops the watches
    /// nothing needs any more.
    fn park(&mut self) {
        let mut missing: Vec<PathBuf> = self
            .paths
            .keys()
            .filter(|p| !self.kernel.contains_key(*p))
            .cloned()
            .collect();
        missing.sort();
        self.parked.clear();
        for p in missing {
            let anchor = p
                .ancestors()
                .skip(1)
                .filter(|a| !a.as_os_str().is_empty())
                .find(|a| self.watch(a))
                .map(Path::to_path_buf);
            if let Some(a) = anchor {
                self.parked.insert(p, a);
            }
        }
        let stale: Vec<PathBuf> = self
            .kernel
            .keys()
            .filter(|k| !self.paths.contains_key(*k) && !self.parked.values().any(|a| a == *k))
            .cloned()
            .collect();
        for k in stale {
            self.unwatch(&k);
        }
        let f = &mut self.filter;
        f.wanted = self.paths.keys().cloned().collect();
        f.quiet.clear();
        for (p, a) in &self.parked {
            if self.paths.contains_key(a) {
                continue;
            }
            let next = p
                .ancestors()
                .find(|c| c.parent() == Some(a.as_path()))
                .unwrap_or(p);
            f.quiet
                .entry(a.clone())
                .or_default()
                .insert(next.to_path_buf());
        }
    }

    /// Checks the watches this batch's structural events concern against the inodes now at
    /// their paths: a watch on an inode that left its path is dropped, a wanted path without a
    /// watch is watched again if it exists, else parked.
    fn resync(&mut self) {
        let all = self.batch.iter().any(Event::need_rescan);
        let roots: Vec<PathBuf> = self
            .batch
            .iter()
            .filter(|e| structural(e))
            .flat_map(|e| e.paths.iter().cloned())
            .collect();
        if !all && roots.is_empty() {
            return;
        }
        // A name deleted or moved away at or above a watch: its inode left the path, even if
        // a new file there got the same (reused) inode number.
        let left: Vec<&Path> = self
            .batch
            .iter()
            .filter(|e| matches!(e.kind, Kind::Remove | Kind::MovedFrom | Kind::Gone))
            .flat_map(|e| e.paths.iter().map(PathBuf::as_path))
            .collect();
        let touched = |p: &Path| all || roots.iter().any(|r| p.starts_with(r));
        let stale: Vec<PathBuf> = self
            .kernel
            .iter()
            .filter(|(k, i)| {
                left.iter().any(|r| k.starts_with(r)) || (touched(k) && ino(k) != Some(**i))
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in &stale {
            self.unwatch(k);
        }
        let mut retry: Vec<PathBuf> = self
            .paths
            .keys()
            .filter(|p| !self.kernel.contains_key(*p) && touched(p))
            .cloned()
            .collect();
        retry.sort();
        for p in retry {
            let back = self.parked.contains_key(&p) || self.was.contains(&p);
            if self.watch(&p) && back {
                self.renewed.insert(p);
            }
        }
        if !stale.is_empty() || self.paths.keys().any(|p| !self.kernel.contains_key(p)) {
            self.park();
        }
    }

    fn add(&mut self, p: &Path) {
        let e = self.paths.entry(p.to_path_buf()).or_default();
        *e += 1;
        if *e == 1 {
            self.watch(p);
            self.park();
        }
    }

    fn release(&mut self, p: &Path) {
        let Some(e) = self.paths.get_mut(p) else {
            return;
        };
        *e -= 1;
        if *e == 0 {
            self.paths.remove(p);
            self.park();
        }
    }

    fn armed(&self, p: &Path) -> bool {
        self.paths.contains_key(p) && self.kernel.contains_key(p)
    }

    /// Events QFileSystemWatcher reports as `directoryChanged(d)`.
    fn dir_changed(&self, d: &Path) -> bool {
        self.batch.iter().any(|e| {
            !matches!(e.kind, Kind::Data | Kind::Gone)
                && e.paths.iter().any(|p| p == d || p.parent() == Some(d))
        })
    }

    /// QuickShell FileView::onWatchedFileChanged / onWatchedDirectoryChanged for target `t`,
    /// plus a target watched again at a new inode (`renewed`), which Qt's dead watches miss.
    fn file_changed(&mut self, t: &Path, d: Option<&Path>) -> bool {
        if let Some(&v) = self.verdicts.get(t) {
            return v;
        }
        let was = self.was.contains(t);
        let own = self
            .batch
            .iter()
            .filter(|e| e.paths.iter().any(|p| p == t))
            .map(|e| &e.kind)
            // A name arriving at or leaving a watched target is the directory watch's event;
            // Qt reports it through the file's own IN_ATTRIB/IN_DELETE_SELF/IN_MOVE_SELF, which
            // may land in a later batch.
            .any(|k| {
                *k != Kind::Gone
                    && !(was && matches!(k, Kind::Create | Kind::MovedFrom | Kind::MovedTo))
            });
        let qt = (was && own) || (!was && d.is_some_and(|d| self.dir_changed(d)) && t.exists());
        let fire = qt || self.renewed.contains(t);
        if fire && !qt {
            self.renewed_only.insert(t.to_path_buf());
        }
        self.verdicts.insert(t.to_path_buf(), fire);
        fire
    }
}

/// Non-empty inotify batches `poll_native` has taken: a host that changed a watched path waits
/// for this to advance to know the change reached the world.
pub(crate) fn batches(w: &World) -> u64 {
    w.fswatch.batches
}

/// Reads the events queued since the last call; the `file_changed` / `folder_changed` calls
/// of this poll judge them.
pub(crate) fn take_events(w: &mut World) {
    let w = watches(w);
    w.batch = w.read();
    w.verdicts.clear();
    w.renewed.clear();
    w.renewed_only.clear();
    if w.batch.is_empty() {
        return;
    }
    w.batches += 1;
    w.was = w
        .paths
        .keys()
        .filter(|p| w.kernel.contains_key(*p))
        .cloned()
        .collect();
    w.resync();
}

/// Whether target `t`'s fileChanged this poll comes only from its watch moving to a new inode
/// (it reappeared, or its directory was replaced), not from a change seen at the old one.
pub(crate) fn renewed_only(w: &World, t: &str) -> bool {
    w.fswatch.renewed_only.contains(Path::new(t))
}

/// FileView directory watch: QuickShell watches `dirPath` = target up to its last '/' (with
/// "./" prepended to bare names), so "/x" gets none.
fn dir_of(t: &str) -> Option<PathBuf> {
    let d = if t.contains('/') {
        t.to_string()
    } else {
        format!("./{t}")
    };
    let d = &d[..d.rfind('/')?];
    (!d.is_empty()).then(|| PathBuf::from(d))
}

/// Whether FileView `i` emits fileChanged this poll; `target` is its path while
/// `watchChanges` is on. A new or changed target is (re)watched and reports nothing yet.
pub(crate) fn file_changed(w: &mut World, i: usize, target: Option<&str>) -> bool {
    {
        let w = watches(w);
        let want = target.map(|t| (PathBuf::from(t), dir_of(t)));
        if w.files.get(&i) != want.as_ref() {
            if let Some((t, d)) = w.files.remove(&i) {
                w.release(&t);
                if let Some(d) = d {
                    w.release(&d);
                }
            }
            if let Some((t, d)) = want {
                w.add(&t);
                if let Some(d) = &d {
                    w.add(d);
                }
                w.files.insert(i, (t, d));
            }
            return false;
        }
        let Some((t, d)) = want else { return false };
        w.file_changed(&t, d.as_deref())
    }
}

/// [`file_changed`] for a native service's own watch `name` (not a FileView item).
pub(crate) fn path_changed(w: &mut World, name: &'static str, target: &Path) -> bool {
    let w = watches(w);
    let want = (target.to_path_buf(), target.parent().map(Path::to_path_buf));
    if w.named.get(name) != Some(&want) {
        if let Some((t, d)) = w.named.remove(name) {
            w.release(&t);
            if let Some(d) = d {
                w.release(&d);
            }
        }
        w.add(&want.0);
        if let Some(d) = &want.1 {
            w.add(d);
        }
        w.named.insert(name, want);
        return false;
    }
    w.file_changed(&want.0, want.1.as_deref())
}

/// Ends [`path_changed`]'s watch `name`, if there is one.
pub(crate) fn path_released(w: &mut World, name: &'static str) {
    let w = watches(w);
    if let Some((t, d)) = w.named.remove(name) {
        w.release(&t);
        if let Some(d) = d {
            w.release(&d);
        }
    }
}

/// Whether [`path_changed`]'s watch `name` follows `target`: watched itself, through its
/// directory, or parked on an existing ancestor.
pub(crate) fn path_watched(w: &mut World, name: &'static str, target: &Path) -> bool {
    let w = watches(w);
    w.named
        .get(name)
        .is_some_and(|(t, _)| t == target && (w.armed(t) || w.parked.contains_key(t)))
}

/// Whether FolderListModel `m`'s directory changed since the last poll (QQuickFolderListModel's
/// directoryChanged/fileChanged rescans). A new directory is watched and reports nothing yet.
pub(crate) fn folder_changed(w: &mut World, m: usize, dir: &str) -> bool {
    {
        let w = watches(w);
        let d = PathBuf::from(dir);
        if w.folders.get(&m) != Some(&d) {
            if let Some(old) = w.folders.remove(&m) {
                w.release(&old);
            }
            w.add(&d);
            w.folders.insert(m, d);
            return false;
        }
        w.dir_changed(&d) || w.renewed.contains(&d)
    }
}

/// DesktopEntryMonitor::startMonitoring: each applications directory existing now, its parents
/// and its (non-symlink) subdirectories at this moment; the set is never revised (a directory
/// replaced on disk is still followed).
pub(crate) fn watch_desktop_dirs(w: &mut World, dirs: &[PathBuf]) {
    {
        let w = watches(w);
        for d in dirs.iter().filter(|d| d.is_dir()) {
            let mut v: Vec<PathBuf> = d
                .ancestors()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .collect();
            if let Ok(rd) = std::fs::read_dir(d) {
                v.extend(
                    rd.flatten()
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                        .map(|e| e.path()),
                );
            }
            for p in v {
                if !w.desktop.contains(&p) {
                    w.add(&p);
                    w.desktop.push(p);
                }
            }
        }
    }
}

/// Whether any DesktopEntries directory reported directoryChanged since the last poll.
pub(crate) fn desktop_dirs_changed(w: &World) -> bool {
    {
        let w = &w.fswatch;
        w.desktop.iter().any(|d| w.dir_changed(d))
    }
}

/// Drops the watches of FileViews no longer alive.
pub(crate) fn retain_files(w: &mut World, live: &[usize]) {
    {
        let w = watches(w);
        let dead: Vec<_> = w
            .files
            .keys()
            .filter(|i| !live.contains(i))
            .copied()
            .collect();
        for i in dead {
            let (t, d) = w.files.remove(&i).unwrap();
            w.release(&t);
            if let Some(d) = d {
                w.release(&d);
            }
        }
    }
}

/// Drops the watches of FolderListModels no longer alive.
pub(crate) fn retain_folders(w: &mut World, live: &[usize]) {
    {
        let w = watches(w);
        let dead: Vec<_> = w
            .folders
            .keys()
            .filter(|m| !live.contains(m))
            .copied()
            .collect();
        for m in dead {
            let d = w.folders.remove(&m).unwrap();
            w.release(&d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, dir_of, file_changed, folder_changed, take_events};
    use crate::runtime::World;
    use std::os::fd::AsRawFd;
    use std::path::PathBuf;

    fn base(tag: &str) -> PathBuf {
        let b = std::env::temp_dir().join(format!("fswatch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(&b).unwrap();
        b
    }

    /// Whether the World's epoll set (what the main loop sleeps on) is readable.
    fn wakes(w: &World) -> bool {
        let mut p = libc::pollfd {
            fd: w.proc_ep.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `p` is one live pollfd for the duration of the call.
        (unsafe { libc::poll(&mut p, 1, 0) }) > 0
    }

    #[test]
    fn directory_of_target_follows_quickshell() {
        assert_eq!(dir_of("/a/b/c.json"), Some(PathBuf::from("/a/b")));
        assert_eq!(dir_of("c.json"), Some(PathBuf::from(".")));
        assert_eq!(dir_of("/c.json"), None);
        assert_eq!(dir_of("a//c"), Some(PathBuf::from("a/")));
    }

    /// A write both the file's and its directory's watch report is one batch: all events are
    /// queued before write() returns and the next poll reads them with one read(), however
    /// long it comes after (no timing involved). The loop's epoll set wakes for it and is
    /// drained by the read.
    #[test]
    fn one_wakeup_reads_one_batch() {
        let b = base("batch");
        let t = b.join("a.json");
        std::fs::write(&t, "1").unwrap();
        let ts = t.to_str().unwrap();
        let mut w = World::default();
        assert!(!file_changed(&mut w, 1, Some(ts)));
        take_events(&mut w);
        assert!(w.fswatch.batch.is_empty());
        assert!(!wakes(&w), "idle watches do not wake the loop");
        std::fs::write(&t, "2").unwrap();
        assert!(wakes(&w));
        take_events(&mut w);
        assert!(!wakes(&w), "one read drains the queue");
        let data = |w: &World| {
            w.fswatch
                .batch
                .iter()
                .filter(|e| e.kind == Kind::Data && e.paths == [t.clone()])
                .count()
        };
        // write() truncates (IN_MODIFY) and writes (IN_MODIFY), seen by both watches.
        assert_eq!(data(&w), 4, "{:?}", w.fswatch.batch);
        assert!(file_changed(&mut w, 1, Some(ts)));
        take_events(&mut w);
        assert!(w.fswatch.batch.is_empty());
        assert!(!file_changed(&mut w, 1, Some(ts)));
        let _ = std::fs::remove_dir_all(&b);
    }

    /// A rename inside a watched directory pairs its IN_MOVED_FROM and IN_MOVED_TO by cookie.
    #[test]
    fn rename_pairs_by_cookie() {
        let b = base("rename");
        std::fs::write(b.join(".tmp"), "x").unwrap();
        let mut w = World::default();
        assert!(!folder_changed(&mut w, 1, b.to_str().unwrap()));
        std::fs::rename(b.join(".tmp"), b.join("a")).unwrap();
        take_events(&mut w);
        assert!(
            w.fswatch
                .batch
                .iter()
                .any(|e| e.kind == Kind::Renamed && e.paths == [b.join(".tmp"), b.join("a")]),
            "{:?}",
            w.fswatch.batch
        );
        assert!(folder_changed(&mut w, 1, b.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&b);
    }

    /// inotify gives two paths to one inode the same descriptor: both hear its events.
    #[test]
    fn paths_sharing_an_inode_both_report() {
        let b = base("alias");
        std::fs::create_dir(b.join("d")).unwrap();
        std::os::unix::fs::symlink(b.join("d"), b.join("l")).unwrap();
        let (d, l) = (b.join("d"), b.join("l"));
        let mut w = World::default();
        assert!(!folder_changed(&mut w, 1, d.to_str().unwrap()));
        assert!(!folder_changed(&mut w, 2, l.to_str().unwrap()));
        std::fs::write(d.join("f"), "x").unwrap();
        take_events(&mut w);
        assert!(folder_changed(&mut w, 1, d.to_str().unwrap()));
        assert!(folder_changed(&mut w, 2, l.to_str().unwrap()));
        // Dropping one keeps the other's (shared) kernel watch.
        super::retain_folders(&mut w, &[1]);
        std::fs::write(d.join("g"), "x").unwrap();
        take_events(&mut w);
        assert!(folder_changed(&mut w, 1, d.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&b);
    }

    /// A path parked on an ancestor costs nothing for that ancestor's other children: their
    /// events are dropped at the read.
    #[test]
    fn parked_path_ignores_its_anchors_other_children() {
        let base = base("park");
        let t = base.join("gen/sub/colors.json");
        let mut w = World::default();
        assert!(!file_changed(&mut w, 1, Some(t.to_str().unwrap())));
        assert_eq!(w.fswatch.parked.get(&t), Some(&base));
        std::fs::write(base.join("other"), "x").unwrap();
        std::fs::write(base.join("other"), "y").unwrap();
        std::fs::create_dir(base.join("gen2")).unwrap();
        std::fs::remove_file(base.join("other")).unwrap();
        take_events(&mut w);
        assert!(w.fswatch.batch.is_empty(), "{:?}", w.fswatch.batch);
        std::fs::create_dir(base.join("gen")).unwrap();
        take_events(&mut w);
        assert!(!w.fswatch.batch.is_empty());
        assert_eq!(
            w.fswatch.parked.get(&t),
            Some(&base.join("gen")),
            "moved down to the new directory"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
