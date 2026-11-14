//! Waits on processes through their pidfds (pidfd_open(2)): any process, not only the test's
//! own children, and immune to pid reuse.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};

/// Blocks in poll(2) until `fd` is readable. No deadline of its own: a hang is nextest's
/// `slow-timeout` to end.
pub(crate) fn wait_readable(fd: &OwnedFd) {
    loop {
        let mut p = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll(2) reads and writes only the one pollfd it is given.
        if unsafe { libc::poll(&mut p, 1, -1) } == 1 {
            return;
        }
        let e = std::io::Error::last_os_error();
        assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
    }
}

/// A process by its pidfd (pidfd_open(2)), any process, not only a child of the test: the fd
/// names that process even after its pid is reused, and turns readable once it terminated.
pub struct PidFd(OwnedFd);

impl PidFd {
    /// Opens `pid`'s pidfd; open it while `pid` surely names the process meant (alive, or a
    /// zombie nobody reaped yet).
    pub fn open(pid: i32) -> PidFd {
        // SAFETY: pidfd_open(2) takes a pid and flags and returns a new descriptor or -1.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        assert!(
            fd >= 0,
            "pidfd_open({pid}): {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `fd` is the new descriptor pidfd_open returned; nothing else owns it.
        PidFd(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
    }

    /// Opens `pid`'s pidfd, or None if it is gone already: ESRCH once it was reaped, EINVAL while
    /// it is being released (its pid no longer has a thread group).
    pub fn open_if_alive(pid: i32) -> Option<PidFd> {
        // SAFETY: pidfd_open(2) takes a pid and flags and returns a new descriptor or -1.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            assert!(
                matches!(e.raw_os_error(), Some(libc::ESRCH | libc::EINVAL)),
                "pidfd_open({pid}): {e}"
            );
            return None;
        }
        // SAFETY: `fd` is the new descriptor pidfd_open returned; nothing else owns it.
        Some(PidFd(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) }))
    }

    /// SIGKILLs the process through its pidfd, so a reused pid is never signalled.
    pub fn kill(&self) {
        // SAFETY: pidfd_send_signal(2) takes a pidfd, a signal, a null siginfo and flags.
        let r = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if r < 0 {
            let e = std::io::Error::last_os_error();
            // ESRCH: it has exited already.
            assert_eq!(
                e.raw_os_error(),
                Some(libc::ESRCH),
                "pidfd_send_signal: {e}"
            );
        }
    }

    /// Blocks until the process terminated.
    pub fn wait_exit(&self) {
        wait_readable(&self.0);
    }

    /// Whether the process terminated (a zombie counts), without waiting: what
    /// [`PidFd::wait_exit`] waits for.
    pub fn exited(&self) -> bool {
        let mut p = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll(2) reads and writes only the one pollfd it is given.
        let n = unsafe { libc::poll(&mut p, 1, 0) };
        assert!(n >= 0, "poll: {}", std::io::Error::last_os_error());
        n == 1
    }
}

/// Blocks until process `pid` terminated (a zombie counts); returns at once if it is gone
/// already. No deadline of its own: a process that never exits leaves the test to nextest's
/// slow-timeout.
pub fn wait_gone(pid: i32) {
    if let Some(p) = PidFd::open_if_alive(pid) {
        p.wait_exit();
    }
}

/// Whether process `pid` terminated (a zombie counts) or is gone already: asked of its pidfd, as
/// [`wait_gone`] waits. Not the state letter of `/proc/<pid>/stat`: between the `Z` of a zombie
/// and the file going, while its parent reaps it, that letter is `X`.
pub fn terminated(pid: i32) -> bool {
    PidFd::open_if_alive(pid).is_none_or(|p| p.exited())
}

/// [`wait_gone`] for each of `pids`.
pub fn wait_all_gone(pids: &[i32]) {
    for &p in pids {
        wait_gone(p);
    }
}

/// SIGKILLs process `pid` (one the test did not spawn) and blocks until it exited, both through
/// its pidfd; returns at once if it is already gone.
pub fn kill_pid_and_wait(pid: i32) {
    if let Some(p) = PidFd::open_if_alive(pid) {
        p.kill();
        p.wait_exit();
    }
}

/// `cmd.output()`, returning only once every process `cmd` started has exited too, as a shell's
/// `wait` cannot: the command gets the write end of a pipe as fd 3, which every descendant
/// inherits (redirecting stdio or `disown` keeps it), and the pipe reads end-of-file once the
/// last of them closed it. No deadline of its own: a hang is nextest's `slow-timeout` to end.
pub fn output_with_descendants(
    cmd: &mut std::process::Command,
) -> std::io::Result<std::process::Output> {
    use std::os::unix::process::CommandExt;
    let (mut ended, open) = std::io::pipe()?;
    let w = open.as_raw_fd();
    let hook = move || {
        // dup2 onto itself would keep close-on-exec: clear the flag instead.
        let r = if w == 3 {
            // SAFETY: fcntl(2) F_SETFD on a descriptor open in the forked child.
            unsafe { libc::fcntl(3, libc::F_SETFD, 0) }
        } else {
            // SAFETY: dup2(2) between two descriptor numbers; `w` is open in the forked child.
            unsafe { libc::dup2(w, 3) }
        };
        if r < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    // SAFETY: the hook runs in the forked child before exec and only calls dup2(2)/fcntl(2),
    // which are async-signal-safe, and allocates nothing.
    unsafe { cmd.pre_exec(hook) };
    let out = cmd.output();
    drop(open);
    let mut rest = Vec::new();
    ended.read_to_end(&mut rest)?;
    out
}

/// A running `cat` (or a renamed copy of it, maybe under another argv[0]) that blocks reading
/// its stdin until killed. [`Running::spawn`] returns once it echoed a line: it then runs its
/// own code, so its comm, argv and exe are what pidof/pkill see (`Command::spawn` returns at
/// the exec, before the kernel sets comm and the argument pages). Killed and reaped on drop.
pub struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    end: Option<ExitStatus>,
}

impl Running {
    /// Starts `cmd` (a `cat` reading stdin and writing stdout) and waits until it echoed.
    #[track_caller]
    pub fn spawn(cmd: &mut Command) -> Running {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {cmd:?}: {e}"));
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut r = Running {
            child,
            stdin,
            stdout,
            end: None,
        };
        if let Err(st) = r.alive() {
            panic!("{cmd:?} exited before it ran ({st})");
        }
        r
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Whether it still runs: a line written to it comes back. Run after whatever might have
    /// signalled it returned from kill(2): a fatal signal then pending ends it before it reads
    /// the line, so the answer does not depend on how soon it dies. `Err` is how it ended
    /// (it is reaped then).
    pub fn alive(&mut self) -> Result<(), ExitStatus> {
        if let Some(st) = self.end {
            return Err(st);
        }
        let sent = self
            .stdin
            .as_mut()
            .is_some_and(|i| i.write_all(b"ping\n").and_then(|()| i.flush()).is_ok());
        let mut line = String::new();
        if sent && self.stdout.read_line(&mut line).unwrap_or(0) > 0 {
            return Ok(());
        }
        self.stdin = None;
        let st = crate::wait_exit(&mut self.child);
        self.end = Some(st);
        Err(st)
    }

    /// Kills it (SIGKILL) and reaps it.
    pub fn kill(mut self) {
        self.stop();
    }

    /// Kills the child and returns once no process of it is left for pidof to list: the child
    /// is reaped, and its output read to end-of-file, which comes when the last process holding
    /// it let go of it. The child may be a wrapper (`unshare --fork --kill-child`) whose own
    /// child, the process the test looks for, is killed only as the wrapper dies and outlives
    /// it for a moment; a killed process drops its memory (its /proc/PID/cmdline turns empty,
    /// which pidof passes by) before its descriptors. Its input stays open until then: a `cat`
    /// that read end-of-file instead closes its output itself, and is still listed after that.
    fn stop(&mut self) {
        if self.end.is_none() {
            let _ = self.child.kill();
            self.end = Some(crate::wait_exit(&mut self.child));
            let _ = std::io::copy(&mut self.stdout, &mut std::io::sink());
            self.stdin = None;
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The absolute path of `tool` on PATH, as `command -v` finds it, else in /usr/bin or /bin (a
/// test may run with a PATH of its fakes only).
#[track_caller]
pub fn tool_path(tool: &str) -> PathBuf {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':')
        .chain(["/usr/bin", "/bin"])
        .map(|d| Path::new(if d.is_empty() { "." } else { d }).join(tool))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("`{tool}` not found"))
}

/// A copy of `cat` at `path` (made by cp(1), so no fd of this process is open on it while a
/// sibling test forks: running it would fail with ETXTBSY), named for the process it plays.
#[track_caller]
pub fn cat_copy(path: &Path) -> PathBuf {
    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let st = Command::new(tool_path("cp"))
            .arg(tool_path("cat"))
            .arg(path)
            .status()
            .expect("run cp(1)");
        assert!(st.success(), "cp cat {}: {st}", path.display());
    }
    path.to_path_buf()
}

/// [`crate::flatpak_sandbox`] whose app is `exec -a argv0 cat`, returned once it runs (echoed a
/// line through the sandbox), so pidof and its `/.flatpak-info` see it. Killing it kills the
/// sandbox (`unshare --kill-child`); when the app dies `unshare` exits too, closing its output.
#[track_caller]
pub fn flatpak_sandbox_running(dir: &Path, app: &str, argv0: &str) -> Option<Running> {
    const SETUP: &str = r#"set -e
mount -t tmpfs tmpfs "$R"
mkdir "$R/usr" "$R/tmp"
mount --rbind /usr "$R/usr"
ln -s usr/bin "$R/bin"
ln -s usr/lib "$R/lib"
ln -s usr/lib64 "$R/lib64"
printf '[Application]\nname=%s\n' "$APP" > "$R/.flatpak-info"
exec chroot "$R" /bin/bash -c 'exec -a "$0" cat' "$ARGV0"
"#;
    let ok = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "true",
        ])
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        crate::equiv_missing("unprivileged user namespaces (`unshare --user --map-root-user`)");
        return None;
    }
    Some(Running::spawn(
        Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--pid",
                "--fork",
                "--kill-child",
                "sh",
                "-c",
                SETUP,
            ])
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("R", dir)
            .env("APP", app)
            .env("ARGV0", argv0),
    ))
}

/// Installs at `path` a program the code under test starts (`pidof P || P`): a bash script that
/// runs `prelude` (e.g. logs its argv), starts the long-running process P in the background and
/// exits once P runs. P is a copy of `cat` named like `path` (in `path`'s directory under
/// `.daemons/`) with argv[0] `$0`, blocked reading a FIFO it holds open itself, so it runs until
/// killed. So pidof sees P as soon as the starter has exited, and the starter (not P) is the
/// child its caller reaps.
#[track_caller]
pub fn install_daemon(path: &Path, prelude: &str) {
    let dir = path.parent().unwrap().join(".daemons");
    let real = cat_copy(&dir.join(path.file_name().unwrap()));
    let body = format!(
        r#"#!/bin/bash
{prelude}
d='{dir}'; f="$d/in.$$"; o="$d/out.$$"
'{mkfifo}' "$f" "$o"
( exec -a "$0" '{real}' <>"$f" >"$o" 2>/dev/null ) &
exec 4<"$o"
echo up >"$f"
read -r _ <&4
'{rm}' -f "$f" "$o"
"#,
        dir = dir.display(),
        real = real.display(),
        mkfifo = tool_path("mkfifo").display(),
        rm = tool_path("rm").display(),
    );
    crate::install_script(path, &body);
}

/// From here on the system refuses this thread every new thread, the way it does at the limit
/// on threads (`RLIMIT_NPROC`, `threads-max`): `pthread_create` fails with `EAGAIN`. New
/// processes still start. It holds for the calling thread until it ends, and for the processes
/// it starts; it cannot be undone, so the test's own thread is the one to call it.
///
/// A seccomp filter does it, so a process started afterwards also gains no privileges from a
/// setuid program (`PR_SET_NO_NEW_PRIVS`, which an unprivileged filter needs). The limits
/// themselves cannot: the user's other processes count towards `RLIMIT_NPROC`, and it refuses
/// new processes too.
pub fn refuse_new_threads() {
    let stmt = |code: u32, k: u32| libc::sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    };
    let jump = |code: u32, k: u32, jt: u8, jf: u8| libc::sock_filter {
        code: (libc::BPF_JMP | code | libc::BPF_K) as u16,
        jt,
        jf,
        k,
    };
    let load = libc::BPF_LD | libc::BPF_W | libc::BPF_ABS;
    let ret = libc::BPF_RET | libc::BPF_K;
    let errno = |e: i32| libc::SECCOMP_RET_ERRNO | e as u32;
    // seccomp_data: the system call's number at offset 0, its first argument at 16 (its low
    // half first on a little-endian machine): clone(2)'s flags, on x86-64 and aarch64 alike.
    let filter = [
        stmt(load, 0),
        // glibc starts a thread with clone3(2), whose flags a filter cannot read (they are
        // behind a pointer), and falls back to clone(2) where the kernel has no clone3.
        jump(libc::BPF_JEQ, libc::SYS_clone3 as u32, 0, 1),
        stmt(ret, errno(libc::ENOSYS)),
        jump(libc::BPF_JEQ, libc::SYS_clone as u32, 0, 3),
        stmt(load, 16),
        jump(libc::BPF_JSET, libc::CLONE_THREAD as u32, 0, 1),
        stmt(ret, errno(libc::EAGAIN)),
        stmt(ret, libc::SECCOMP_RET_ALLOW),
    ];
    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: prctl(2) with integer arguments only.
    let r = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    assert_eq!(
        r,
        0,
        "PR_SET_NO_NEW_PRIVS: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: `prog` points to the `len` instructions of `filter`, which the kernel copies
    // during the call and does not write to.
    let r = unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &raw const prog,
        )
    };
    assert_eq!(r, 0, "PR_SET_SECCOMP: {}", std::io::Error::last_os_error());
}

/// Runs `f` and returns what was written to this process's stderr (descriptor 2) meanwhile:
/// the log lines of the code under test. Nothing reaches the real stderr during `f`, except
/// that a panic in `f` is passed on with everything written before it. It redirects the whole
/// process's stderr, so the caller is a test in a process of its own
/// ([`crate::run_in_own_process`]).
pub fn stderr_of(f: impl FnOnce()) -> String {
    use std::io::Seek as _;
    let mut log = tempfile::tempfile().expect("a temporary file for stderr");
    // SAFETY: dup(2) of this process's open stderr.
    let stderr = unsafe { libc::dup(2) };
    assert!(stderr >= 0, "dup: {}", std::io::Error::last_os_error());
    // SAFETY: dup2(2) between two open descriptors; `log` stays open for the call.
    assert_eq!(unsafe { libc::dup2(log.as_raw_fd(), 2) }, 2);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    // SAFETY: dup2(2) between two open descriptors, putting stderr back.
    assert_eq!(unsafe { libc::dup2(stderr, 2) }, 2);
    // SAFETY: close(2) of the copy made above, used by nothing else.
    unsafe { libc::close(stderr) };
    let mut text = String::new();
    log.rewind().expect("rewind");
    log.read_to_string(&mut text)
        .expect("read the captured stderr");
    if let Err(panic) = r {
        elog::eprint!("{text}");
        std::panic::resume_unwind(panic);
    }
    text
}

/// [`stderr_of`], beside what `f` returns.
pub fn stderr_with<R>(f: impl FnOnce() -> R) -> (R, String) {
    let mut r = None;
    let text = stderr_of(|| r = Some(f()));
    (r.expect("f returned"), text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn running_tells_alive_from_signalled() {
        let mut a = super::Running::spawn(&mut std::process::Command::new("cat"));
        let mut b = super::Running::spawn(&mut std::process::Command::new("cat"));
        // SAFETY: kill(2) on our own unreaped child's pid.
        assert_eq!(unsafe { libc::kill(b.id() as i32, libc::SIGTERM) }, 0);
        assert_eq!(a.alive(), Ok(()));
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(b.alive().unwrap_err().signal(), Some(libc::SIGTERM));
    }

    /// `kill` returns once the sandboxed app is gone too, not only the `unshare` that wraps it
    /// and dies first: a test asks pidof next.
    #[test]
    fn killed_sandbox_leaves_no_process_listed() {
        const ARGV0: &str = "ri-tenv-sandboxed";
        let listed = || -> Vec<i32> {
            std::fs::read_dir("/proc")
                .unwrap()
                .flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
                .filter(|pid| {
                    std::fs::read(format!("/proc/{pid}/cmdline"))
                        .is_ok_and(|c| c.split(|&b| b == 0).next() == Some(ARGV0.as_bytes()))
                })
                .collect()
        };
        let d = tempfile::tempdir().unwrap();
        for _ in 0..20 {
            let Some(app) = super::flatpak_sandbox_running(d.path(), "org.example.App", ARGV0)
            else {
                return;
            };
            assert_eq!(listed().len(), 1, "the sandboxed app runs");
            app.kill();
            assert_eq!(listed(), [], "the sandboxed app outlived `kill`");
        }
    }

    #[test]
    fn daemon_runs_once_its_starter_exited() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("ri-tenv-daemon");
        super::install_daemon(&p, "");
        let st = std::process::Command::new(&p)
            .stdin(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(st.success());
        let me = std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
            .find(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| {
                    c.split(|&b| b == 0).next() == Some(p.as_os_str().as_encoded_bytes())
                })
            })
            .expect("the daemon runs");
        // SAFETY: kill(2) takes plain integers.
        unsafe { libc::kill(me, libc::SIGKILL) };
        super::wait_gone(me);
    }

    #[test]
    fn stderr_of_returns_what_was_written_and_puts_stderr_back() {
        const TEST: &str = concat!(
            module_path!(),
            "::stderr_of_returns_what_was_written_and_puts_stderr_back"
        );
        if !crate::in_own_process(TEST) {
            crate::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let before = std::fs::read_link("/proc/self/fd/2").unwrap();
        let text = super::stderr_of(|| elog::eprintln!("a line"));
        assert_eq!(text, "a line\n");
        assert_eq!(std::fs::read_link("/proc/self/fd/2").unwrap(), before);
    }

    #[test]
    fn wait_gone_wakes_on_a_non_child_exit() {
        // `sh` forks the sleeper and exits: the sleeper is not this process's child.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("pid");
        crate::fifo(&f);
        let mut sh = std::process::Command::new("sh")
            .args(["-c", "sleep infinity & echo $! > \"$0\""])
            .arg(&f)
            .spawn()
            .unwrap();
        let pid: i32 = std::fs::read_to_string(&f).unwrap().trim().parse().unwrap();
        crate::wait_exit(&mut sh);
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        super::wait_gone(pid);
        super::wait_all_gone(&[pid, pid]);
    }

    #[test]
    fn terminated_tells_a_running_process_from_a_zombie_and_from_one_reaped() {
        let mut sleeper = std::process::Command::new("sleep")
            .arg("infinity")
            .spawn()
            .unwrap();
        let pid = sleeper.id() as i32;
        assert!(!super::terminated(pid));
        sleeper.kill().unwrap();
        // Not reaped: a zombie.
        super::wait_gone(pid);
        assert!(super::terminated(pid));
        crate::wait_exit(&mut sleeper);
        assert!(super::terminated(pid));
    }

    /// Under [`super::refuse_new_threads`] a new thread is refused with `EAGAIN`, as at the
    /// limit on threads, while a new process still starts; a thread that did not call it is
    /// unaffected.
    #[test]
    fn refuse_new_threads_refuses_threads_only() {
        let refused = std::thread::spawn(|| {
            super::refuse_new_threads();
            let e = std::thread::Builder::new().spawn(|| ()).unwrap_err();
            let ran = std::process::Command::new("true").status().unwrap();
            (e.raw_os_error(), ran.success())
        });
        assert_eq!(refused.join().unwrap(), (Some(libc::EAGAIN), true));
        std::thread::spawn(|| ()).join().unwrap();
    }
}
