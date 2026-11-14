//! `Rustshell.reload()` and the check that the QML tree still matches the compiled code.

use ricompiled::Stale;
use std::path::Path;

/// Files of the QML tree at `root` this build cannot run as they are (edited or added since);
/// with `-p FILE` (`app_file`) only that file, not whatever directory holds it.
pub fn stale_tree(root: &Path, app_file: Option<&Path>) -> Vec<Stale> {
    match app_file {
        Some(f) => {
            let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            let key = ricompiled::file_key(&root, f).unwrap_or_else(|| f.display().to_string());
            ricompiled::stale(&key, f).into_iter().collect()
        }
        None => ricompiled::stale_files(root),
    }
}

/// What keeps the QML tree at `root` from being the tree this binary was compiled from, one
/// line per file: the files [`stale_tree`] names and, for a whole tree, the compiled files it
/// lacks. The one rule of the start, of `Rustshell.reload()` and of `--check-tree`: a build
/// without the interpreter runs this tree or none, since a file gone leaves the shell without
/// what it declared (a panel, or every layer when the root needs it).
pub fn differing_tree(root: &Path, app_file: Option<&Path>) -> Vec<String> {
    let mut out: Vec<String> = stale_tree(root, app_file)
        .iter()
        .map(ToString::to_string)
        .collect();
    if app_file.is_none() {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        out.extend(
            ricompiled::SOURCES
                .iter()
                .filter(|(key, _)| !root.join(key).is_file())
                .map(|(key, _)| format!("{key}: missing")),
        );
    }
    out
}

/// The command that puts back the tree the shell is built from, with a build of it.
pub const REBUILD_HINT: &str = "tools/install.sh";

/// Why `Rustshell.reload()` did not happen.
#[derive(Debug)]
pub enum Refused {
    /// The tree changed since the build and this build has no interpreter for the changed
    /// scripts: the re-executed shell would refuse to start, so the current one keeps running.
    Stale(Vec<String>),
    /// `exec` itself failed; the `Process` children and pending signals are as before.
    Exec(std::io::Error),
}

/// `Rustshell.reload()`: unless `check_stale` finds files the build cannot run, `flush` what
/// the exit path writes out before the process image goes (`exec` discards it as `exit` does)
/// and call `exec` with the variables the next image needs (it replaces the process and
/// returns only on failure): the running `Process` groups, which the next image kills, and
/// the termination signals not read yet. SIGTERM, SIGINT and SIGHUP are blocked for the exec
/// ([`crate::shutdown::block`]). Either way the current shell keeps running when this
/// returns, its children and signals as before.
pub fn reload(
    root: &Path,
    app_file: Option<&Path>,
    check_stale: bool,
    flush: impl FnOnce(),
    exec: impl FnOnce(&[(&'static str, String)]) -> std::io::Error,
) -> Refused {
    if check_stale {
        // The rule of the start (`differing_tree`): what it would refuse is refused here,
        // with the running shell still there.
        let stale = differing_tree(root, app_file);
        if !stale.is_empty() {
            return Refused::Stale(stale);
        }
    }
    flush();
    // From here to the next image's handlers a termination signal stays pending instead of
    // killing an image that has none yet, with the groups handed over below left running.
    let mask = crate::shutdown::block();
    let groups = qml::native::hand_over_children();
    let list = groups.iter().map(i32::to_string).collect::<Vec<_>>();
    let mut env = vec![(qml::native::HANDED_OVER_ENV, list.join(","))];
    env.extend(crate::shutdown::keep_across_exec());
    env.extend(crate::rilog::keep_across_exec());
    let err = exec(&env);
    crate::shutdown::close_on_exec();
    crate::rilog::close_on_exec();
    qml::native::take_back_children(&groups);
    crate::shutdown::restore(&mask);
    Refused::Exec(err)
}

/// Names to the next image the copy of this image's Wayland socket that [`keep_surfaces`]
/// left open across the exec of a reload.
pub const SURFACES_ENV: &str = "RI_RELOADED_SURFACES";

/// A copy of the Wayland socket `conn` that stays open across `exec`. While it is open the
/// compositor goes on showing this image's windows with their last frame, so the screen is
/// not left without the shell while the next image loads: a Quickshell reload keeps the old
/// windows until the new ones are there. The next image closes it ([`inherited_surfaces`]).
pub fn keep_surfaces(conn: std::os::fd::BorrowedFd) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: `conn` is open; F_DUPFD returns a descriptor of its own, without FD_CLOEXEC.
    let copy = unsafe { libc::fcntl(conn.as_raw_fd(), libc::F_DUPFD, 3) };
    // SAFETY: `copy` was just made and nothing else owns it.
    (copy >= 0).then(|| unsafe { std::os::fd::OwnedFd::from_raw_fd(copy) })
}

/// The socket [`SURFACES_ENV`] names (`value`), closed on exec from here on so that no
/// child of this image keeps the previous image's windows on the screen. `None` unless it
/// is an open socket. Dropping it takes those windows away.
pub fn inherited_surfaces(value: &str) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    let fd: libc::c_int = value.parse().ok().filter(|fd| *fd > 2)?;
    // SAFETY: a zeroed `stat` is a valid value of this plain C struct.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `st` is a `stat` to write to; a descriptor that is not open makes fstat fail.
    let known = unsafe { libc::fstat(fd, &mut st) } == 0;
    if !known || st.st_mode & libc::S_IFMT != libc::S_IFSOCK {
        return None;
    }
    // SAFETY: F_SETFD on the open descriptor `fd`.
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: `fd` is an open socket the previous image left to this one alone.
    Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}

/// How long the previous image's windows stay at most once this image has loaded and has not
/// drawn its own ([`inherited_surfaces`]): a window that never gets its first frame, or a
/// picture that never ends its load, must not keep them for good.
pub const PREDECESSOR_STAYS: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether the previous image's windows leave the screen now: every window of this image has
/// its first frame and no picture of it is still loading or fading in (`drawn`), or this
/// image has `waited` longer than [`PREDECESSOR_STAYS`] for that since its first turn.
pub fn predecessor_leaves(drawn: bool, waited: std::time::Duration) -> bool {
    drawn || waited > PREDECESSOR_STAYS
}

/// The descriptors this process has open, without the standard three.
pub fn open_descriptors() -> Vec<i32> {
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return Vec::new();
    };
    let names = dir.flatten().map(|e| e.file_name());
    let fds = names.filter_map(|n| n.to_str()?.parse().ok());
    fds.filter(|fd| *fd > 2).collect()
}

/// Closes on exec every one of `fds` that is not in `keep`: what a library opened without
/// FD_CLOEXEC (Mesa's software renderer does, two per start) would otherwise stay open in
/// every next image of a reload, two more each time.
pub fn close_on_exec_except(fds: &[i32], keep: &[i32]) {
    for fd in fds.iter().filter(|fd| !keep.contains(fd)) {
        // SAFETY: fcntl(2) F_GETFD / F_SETFD take plain integers; a closed `fd` fails.
        let flags = unsafe { libc::fcntl(*fd, libc::F_GETFD) };
        if flags >= 0 && flags & libc::FD_CLOEXEC == 0 {
            // SAFETY: as above.
            unsafe { libc::fcntl(*fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
        }
    }
}

/// The groups [`reload`] handed over, as [`qml::native::HANDED_OVER_ENV`] lists them.
pub fn handed_over(list: &str) -> Vec<i32> {
    list.split(',').filter_map(|p| p.parse().ok()).collect()
}

/// What `Rustshell.reloadFailed` says of a reload refused on a tree that differs from the
/// one compiled in: `differing` is [`differing_tree`]'s list.
pub fn stale_text(root: &Path, differing: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut text = format!(
        "{} differs from the tree this shell was built from. The running shell keeps its \
         current code.\n",
        root.display()
    );
    for s in differing.iter().take(SHOWN) {
        text.push_str(&format!("{s}\n"));
    }
    if differing.len() > SHOWN {
        text.push_str(&format!("and {} more\n", differing.len() - SHOWN));
    }
    text.push_str(&format!("Install again: {REBUILD_HINT}"));
    text
}

#[cfg(test)]
mod tests {
    use crate::shutdown::{SIGNALS, blocked};

    /// The exec of a reload runs with SIGTERM, SIGINT and SIGHUP blocked: one sent before the
    /// next image has its handlers stays pending instead of killing it. An exec that fails
    /// leaves the signals as they were.
    #[test]
    fn termination_signals_are_blocked_for_the_exec_only() {
        assert_eq!(SIGNALS.map(blocked), [false; 3]);
        let mut at_exec = None;
        let refused = super::reload(
            std::path::Path::new("/nonexistent"),
            None,
            false,
            || {},
            |_| {
                at_exec = Some(SIGNALS.map(blocked));
                std::io::Error::from_raw_os_error(libc::E2BIG)
            },
        );
        assert!(matches!(refused, super::Refused::Exec(_)), "{refused:?}");
        assert_eq!(at_exec, Some([true; 3]), "at the exec");
        assert_eq!(SIGNALS.map(blocked), [false; 3], "after the failed exec");
    }

    /// The copy of the Wayland socket for the next image stays open across an exec and keeps
    /// the connection up after the original closes; the next image takes it by number, closes
    /// it on exec for its own children, and ends the connection by dropping it. A number that
    /// names no socket is not taken.
    #[test]
    fn only_what_the_next_image_is_told_about_stays_open_across_the_exec() {
        use std::os::fd::{AsFd, AsRawFd};
        let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
        let (kept, left) = (
            super::keep_surfaces(a.as_fd()).unwrap(),
            super::keep_surfaces(b.as_fd()).unwrap(),
        );
        let open = super::open_descriptors();
        let ours = [a.as_raw_fd(), kept.as_raw_fd(), left.as_raw_fd()];
        assert!(ours.iter().all(|fd| open.contains(fd)), "{open:?}");
        assert!(!open.iter().any(|fd| *fd < 3), "{open:?}");
        super::close_on_exec_except(&ours, &[kept.as_raw_fd()]);
        // SAFETY: F_GETFD on a descriptor only reads its flags.
        let closes = |fd: i32| unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC != 0;
        assert!(!closes(kept.as_raw_fd()), "named to the next image");
        assert!(closes(left.as_raw_fd()), "left open by a library");
        assert!(closes(a.as_raw_fd()), "already closed on exec");
    }

    #[test]
    fn the_socket_copy_outlives_the_exec_and_ends_with_the_next_image() {
        use std::io::Read;
        use std::os::fd::{AsFd, AsRawFd, IntoRawFd};
        let (ours, mut compositor) = std::os::unix::net::UnixStream::pair().unwrap();
        let copy = super::keep_surfaces(ours.as_fd()).unwrap();
        // SAFETY: F_GETFD on an open descriptor.
        let flags = |fd: i32| unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert_eq!(
            flags(copy.as_raw_fd()) & libc::FD_CLOEXEC,
            0,
            "open across exec"
        );
        drop(ours);
        compositor.set_nonblocking(true).unwrap();
        let mut byte = [0u8; 1];
        let closed = |r: std::io::Result<usize>| matches!(r, Ok(0));
        assert!(
            !closed(compositor.read(&mut byte)),
            "the copy keeps it open"
        );

        let number = copy.into_raw_fd().to_string();
        let taken = super::inherited_surfaces(&number).unwrap();
        assert_ne!(
            flags(taken.as_raw_fd()) & libc::FD_CLOEXEC,
            0,
            "closed on exec"
        );
        drop(taken);
        assert!(closed(compositor.read(&mut byte)), "the connection ended");

        let file = std::fs::File::open("/dev/null").unwrap();
        for not_a_socket in [file.as_raw_fd().to_string(), number, "1".into(), "x".into()] {
            assert!(
                super::inherited_surfaces(&not_a_socket).is_none(),
                "{not_a_socket}"
            );
        }
    }

    /// The previous image's windows leave as soon as this image has drawn its own, however
    /// soon that is. An image that has not drawn keeps them through [`PREDECESSOR_STAYS`] and
    /// not past it.
    #[test]
    fn the_previous_images_windows_leave_at_the_first_frames_or_past_the_limit() {
        use super::{PREDECESSOR_STAYS, predecessor_leaves};
        let zero = std::time::Duration::ZERO;
        let past = PREDECESSOR_STAYS + std::time::Duration::from_nanos(1);
        assert!(predecessor_leaves(true, zero), "drawn at once");
        assert!(predecessor_leaves(true, past), "drawn late");
        assert!(!predecessor_leaves(false, zero), "not drawn yet");
        assert!(
            !predecessor_leaves(false, PREDECESSOR_STAYS),
            "at the limit"
        );
        assert!(predecessor_leaves(false, past), "never drawn");
    }
}
