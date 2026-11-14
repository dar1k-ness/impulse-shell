//! A program started detached, as `QProcess::startDetached` starts it (`Rustshell.execDetached`,
//! `Process.startDetached()`, `DesktopEntry.execute`, `xdg-open`): in a session of its own
//! (setsid(2)), where it also leads its own process group, so a signal sent to the shell's
//! group or session (Ctrl+C in its terminal, `kill -- -pgid`) does not reach it. Its stdin,
//! stdout and stderr are `/dev/null`.
//!
//! posix_spawn(3) with `POSIX_SPAWN_SETSID` starts it: std's `Command` has no stable way to ask
//! for a session, and a `pre_exec` hook would fork the shell's whole address space. Unlike
//! `QProcess::startDetached`, which forks twice, the program stays this process's child: the
//! caller reaps it ([`workpool::adopt`]), and the next image does after a reload.
use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;

/// What to start: Quickshell's `ProcessContext`, resolved.
pub(crate) struct Launch {
    /// The command line; `argv[0]` is the program as it was named.
    pub argv: Vec<String>,
    /// The executable `argv[0]` names; `None` when there is no such program.
    pub exe: Option<std::path::PathBuf>,
    /// The program's whole environment.
    pub env: Vec<(OsString, OsString)>,
    /// Its working directory; the shell's when `None` or empty.
    pub cwd: Option<String>,
}

struct FileActions(libc::posix_spawn_file_actions_t);

impl Drop for FileActions {
    fn drop(&mut self) {
        // SAFETY: `self.0` was initialized by posix_spawn_file_actions_init(3).
        unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
    }
}

struct Attr(libc::posix_spawnattr_t);

impl Drop for Attr {
    fn drop(&mut self) {
        // SAFETY: `self.0` was initialized by posix_spawnattr_init(3).
        unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
    }
}

/// The posix_spawn family returns the error number instead of setting errno.
fn check(r: libc::c_int) -> std::io::Result<()> {
    if r == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(r))
    }
}

fn cstring(b: &[u8]) -> std::io::Result<CString> {
    CString::new(b).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a NUL byte in the command",
        )
    })
}

/// The NULL-terminated pointer array execve(2) takes; `strs` must outlive it.
fn pointers(strs: &[CString]) -> Vec<*mut libc::c_char> {
    strs.iter()
        .map(|s| s.as_ptr().cast_mut())
        .chain([std::ptr::null_mut()])
        .collect()
}

impl Launch {
    /// `argv` with the user's environment ([`workpool::userenv`]), in `cwd`: what
    /// `Rustshell.execDetached` gives a program.
    pub(crate) fn inherit(argv: &[String], cwd: Option<String>) -> Launch {
        Launch {
            exe: argv.first().and_then(|p| crate::native::resolve(p, None)),
            argv: argv.to_vec(),
            env: workpool::userenv::vars(),
            cwd,
        }
    }

    /// `argv` with this process's environment, the config's pragmas applied: what a
    /// `QProcess` Qt starts itself gives a program.
    pub(crate) fn own_env(argv: &[String]) -> Launch {
        Launch {
            env: std::env::vars_os().collect(),
            ..Launch::inherit(argv, None)
        }
    }

    /// Starts the program and returns its pid; the caller reaps it. An error is the one a
    /// failed `chdir` or `execve` in the child reported.
    pub(crate) fn spawn(&self) -> std::io::Result<i32> {
        let Some(exe) = &self.exe else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "program not found",
            ));
        };
        let exe = cstring(exe.as_os_str().as_bytes())?;
        let args = self
            .argv
            .iter()
            .map(|a| cstring(a.as_bytes()))
            .collect::<Result<Vec<_>, _>>()?;
        let env = self
            .env
            .iter()
            .map(|(k, v)| cstring(&[k.as_bytes(), b"=", v.as_bytes()].concat()))
            .collect::<Result<Vec<_>, _>>()?;
        let cwd = self
            .cwd
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| cstring(d.as_bytes()))
            .transpose()?;
        let (argv, envp) = (pointers(&args), pointers(&env));

        // SAFETY: zeroed storage for posix_spawn_file_actions_init(3) to initialize.
        let mut fa = FileActions(unsafe { std::mem::zeroed() });
        // SAFETY: `fa.0` is that storage; from here `FileActions::drop` destroys it.
        check(unsafe { libc::posix_spawn_file_actions_init(&mut fa.0) })?;
        for (fd, mode) in [
            (0, libc::O_RDONLY),
            (1, libc::O_WRONLY),
            (2, libc::O_WRONLY),
        ] {
            // SAFETY: `fa.0` is initialized and the path is a NUL-terminated literal, copied.
            check(unsafe {
                libc::posix_spawn_file_actions_addopen(
                    &mut fa.0,
                    fd,
                    c"/dev/null".as_ptr(),
                    mode,
                    0,
                )
            })?;
        }
        if let Some(d) = &cwd {
            // SAFETY: `fa.0` is initialized and `d` is a NUL-terminated string, copied.
            check(unsafe { libc::posix_spawn_file_actions_addchdir_np(&mut fa.0, d.as_ptr()) })?;
        }

        // SAFETY: zeroed storage for posix_spawnattr_init(3) to initialize.
        let mut attr = Attr(unsafe { std::mem::zeroed() });
        // SAFETY: `attr.0` is that storage; from here `Attr::drop` destroys it.
        check(unsafe { libc::posix_spawnattr_init(&mut attr.0) })?;
        // As std's `Command`: the program starts with no signal blocked and with SIGPIPE,
        // which this process ignores, back at its default action.
        // SAFETY: zeroed storage for sigemptyset(3) to initialize.
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: `set` is a live sigset_t.
        unsafe { libc::sigemptyset(&mut set) };
        // SAFETY: `attr.0` is an initialized attributes object and `set` a live sigset_t.
        check(unsafe { libc::posix_spawnattr_setsigmask(&mut attr.0, &set) })?;
        // SAFETY: `set` is a live, initialized sigset_t and SIGPIPE a valid signal.
        unsafe { libc::sigaddset(&mut set, libc::SIGPIPE) };
        // SAFETY: `attr.0` is an initialized attributes object and `set` a live sigset_t.
        check(unsafe { libc::posix_spawnattr_setsigdefault(&mut attr.0, &set) })?;
        let flags = libc::c_int::from(libc::POSIX_SPAWN_SETSID)
            | libc::POSIX_SPAWN_SETSIGMASK
            | libc::POSIX_SPAWN_SETSIGDEF;
        // SAFETY: `attr.0` is initialized; the flags are plain integers.
        check(unsafe { libc::posix_spawnattr_setflags(&mut attr.0, flags as libc::c_short) })?;

        let mut pid: libc::pid_t = 0;
        // SAFETY: `exe` is NUL-terminated; `argv` and `envp` are NULL-terminated arrays of
        // NUL-terminated strings kept alive by `args` and `env`; `fa.0` and `attr.0` are
        // initialized; `pid` is writable.
        check(unsafe {
            libc::posix_spawn(
                &mut pid,
                exe.as_ptr(),
                &fa.0,
                &attr.0,
                argv.as_ptr(),
                envp.as_ptr(),
            )
        })?;
        Ok(pid)
    }

    /// [`spawn`](Launch::spawn) for `what` (the QML call); a failure is logged with the
    /// command line, as QProcess reports `FailedToStart`.
    pub(crate) fn start(&self, what: &str) -> Option<i32> {
        match self.spawn() {
            Ok(pid) => Some(pid),
            Err(e) => {
                elog::eprintln!("{what} {:?}: failed to start: {e}", self.argv);
                None
            }
        }
    }
}
