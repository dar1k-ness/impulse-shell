//! Native `Rustshell.Services.Pam`: every `PamContext` conversation runs in a helper process,
//! as in quickshell, so a hung PAM module (fingerprint reader, hardware key) never blocks the
//! shell and `abort()` can always kill it. libpam through the [`ffi`] declarations.
//!
//! The helper is a freshly executed image (the shell binary run with [`ARG`], started through
//! `std::process::Command`, i.e. posix_spawn), never a bare `fork()` of the shell: after
//! `fork()` in a multithreaded process POSIX allows only async-signal-safe calls until `exec`,
//! and libpam dlopens modules, allocates and (pam_systemd, pam_fprintd) talks D-Bus. The two
//! processes talk over one `AF_UNIX` socket, the helper's stdin.

use crate::runtime::{Runtime, World};
use crate::value::Value;
use ffi::{
    PAM_AUTH_ERR, PAM_BUF_ERR, PAM_CONV_ERR, PAM_ERROR_MSG, PAM_MAXTRIES, PAM_PROMPT_ECHO_OFF,
    PAM_PROMPT_ECHO_ON, PAM_SUCCESS, pam_conv, pam_handle_t, pam_message, pam_response,
};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_int, c_void};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::mpsc::Receiver;

/// The part of the Linux-PAM application API this module uses, declared from
/// `<security/pam_appl.h>` and `<security/_pam_types.h>` (Linux-PAM's stable C ABI; pam_start(3),
/// pam_authenticate(3), pam_end(3), pam_conv(3)). `ffi_matches_header` checks it against the
/// installed headers.
#[allow(non_camel_case_types)]
mod ffi {
    use std::ffi::{c_char, c_int, c_void};

    pub const PAM_SUCCESS: c_int = 0;
    pub const PAM_BUF_ERR: c_int = 5;
    pub const PAM_AUTH_ERR: c_int = 7;
    pub const PAM_MAXTRIES: c_int = 11;
    pub const PAM_CONV_ERR: c_int = 19;

    pub const PAM_PROMPT_ECHO_OFF: c_int = 1;
    pub const PAM_PROMPT_ECHO_ON: c_int = 2;
    pub const PAM_ERROR_MSG: c_int = 3;

    /// `pam_handle_t`, opaque to applications.
    #[repr(C)]
    pub struct pam_handle_t {
        _opaque: [u8; 0],
        _not_send_sync: std::marker::PhantomData<(*mut u8, std::marker::PhantomPinned)>,
    }

    #[repr(C)]
    pub struct pam_message {
        pub msg_style: c_int,
        pub msg: *const c_char,
    }

    #[repr(C)]
    pub struct pam_response {
        pub resp: *mut c_char,
        /// Unused by Linux-PAM; must be 0.
        pub resp_retcode: c_int,
    }

    #[repr(C)]
    pub struct pam_conv {
        pub conv: Option<
            unsafe extern "C" fn(
                num_msg: c_int,
                msg: *mut *const pam_message,
                resp: *mut *mut pam_response,
                appdata_ptr: *mut c_void,
            ) -> c_int,
        >,
        pub appdata_ptr: *mut c_void,
    }

    #[link(name = "pam")]
    unsafe extern "C" {
        /// Linux-PAM 1.4.0 and later.
        pub fn pam_start_confdir(
            service_name: *const c_char,
            user: *const c_char,
            pam_conversation: *const pam_conv,
            confdir: *const c_char,
            pamh: *mut *mut pam_handle_t,
        ) -> c_int;
        pub fn pam_authenticate(pamh: *mut pam_handle_t, flags: c_int) -> c_int;
        pub fn pam_end(pamh: *mut pam_handle_t, pam_status: c_int) -> c_int;
    }
}

// `PamResult` / `PamError` values (quickshell conversation.hpp).
pub(crate) const RESULT_SUCCESS: f64 = 0.0;
const RESULT_FAILED: f64 = 1.0;
pub(crate) const RESULT_ERROR: f64 = 2.0;
const RESULT_MAX_TRIES: f64 = 3.0;
const ERROR_START_FAILED: f64 = 1.0;
const ERROR_TRY_AUTH_FAILED: f64 = 2.0;
const ERROR_INTERNAL: f64 = 3.0;

/// `PamResult.toString()`.
pub(crate) fn result_string(v: f64) -> &'static str {
    match v as i64 {
        0 => "Success",
        1 => "Failed",
        2 => "Error occurred while authenticating",
        3 => "The authentication method has no more attempts available",
        _ => "Invalid result",
    }
}

/// `PamError.toString()`.
pub(crate) fn error_string(v: f64) -> &'static str {
    match v as i64 {
        1 => "Failed to start the PAM session",
        2 => "Failed to try authenticating",
        3 => "Internal error occurred",
        _ => "Invalid error",
    }
}

// Child → parent events and exit codes (quickshell ipc.hpp).
const EV_REQUEST: u8 = 0;
const EV_EXIT: u8 = 1;
const EXIT_SUCCESS: u8 = 0;
const EXIT_START_FAILED: u8 = 1;
const EXIT_AUTH_FAILED: u8 = 2;
const EXIT_MAX_TRIES: u8 = 3;
const EXIT_PAM_ERROR: u8 = 4;
const EXIT_OTHER_ERROR: u8 = 5;

/// Overwrite secret bytes so they don't linger in freed memory.
pub(crate) fn zero(b: &mut [u8]) {
    for x in b.iter_mut() {
        // SAFETY: `x` is a `&mut u8`: valid and aligned for a one-byte write.
        unsafe { std::ptr::write_volatile(x, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn zero_string(s: String) {
    let mut b = s.into_bytes();
    zero(&mut b);
}

/// A child process handle that can't signal a recycled pid (signals go through its pidfd).
pub(crate) struct ChildRef {
    pub pid: libc::pid_t,
    pidfd: OwnedFd,
}

impl ChildRef {
    /// `pid` must be an unreaped child of this process.
    pub fn new(pid: libc::pid_t) -> std::io::Result<ChildRef> {
        Ok(ChildRef {
            pid,
            pidfd: workpool::pidfd_open(pid)?,
        })
    }

    pub fn signal(&self, sig: c_int) {
        // SAFETY: the pidfd is owned by `self`, open for the call; pidfd_send_signal(2) takes a
        // null siginfo and flags 0 for a plain kill(2)-like signal.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                sig,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    }
}

fn fd_write_all(fd: RawFd, mut b: &[u8]) -> bool {
    while !b.is_empty() {
        // SAFETY: `b` is `b.len()` readable bytes, alive for the call.
        let n = unsafe { libc::write(fd, b.as_ptr().cast(), b.len()) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if n <= 0 {
            return false;
        }
        b = &b[n as usize..];
    }
    true
}

fn fd_read_exact(fd: RawFd, mut b: &mut [u8]) -> bool {
    while !b.is_empty() {
        // SAFETY: `b` is `b.len()` writable bytes, alive for the call.
        let n = unsafe { libc::read(fd, b.as_mut_ptr().cast(), b.len()) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if n <= 0 {
            return false;
        }
        b = &mut b[n as usize..];
    }
    true
}

fn fd_write_str(fd: RawFd, s: &[u8]) -> bool {
    fd_write_all(fd, &(s.len() as u32).to_ne_bytes()) && fd_write_all(fd, s)
}

/// One length-prefixed string. The buffer has room for a NUL after it: a response becomes a C
/// string in place, so the secret is never copied to a grown buffer and freed unwiped.
fn fd_read_str(fd: RawFd) -> Option<Vec<u8>> {
    let mut n = [0u8; 4];
    if !fd_read_exact(fd, &mut n) {
        return None;
    }
    let n = u32::from_ne_bytes(n) as usize;
    let mut b = Vec::with_capacity(n + 1);
    b.resize(n, 0u8);
    if !fd_read_exact(fd, &mut b) {
        zero(&mut b);
        return None;
    }
    Some(b)
}

/// The helper's end of the socket.
struct ChildIo {
    fd: RawFd,
}

/// The PAM conversation inside the helper: relay each message, wait for responses.
unsafe extern "C" fn conversation(
    n: c_int,
    msgs: *mut *const pam_message,
    resp: *mut *mut pam_response,
    data: *mut c_void,
) -> c_int {
    if n <= 0 || msgs.is_null() || resp.is_null() || data.is_null() {
        return PAM_CONV_ERR;
    }
    // SAFETY: `data` is the appdata_ptr `serve` gave pam_start_confdir: its `io`, which lives
    // until that process exits.
    let io = unsafe { &*(data as *const ChildIo) };
    // Freed by libpam, so allocated by libc.
    // SAFETY: calloc(3) has no preconditions; a null result is checked below.
    let out = unsafe { libc::calloc(n as usize, std::mem::size_of::<pam_response>()) }
        as *mut pam_response;
    if out.is_null() {
        return PAM_BUF_ERR;
    }
    // SAFETY: calloc returned `n` zeroed responses (null `resp`, code 0: valid values),
    // aligned for any type and used through this slice alone until `*resp = out` below.
    let replies = unsafe { std::slice::from_raw_parts_mut(out, n as usize) };
    // SAFETY: pam_conv(3): `msgs` (checked non-null) points to `n` message pointers, valid for
    // the call.
    let msgs = unsafe { std::slice::from_raw_parts(msgs, n as usize) };
    for (i, &m) in msgs.iter().enumerate() {
        // SAFETY: pam_conv(3): each of those points to a valid pam_message for the call.
        let m = unsafe { &*m };
        let text = if m.msg.is_null() {
            &[][..]
        } else {
            // SAFETY: a non-null message text is a NUL-terminated string valid for the call.
            unsafe { CStr::from_ptr(m.msg) }.to_bytes()
        };
        let required = m.msg_style == PAM_PROMPT_ECHO_OFF || m.msg_style == PAM_PROMPT_ECHO_ON;
        let head = [
            EV_REQUEST,
            (m.msg_style != PAM_PROMPT_ECHO_OFF) as u8,
            (m.msg_style == PAM_ERROR_MSG) as u8,
            required as u8,
        ];
        if !fd_write_all(io.fd, &head) || !fd_write_str(io.fd, text) {
            // SAFETY: _exit(2) has no preconditions; it ends the helper.
            unsafe { libc::_exit(1) };
        }
        if required {
            let Some(mut r) = fd_read_str(io.fd) else {
                // SAFETY: _exit(2) has no preconditions; it ends the helper.
                unsafe { libc::_exit(EXIT_OTHER_ERROR as c_int) };
            };
            r.retain(|&c| c != 0);
            r.push(0);
            // SAFETY: `r` is NUL-terminated (pushed above), alive for the call.
            replies[i].resp = unsafe { libc::strdup(r.as_ptr().cast()) };
            zero(&mut r);
        }
    }
    // SAFETY: pam_conv(3): `resp` (checked non-null) is where the response array goes.
    unsafe { *resp = out };
    PAM_SUCCESS
}

/// Close every inherited descriptor except stdio and `keep`: the shell's descriptors opened
/// without `O_CLOEXEC` reach the helper, and PAM modules must not see them.
fn close_other_fds(keep: RawFd) {
    let k = keep as u32;
    for (a, b) in [(3, k.wrapping_sub(1)), (k + 1, u32::MAX)] {
        if a > b || a < 3 {
            continue;
        }
        // SAFETY: close_range(2) takes plain integers; nothing in the helper owns these
        // inherited descriptors.
        if unsafe { libc::syscall(libc::SYS_close_range, a, b, 0) } != 0 {
            for fd in a..=b.min(65535) {
                // SAFETY: as for close_range above.
                unsafe { libc::close(fd as c_int) };
            }
        }
    }
}

/// The argument that makes the shell binary run [`serve`].
pub const ARG: &str = "__ri-pam-conversation";

/// Builds the command that runs [`serve`]; set by the embedding binary ([`enable`]).
pub type Launcher = fn() -> std::process::Command;

static LAUNCHER: std::sync::Mutex<Option<Launcher>> = std::sync::Mutex::new(None);

/// Run `PamContext` conversations with `launch`, a command that runs [`serve`] (the shell
/// binary: [`self_command`]). Without this call every conversation fails to start.
pub fn enable(launch: Launcher) {
    *LAUNCHER.lock().unwrap_or_else(|e| e.into_inner()) = Some(launch);
}

fn launcher() -> Option<Launcher> {
    let l = *LAUNCHER.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(test)]
    let l = l.or(Some(tests::helper_command as Launcher));
    l
}

/// The command [`enable`] takes in the shell binary: this executable, run with [`ARG`]. It
/// keeps this process's environment, the config's pragmas applied, not the user's
/// ([`workpool::userenv`]): Quickshell's PAM conversation runs in a `fork()` of the shell
/// with no exec (src/services/pam/subprocess.cpp:40), so it sees the shell's own.
pub fn self_command() -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new("/proc/self/exe");
    c.arg0("ri-pam").arg(ARG);
    c
}

/// The helper's main: one conversation over the socket on stdin, then exits. The shell first
/// sends its pid and the (configDirectory, config, user) to start with.
pub fn serve() -> ! {
    // SAFETY: fcntl(2) F_DUPFD_CLOEXEC takes plain integers: the socket moves off stdin (to 3
    // or above) and is not inherited by programs a module runs.
    let fd = unsafe { libc::fcntl(0, libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        std::process::exit(1);
    }
    if let Ok(null) = std::fs::File::open("/dev/null") {
        // SAFETY: dup2(2) takes plain integers; `null` stays open for the call.
        unsafe { libc::dup2(null.as_raw_fd(), 0) };
    }
    // SAFETY: PR_SET_PDEATHSIG takes a signal number and touches no memory of ours.
    unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) };
    let mut pid = [0u8; 4];
    let setup = fd_read_exact(fd, &mut pid)
        .then(|| (fd_read_str(fd)?, fd_read_str(fd)?, fd_read_str(fd)?).into())
        .flatten()
        .and_then(|(d, c, u): (Vec<u8>, Vec<u8>, Vec<u8>)| {
            Some((
                CString::new(d).ok()?,
                CString::new(c).ok()?,
                CString::new(u).ok()?,
            ))
        });
    // The shell died before PR_SET_PDEATHSIG took effect: this process was reparented.
    // SAFETY: getppid(2) takes no arguments and always succeeds.
    let orphan = unsafe { libc::getppid() } != i32::from_ne_bytes(pid);
    let (Some((dir, cfg, user)), false) = (setup, orphan) else {
        std::process::exit(1);
    };
    close_other_fds(fd);
    let io = ChildIo { fd };
    let conv = pam_conv {
        conv: Some(conversation),
        appdata_ptr: &io as *const ChildIo as *mut c_void,
    };
    let mut h: *mut pam_handle_t = std::ptr::null_mut();
    // SAFETY: the three strings are NUL-terminated and outlive the handle; `conv` and the `io`
    // it points to live until this process exits; `h` is writable.
    let r =
        unsafe { ffi::pam_start_confdir(cfg.as_ptr(), user.as_ptr(), &conv, dir.as_ptr(), &mut h) };
    let code = if r != PAM_SUCCESS {
        EXIT_START_FAILED
    } else {
        // SAFETY: `h` is the handle pam_start_confdir just created.
        let r = unsafe { ffi::pam_authenticate(h, 0) };
        // SAFETY: `h` is that live handle, ended once here and not used after.
        unsafe { ffi::pam_end(h, r) };
        match r {
            PAM_SUCCESS => EXIT_SUCCESS,
            PAM_AUTH_ERR => EXIT_AUTH_FAILED,
            PAM_MAXTRIES => EXIT_MAX_TRIES,
            _ => EXIT_PAM_ERROR,
        }
    };
    std::process::exit(if fd_write_all(fd, &[EV_EXIT, code]) {
        0
    } else {
        1
    })
}

/// Start the conversation helper in its own process group. Returns it and our end of the
/// socket.
fn spawn(dir: &str, cfg: &str, user: &str) -> Option<(ChildRef, OwnedFd)> {
    let Some(launch) = launcher() else {
        elog::eprintln!("PAM: no conversation helper (qml::pam::enable was not called)");
        return None;
    };
    let mut p = [0 as c_int; 2];
    // SAFETY: `p` is the writable array of two descriptors socketpair(2) fills.
    if unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
            p.as_mut_ptr(),
        )
    } != 0
    {
        return None;
    }
    // SAFETY: socketpair just returned this end, owned by nothing else.
    let ours = unsafe { OwnedFd::from_raw_fd(p[0]) };
    // SAFETY: socketpair just returned this end, owned by nothing else.
    let theirs = unsafe { OwnedFd::from_raw_fd(p[1]) };
    let mut c = launch();
    {
        use std::os::unix::process::CommandExt;
        // Own process group: abort also kills helpers a module spawned (pam_exec, fprintd).
        c.process_group(0);
    }
    c.stdin(std::process::Stdio::from(theirs));
    let ch = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            elog::eprintln!("PAM: cannot start the conversation helper: {e}");
            return None;
        }
    };
    let pid = ch.id() as libc::pid_t;
    // Dropping a `Child` neither waits nor kills: `reader` reaps it.
    drop(ch);
    let kill = || {
        // SAFETY: kill(2) takes plain integers; `pid` is our unreaped child.
        unsafe { libc::kill(pid, libc::SIGKILL) };
        // SAFETY: waitpid(2) accepts a null status pointer; `pid` is reaped once.
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
    };
    let fd = ours.as_raw_fd();
    let sent = fd_write_all(fd, &std::process::id().to_ne_bytes())
        && [dir, cfg, user]
            .iter()
            .all(|s| fd_write_str(fd, s.as_bytes()));
    if !sent {
        elog::eprintln!("PAM: conversation helper exited at start");
        kill();
        return None;
    }
    match ChildRef::new(pid) {
        Ok(c) => Some((c, ours)),
        Err(e) => {
            elog::eprintln!("PAM: conversation helper: pidfd_open: {e}");
            kill();
            None
        }
    }
}

enum Ev {
    Message {
        text: String,
        error: bool,
        required: bool,
        visible: bool,
    },
    Exit(u8),
    Broken,
}

/// Reads child events until exit/EOF, then reaps the child (off the main thread). An error
/// is the system refusing the thread (`EAGAIN`): nothing reads or reaps the child then.
fn reader(
    item: usize,
    id: u64,
    pid: libc::pid_t,
    rd: OwnedFd,
    tx: crate::svc::Sender<(usize, u64, Ev)>,
) -> std::io::Result<()> {
    let read = move || {
        let fd = rd.as_raw_fd();
        loop {
            let mut t = [0u8];
            let ev = if !fd_read_exact(fd, &mut t) {
                Ev::Broken
            } else if t[0] == EV_EXIT {
                let mut c = [0u8];
                if fd_read_exact(fd, &mut c) {
                    Ev::Exit(c[0])
                } else {
                    Ev::Broken
                }
            } else if t[0] == EV_REQUEST {
                let mut f = [0u8; 3];
                match (fd_read_exact(fd, &mut f), fd_read_str(fd)) {
                    (true, Some(s)) => Ev::Message {
                        text: String::from_utf8_lossy(&s).into_owned(),
                        visible: f[0] != 0,
                        error: f[1] != 0,
                        required: f[2] != 0,
                    },
                    _ => Ev::Broken,
                }
            } else {
                Ev::Broken
            };
            let last = !matches!(ev, Ev::Message { .. });
            let _ = tx.send((item, id, ev));
            if last {
                break;
            }
        }
        drop(rd);
        // SAFETY: waitpid(2) accepts a null status pointer; only this thread reaps `pid`.
        unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
    };
    std::thread::Builder::new()
        .name("pam-reader".into())
        .spawn(read)
        .map(drop)
}

struct Conv {
    id: u64,
    child: ChildRef,
    to_child: OwnedFd,
    /// Until when the helper's first event is waited for ([`Runtime::pam_helper_due`]).
    awaited: Option<std::time::Instant>,
}

/// A helper asks for the password a few ms after it starts; one that sends nothing (a
/// fingerprint reader waiting for a finger) is not waited for longer than this.
const HELPER_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

impl Drop for Conv {
    fn drop(&mut self) {
        // The reader thread reaps it once the pipe closes. The group can't be recycled while
        // any of its members (or the unreaped leader) exists.
        self.child.signal(libc::SIGKILL);
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        unsafe { libc::kill(-self.child.pid, libc::SIGKILL) };
    }
}

#[derive(Default)]
struct Ctx {
    seen: bool,
    target: bool,
    /// Last `active` value written or observed: a different value was assigned from QML.
    active: bool,
    conv: Option<Conv>,
    /// (config, configDirectory, user) at start; quickshell refuses changes while active.
    locked: Option<[String; 3]>,
    response_required: bool,
}

pub struct Pam {
    ctxs: HashMap<usize, Ctx>,
    tx: crate::svc::Sender<(usize, u64, Ev)>,
    rx: Receiver<(usize, u64, Ev)>,
    next: u64,
}

impl Pam {
    fn new(h: &crate::svc::Handle) -> Pam {
        let (tx, rx) = h.channel();
        Pam {
            ctxs: HashMap::new(),
            tx,
            rx,
            next: 0,
        }
    }
}

fn is_ctx(w: &World, item: usize) -> bool {
    w.items
        .get(item)
        .is_some_and(|i| i.types.iter().any(|t| t == "PamContext"))
}

fn read_str(w: &mut World, item: usize, name: &str) -> String {
    match w.read(item, name) {
        Some(Value::Str(s)) => s.to_string(),
        Some(Value::Undefined | Value::Null) | None => String::new(),
        Some(v) => v.to_js_string(),
    }
}

fn user_name(user: &str) -> Result<String, String> {
    use crate::osutil::PwKey;
    let not_found = || format!("because specified user was not found: {user:?}");
    let c;
    let key = if user.is_empty() {
        // SAFETY: getuid(2) takes no arguments and always succeeds.
        PwKey::Uid(unsafe { libc::getuid() })
    } else {
        c = CString::new(user).map_err(|_| not_found())?;
        PwKey::Name(&c)
    };
    match crate::osutil::passwd(key, |e| e.name().map(|n| n.to_string_lossy().into_owned())) {
        Ok(Some(Some(name))) => Ok(name),
        Ok(_) if !user.is_empty() => Err(not_found()),
        Ok(_) => Err("due to error in getpw*_r: 0".into()),
        Err(e) => Err(format!("due to error in getpw*_r: {e}")),
    }
}

impl Pam {
    /// Pick up QML-side changes: defaults on first sight, `active` assignments, relative
    /// `configDirectory`, and writes to config/user/configDirectory while active (reverted).
    fn sync(&mut self, w: &mut World, item: usize) {
        let ctx = self.ctxs.entry(item).or_default();
        if !ctx.seen {
            ctx.seen = true;
            if read_str(w, item, "config").is_empty() {
                w.write(item, "config", Value::str("login"));
            }
            if read_str(w, item, "configDirectory").is_empty() {
                w.write(item, "configDirectory", Value::str("/etc/pam.d"));
            }
        }
        let dir = read_str(w, item, "configDirectory");
        if !dir.starts_with('/') {
            // PamContext::setConfigDirectory: `context->resolvedUrl(configDirectory).path()`.
            let joined = std::path::PathBuf::from(w.resolved_qurl(item, &dir).path());
            // A relative declaring file (`rusty-impulse -p rel/x.qml`) must still resolve to an absolute
            // path, or the next sync would join it again and grow it on every poll.
            let abs = std::path::absolute(&joined).unwrap_or(joined);
            w.write(item, "configDirectory", Value::str(&abs.to_string_lossy()));
        }
        let ctx = self.ctxs.get_mut(&item).unwrap();
        if let Some(locked) = ctx.locked.clone() {
            for (k, v) in ["config", "configDirectory", "user"].iter().zip(locked) {
                if read_str(w, item, k) != v {
                    elog::eprintln!("PamContext: Cannot set {k} on PamContext while it is active.");
                    w.write(item, k, Value::str(&v));
                }
            }
        }
        let a = w.read(item, "active").is_some_and(|v| v.truthy());
        let ctx = self.ctxs.get_mut(&item).unwrap();
        if a != ctx.active {
            ctx.active = a;
            self.set_active(w, item, a);
        }
    }

    fn set_active(&mut self, w: &mut World, item: usize, active: bool) {
        let ctx = self.ctxs.entry(item).or_default();
        if ctx.target != active {
            ctx.target = active;
            if active {
                self.start_conversation(w, item);
            } else {
                self.abort_conversation(w, item);
            }
        }
        let ctx = self.ctxs.get_mut(&item).unwrap();
        ctx.active = ctx.conv.is_some();
        w.write(item, "active", Value::Bool(ctx.active));
    }

    fn start_conversation(&mut self, w: &mut World, item: usize) {
        if self.ctxs.get(&item).is_some_and(|c| c.conv.is_some()) {
            return;
        }
        let cfg = read_str(w, item, "config");
        let dir = read_str(w, item, "configDirectory");
        let user = read_str(w, item, "user");
        let fail = |p: &mut Pam, why: String| {
            elog::eprintln!("PamContext: Cannot start {why}");
            p.ctxs.get_mut(&item).unwrap().target = false;
        };
        if !std::path::Path::new(&dir).is_dir() {
            return fail(
                self,
                format!("because specified config directory {dir:?} is not a directory."),
            );
        }
        let file = std::path::Path::new(&dir).join(&cfg);
        if !file.is_file() {
            return fail(
                self,
                format!("because specified config file {file:?} is not a file."),
            );
        }
        let name = match user_name(&user) {
            Ok(n) => n,
            Err(e) => return fail(self, e),
        };
        self.next += 1;
        let id = self.next;
        let spawned = spawn(&dir, &cfg, &name).and_then(|(child, sock)| {
            let from_child = sock.try_clone().ok()?;
            Some((child, sock, from_child))
        });
        let Some((child, to_child, from_child)) = spawned else {
            elog::eprintln!("PamContext: Failed to create pam subprocess.");
            self.on_error(w, item, ERROR_INTERNAL);
            return;
        };
        if let Err(e) = reader(item, id, child.pid, from_child, self.tx.clone()) {
            elog::eprintln!("PamContext: Failed to start the pam reader thread: {e}");
            // No thread reads the helper or reaps it: end its group and reap it here.
            child.signal(libc::SIGKILL);
            // SAFETY: kill(2) takes plain integers; the unreaped leader keeps the group's id.
            unsafe { libc::kill(-child.pid, libc::SIGKILL) };
            // SAFETY: waitpid(2) accepts a null status pointer; nothing else reaps `pid`.
            unsafe { libc::waitpid(child.pid, std::ptr::null_mut(), 0) };
            self.on_error(w, item, ERROR_INTERNAL);
            return;
        }
        let ctx = self.ctxs.get_mut(&item).unwrap();
        ctx.conv = Some(Conv {
            id,
            child,
            to_child,
            awaited: Some(std::time::Instant::now() + HELPER_WAIT),
        });
        ctx.locked = Some([cfg, dir, user]);
        ctx.active = true;
        w.write(item, "active", Value::Bool(true));
    }

    fn abort_conversation(&mut self, w: &mut World, item: usize) {
        let Some(ctx) = self.ctxs.get_mut(&item) else {
            return;
        };
        if ctx.conv.take().is_none() {
            return;
        }
        ctx.target = false;
        ctx.locked = None;
        ctx.active = false;
        ctx.response_required = false;
        w.write(item, "active", Value::Bool(false));
        w.write(item, "message", Value::str(""));
        w.write(item, "messageIsError", Value::Bool(false));
        w.write(item, "responseRequired", Value::Bool(false));
    }

    fn on_error(&mut self, w: &mut World, item: usize, e: f64) {
        self.abort_conversation(w, item);
        crate::native::emit(w, item, "error", &["error"], vec![Value::Num(e)]);
        self.on_completed(w, item, RESULT_ERROR);
    }

    fn on_completed(&mut self, w: &mut World, item: usize, r: f64) {
        self.abort_conversation(w, item);
        crate::native::emit(w, item, "completed", &["result"], vec![Value::Num(r)]);
    }

    fn handle(&mut self, w: &mut World, item: usize, ev: Ev) {
        match ev {
            Ev::Message {
                text,
                error,
                required,
                visible,
            } => {
                w.write(item, "message", Value::str(&text));
                w.write(item, "messageIsError", Value::Bool(error));
                w.write(item, "responseVisible", Value::Bool(visible));
                w.write(item, "responseRequired", Value::Bool(required));
                if let Some(c) = self.ctxs.get_mut(&item) {
                    c.response_required = required;
                }
                crate::native::emit(w, item, "pamMessage", &[], vec![]);
            }
            Ev::Exit(code) => match code {
                EXIT_SUCCESS => self.on_completed(w, item, RESULT_SUCCESS),
                EXIT_AUTH_FAILED => self.on_completed(w, item, RESULT_FAILED),
                EXIT_MAX_TRIES => self.on_completed(w, item, RESULT_MAX_TRIES),
                EXIT_START_FAILED => self.on_error(w, item, ERROR_START_FAILED),
                EXIT_PAM_ERROR => self.on_error(w, item, ERROR_TRY_AUTH_FAILED),
                _ => self.on_error(w, item, ERROR_INTERNAL),
            },
            Ev::Broken => {
                elog::eprintln!("PamContext: Failed to read subprocess request.");
                self.on_error(w, item, ERROR_INTERNAL);
            }
        }
    }

    fn respond(&mut self, w: &mut World, item: usize, response: String) {
        let ok = match self.ctxs.get(&item) {
            Some(Ctx {
                conv: Some(c),
                response_required: true,
                ..
            }) => Some(fd_write_str(c.to_child.as_raw_fd(), response.as_bytes())),
            _ => None,
        };
        zero_string(response);
        match ok {
            None => {
                elog::eprintln!(
                    "PamContext: response was ignored as this context does not require one."
                )
            }
            Some(false) => {
                elog::eprintln!("PamContext: Failed to write response to subprocess.");
                self.on_error(w, item, ERROR_INTERNAL);
            }
            Some(true) => {}
        }
    }
}

/// `PamContext.start()` / `abort()` / `respond()` called from QML.
/// `args: None` only probes for the method ([`crate::eval::Host::native`]).
pub(crate) fn call(
    w: &mut World,
    item: usize,
    name: &str,
    args: Option<&[Value]>,
) -> Option<Value> {
    if !matches!(name, "start" | "abort" | "respond") || !is_ctx(w, item) {
        return None;
    }
    let Some(args) = args else {
        return Some(Value::Undefined);
    };
    let mut p = w
        .pam
        .take()
        .unwrap_or_else(|| Pam::new(&w.waker.handle(crate::svc::Svc::Pam)));
    p.sync(w, item);
    let r = match name {
        "start" => {
            p.set_active(w, item, true);
            Value::Bool(p.ctxs.get(&item).is_some_and(|c| c.conv.is_some()))
        }
        "abort" => {
            p.set_active(w, item, false);
            Value::Undefined
        }
        _ => {
            let s = match args.first() {
                Some(Value::Str(s)) => s.to_string(),
                Some(v) => v.to_js_string(),
                None => String::new(),
            };
            p.respond(w, item, s);
            Value::Undefined
        }
    };
    w.pam = Some(p);
    Some(r)
}

impl Runtime {
    /// Drive live `PamContext` instances; returns whether anything changed. Each PAM message
    /// is delivered on its own settle, so `onPamMessage` sees that message's state (and its
    /// synchronous `respond()` answers it), as with quickshell's per-message signal.
    pub(crate) fn poll_pam(&mut self) -> bool {
        let rev = {
            let mut w = self.world.borrow_mut();
            let items = w.of_type("PamContext");
            if items.is_empty() && w.pam.is_none() {
                return false;
            }
            let rev = w.revision;
            let mut p = w
                .pam
                .take()
                .unwrap_or_else(|| Pam::new(&w.waker.handle(crate::svc::Svc::Pam)));
            // Destroyed contexts: kill their conversation silently.
            p.ctxs.retain(|i, _| items.contains(i));
            for &i in &items {
                w.mirror(i, crate::svc::Svc::Pam);
                p.sync(&mut w, i);
            }
            w.pam = Some(p);
            rev
        };
        let mut changed = false;
        loop {
            let ev = {
                let w = self.world.borrow();
                match w.pam.as_ref().map(|p| p.rx.try_recv()) {
                    Some(Ok(ev)) => ev,
                    _ => break,
                }
            };
            let (item, id, ev) = ev;
            let mut w = self.world.borrow_mut();
            let mut p = w
                .pam
                .take()
                .unwrap_or_else(|| Pam::new(&w.waker.handle(crate::svc::Svc::Pam)));
            let conv = p
                .ctxs
                .get_mut(&item)
                .and_then(|c| c.conv.as_mut())
                .filter(|c| c.id == id);
            let current = conv.is_some();
            if let Some(c) = conv {
                c.awaited = None;
            }
            if current {
                p.handle(&mut w, item, ev);
            }
            w.pam = Some(p);
            drop(w);
            if current {
                changed = true;
                self.settle();
            }
        }
        changed || self.world.borrow().revision != rev
    }

    /// How long the first event of a conversation just started is still waited for: its
    /// helper asks for the password a few ms after `start()`, and the answer (`respond()` in
    /// `onPamMessage`) is given by the turn that reads the request. A loop that draws a long
    /// frame in between (a blurred wallpaper takes 0.8 s without a GPU) starts the check that
    /// much later, and its result comes after the user typed the next password: a failure
    /// then clears that one. Qt renders on another thread and answers at once.
    pub fn pam_helper_due(&self) -> Option<std::time::Duration> {
        self.pam_helper_due_at(std::time::Instant::now())
    }

    /// [`Runtime::pam_helper_due`] at the moment `now`.
    fn pam_helper_due_at(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        let w = self.world.borrow();
        w.pam
            .as_ref()?
            .ctxs
            .values()
            .filter_map(|c| c.conv.as_ref()?.awaited)
            .filter_map(|t| t.checked_duration_since(now))
            .filter(|d| !d.is_zero())
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::HELPER_WAIT;
    use crate::runtime::Runtime;
    use crate::value::Value;

    const HELPER: &str = concat!(module_path!(), "::conversation_helper");

    /// [`super::launcher`] in this test binary: it runs [`conversation_helper`] in a process of
    /// its own, which serves the conversation.
    pub(super) fn helper_command() -> std::process::Command {
        let mut c = testenv::own_process_command(HELPER);
        c.stdout(std::process::Stdio::null());
        c
    }

    /// The conversation helper when [`helper_command`] runs it; a no-op test otherwise.
    #[test]
    fn conversation_helper() {
        if testenv::in_own_process(HELPER) {
            super::serve();
        }
    }

    /// The conversation runs in a freshly executed helper, never in a bare fork of this
    /// multithreaded process: what a module sees as its parent's command line (pam_exec
    /// `stdout` turns the script's output into a PAM_TEXT_INFO message) is the helper's.
    #[test]
    fn conversation_runs_in_an_executed_helper() {
        let d = fixture("exec", "");
        let sh = d.join("parent.sh");
        crate::testenv::install_script(
            &sh,
            "#!/bin/sh
tr '\\0' ' ' < /proc/$PPID/cmdline
",
        );
        std::fs::write(
            d.join("t"),
            format!(
                "auth optional pam_exec.so stdout {}\nauth required pam_permit.so\n",
                sh.display()
            ),
        )
        .unwrap();
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        let log = run(&mut rt, r, "done");
        let name = HELPER.split_once("::").unwrap().1;
        let exe = std::env::current_exe().unwrap();
        let cmdline = format!("I:{} {name} --exact", exe.display());
        assert!(log.contains(&cmdline), "{log}");
        assert!(log.ends_with("done:0"), "{log}");
    }

    /// A fresh directory per call: tests run in parallel and several share a fixture name.
    fn fixture(name: &str, conf: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("ri-pam-{}-{seq}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let check = d.join("check.sh");
        crate::testenv::install_script(&check, "#!/bin/sh\nread -r p\n[ \"$p\" = secret ]\n");
        std::fs::write(d.join("t"), conf.replace("CHECK", &check.to_string_lossy())).unwrap();
        d
    }

    /// LockContext-style client: answers prompts with `answer`, logs messages and results.
    fn client(dir: &std::path::Path, extra: &str) -> String {
        // A property is set once (Qt: "Property value set multiple times").
        let config = match extra.contains("config:") {
            true => "",
            false => "config: 't';",
        };
        format!(
            "import QtQuick\nimport Rustshell.Services.Pam\nItem {{ id: r\n\
             property var log: []; property string answer: 'secret'; property int step: 0\n\
             property bool started\n\
             onStepChanged: {{ if (step === 1) r.started = pam.start(); else if (step === 2) pam.abort(); else if (step === 3) {{ pam.abort(); r.started = pam.start() }} }}\n\
             PamContext {{ id: pam; configDirectory: '{}'; {config} {extra}\n\
               onPamMessage: {{ r.log = [...r.log, (this.messageIsError ? 'E:' : 'I:') + this.message + (this.responseRequired ? '?' : '') + (this.responseVisible ? 'v' : '')];\n\
                 if (this.responseRequired && r.answer !== 'none') this.respond(r.answer) }}\n\
               onCompleted: result => r.log = [...r.log, 'done:' + result]\n\
               onError: error => r.log = [...r.log, 'err:' + error] }} }}",
            dir.display()
        )
    }

    fn run(rt: &mut Runtime, r: usize, until: &str) -> String {
        crate::native::tests::run_until(rt, &|rt| rt.get(r, "log").to_js_string().contains(until));
        rt.get(r, "log").to_js_string()
    }

    fn step(rt: &mut Runtime, r: usize, n: f64) {
        rt.world.borrow_mut().write(r, "step", Value::Num(n));
        rt.settle();
    }

    #[test]
    fn success_and_denied() {
        let d = fixture("ok", "auth required pam_permit.so\n");
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        rt.poll_native();
        assert_eq!(rt.get(r, "log").to_js_string(), "");
        assert!(!rt.get(r, "active").truthy());
        step(&mut rt, r, 1.0);
        assert!(rt.get(r, "started").truthy());
        assert_eq!(run(&mut rt, r, "done"), "done:0");
        let pam = rt.id("pam").unwrap();
        assert!(!rt.get(pam, "active").truthy());

        let d = fixture("deny", "auth required pam_deny.so\n");
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        assert_eq!(run(&mut rt, r, "done"), "done:1");
    }

    /// The system refuses the thread that reads the conversation helper (the limit on threads
    /// is reached as a password check starts): the check ends with an internal error, which
    /// the lock screen shows, instead of a panic of the shell; the context is idle and can
    /// be started again, and the helper nobody reads was ended and reaped. The refusal is in
    /// the log. In a process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_reader_thread_ends_the_check_with_an_error() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_reader_thread_ends_the_check_with_an_error"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let d = fixture("nothread", PROMPTS);
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        crate::testenv::refuse_new_threads();
        let ((), log) = crate::testenv::stderr_with(|| step(&mut rt, r, 1.0));
        assert!(
            log.contains("PamContext: Failed to start the pam reader thread:"),
            "stderr: {log:?}"
        );
        assert_eq!(run(&mut rt, r, "done"), "err:3,done:2");
        let pam = rt.id("pam").unwrap();
        assert!(!rt.get(pam, "active").truthy());
        // SAFETY: waitpid(2) with a null status pointer; -1 is any child of this process.
        let r = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        let e = std::io::Error::last_os_error();
        assert_eq!(
            (r, e.raw_os_error()),
            (-1, Some(libc::ECHILD)),
            "the helper is still this process's child"
        );
    }

    const PROMPTS: &str = "auth optional pam_echo.so hello info\n\
        auth optional pam_exec.so /bin/false\n\
        auth [success=1 default=ignore] pam_exec.so expose_authtok quiet CHECK\n\
        auth requisite pam_deny.so\n\
        auth required pam_permit.so\n";

    #[test]
    fn messages_prompt_and_password() {
        let d = fixture("pw", PROMPTS);
        for (answer, result) in [("secret", "done:0"), ("wrong", "done:1")] {
            let mut rt = Runtime::new(None);
            let r = rt.load_str(&client(&d, "")).unwrap();
            rt.settle();
            rt.world.borrow_mut().write(r, "answer", Value::str(answer));
            step(&mut rt, r, 1.0);
            let pam = rt.id("pam").unwrap();
            assert!(rt.get(pam, "active").truthy());
            let log = run(&mut rt, r, "done");
            assert_eq!(
                log,
                // rustshell: echo = style != PAM_PROMPT_ECHO_OFF, so info/error read responseVisible.
                format!("I:hello infov,E:/bin/false failed: exit code 1v,I:Password: ?,{result}")
            );
            assert!(!rt.get(pam, "active").truthy());
            assert_eq!(rt.get(pam, "message").to_js_string(), "");
            assert!(!rt.get(pam, "responseRequired").truthy());
        }
    }

    #[test]
    fn a_started_helper_is_waited_for_until_its_first_event() {
        let d = fixture("due", PROMPTS);
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        assert_eq!(rt.pam_helper_due(), None);
        rt.world.borrow_mut().write(r, "answer", Value::str("none"));
        step(&mut rt, r, 1.0);
        let left = rt
            .pam_helper_due()
            .expect("the helper's request is on its way");
        assert!(left <= HELPER_WAIT, "{left:?}");
        // The turn that reads the helper's first event ends the wait, whatever comes after.
        run(&mut rt, r, "hello");
        assert_eq!(rt.pam_helper_due(), None);
        run(&mut rt, r, "Password");
        assert_eq!(rt.pam_helper_due(), None);
        step(&mut rt, r, 2.0);
        assert_eq!(rt.pam_helper_due(), None);

        // A helper that says nothing (a module waiting for a finger) is not waited for long.
        let d = fixture("due-hang", "");
        let pidfile = silent_module(&d);
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        let before = std::time::Instant::now();
        step(&mut rt, r, 1.0);
        let first = module_pid(&pidfile);
        let started = std::time::Instant::now();
        assert!(rt.pam_helper_due_at(before).is_some());
        assert_eq!(rt.pam_helper_due_at(started + HELPER_WAIT), None);
        assert!(rt.get(rt.id("pam").unwrap(), "active").truthy());
        // An aborted conversation is not waited for.
        let before = std::time::Instant::now();
        step(&mut rt, r, 3.0);
        let second = module_pid(&pidfile);
        assert!(rt.pam_helper_due_at(before).is_some());
        step(&mut rt, r, 2.0);
        assert_eq!(rt.pam_helper_due_at(before), None);
        // pam_exec setsid()s its helper: the abort does not end it.
        for pid in [first, second] {
            // SAFETY: kill(2) takes plain integers and touches no memory of ours.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }

    /// Makes the service `t` of `dir` one module that never returns and says nothing; the
    /// fifo its helper writes its pid to once it runs ([`module_pid`]).
    fn silent_module(dir: &std::path::Path) -> std::path::PathBuf {
        let pidfile = dir.join("pid");
        let _ = std::fs::remove_file(&pidfile);
        crate::testenv::fifo(&pidfile);
        let hang = dir.join("hang.sh");
        crate::testenv::install_script(
            &hang,
            &format!(
                "#!/bin/sh\necho $$ > {}\nexec sleep infinity\n",
                pidfile.display()
            ),
        );
        std::fs::write(
            dir.join("t"),
            format!("auth required pam_exec.so quiet {}\n", hang.display()),
        )
        .unwrap();
        pidfile
    }

    /// The pid of the helper of [`silent_module`] that started last; blocks until it runs.
    fn module_pid(pidfile: &std::path::Path) -> i32 {
        std::fs::read_to_string(pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    #[test]
    fn abort_during_prompt_and_hung_module() {
        let d = fixture("abort", PROMPTS);
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        rt.world.borrow_mut().write(r, "answer", Value::str("none"));
        step(&mut rt, r, 1.0);
        run(&mut rt, r, "Password");
        let pam = rt.id("pam").unwrap();
        assert!(rt.get(pam, "responseRequired").truthy());
        // Nothing is pending once this poll took it: the next wakeup is the killed
        // conversation's last event.
        rt.poll_native();
        crate::native::tests::drain_events();
        step(&mut rt, r, 2.0);
        assert!(!rt.get(pam, "active").truthy());
        assert!(!rt.get(pam, "responseRequired").truthy());
        assert_eq!(rt.get(pam, "message").to_js_string(), "");
        crate::native::tests::wait_event(&mut rt);
        rt.poll_native();
        assert!(!rt.get(r, "log").to_js_string().contains("done"));

        // A module that never returns: abort kills the child right away instead of waiting.
        let d = fixture("hang", "");
        let pidfile = silent_module(&d);
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "")).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        // The conversation child runs the module on its own; it writes its pid once it runs.
        let helper = module_pid(&pidfile);
        let pam = rt.id("pam").unwrap();
        assert!(rt.get(pam, "active").truthy());
        // The module never returns: an abort that waited for it would never return.
        step(&mut rt, r, 2.0);
        assert!(!rt.get(pam, "active").truthy());
        // pam_exec setsid()s its helper; like Quickshell (SIGKILL to the child only) it is not
        // part of the abort.
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        unsafe { libc::kill(helper, libc::SIGKILL) };
        // Restarting right after an abort runs a fresh conversation; stale events are dropped.
        step(&mut rt, r, 3.0);
        assert!(rt.get(r, "started").truthy());
        step(&mut rt, r, 2.0);
        rt.poll_native();
        assert_eq!(rt.get(r, "log").to_js_string(), "");
    }

    #[test]
    fn restart_while_active_and_declarative_active() {
        let d = fixture("restart", "auth required pam_permit.so\n");
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "active: true")).unwrap();
        rt.settle();
        assert_eq!(run(&mut rt, r, "done"), "done:0");
        step(&mut rt, r, 3.0);
        assert!(rt.get(r, "started").truthy());
        assert_eq!(run(&mut rt, r, "done:0,done"), "done:0,done:0");
    }

    /// pam_iitest.so (tests/auth): info + error + visible PIN prompt in one conversation call,
    /// then a hidden password prompt. Needs gcc and the libpam headers.
    #[test]
    fn multi_message_module() {
        let d = fixture("multi", "");
        let so = d.join("pam_iitest.so");
        let src = d.join("pam_iitest.c");
        std::fs::write(&src, include_str!("../tests/auth/pam_iitest.c")).unwrap();
        let built = std::process::Command::new("gcc")
            .args(["-fPIC", "-shared", "-o"])
            .arg(&so)
            .arg(&src)
            .arg("-lpam")
            .status()
            .is_ok_and(|s| s.success());
        if !built {
            crate::testenv::unavailable(
                "cannot build pam_iitest.so (needs gcc and the libpam headers)",
            );
            return;
        }
        std::fs::write(d.join("t"), format!("auth required {}\n", so.display())).unwrap();
        for (answers, result) in [("4242,hunter2", "done:0"), ("4242,nope", "done:1")] {
            let qml = client(&d, "")
                .replace(
                    "property string answer: 'secret'",
                    &format!("property var answers: '{answers}'.split(',')"),
                )
                .replace("this.respond(r.answer)", "this.respond(r.answers.shift())")
                .replace("r.answer !== 'none'", "true");
            let mut rt = Runtime::new(None);
            let r = rt.load_str(&qml).unwrap();
            rt.settle();
            step(&mut rt, r, 1.0);
            assert_eq!(
                run(&mut rt, r, "done"),
                format!(
                    "I:Touch the key or enter PINv,E:Fingerprint reader unavailablev,\
                     I:PIN: ?v,I:Password: ?,{result}"
                )
            );
        }
    }

    /// The [`ffi`] declarations match the installed `<security/pam_appl.h>`: constants, struct
    /// layouts and the entry points' C signatures (gcc rejects a conflicting redeclaration).
    /// Needs gcc and the libpam headers.
    #[test]
    fn ffi_matches_header() {
        use super::ffi::*;
        use std::mem::{align_of, offset_of, size_of};
        let d = fixture("header", "");
        let (src, exe) = (d.join("layout.c"), d.join("layout"));
        std::fs::write(
            &src,
            r#"#include <security/pam_appl.h>
#include <stddef.h>
#include <stdio.h>
int pam_start_confdir(const char *, const char *, const struct pam_conv *, const char *,
                      pam_handle_t **);
int pam_authenticate(pam_handle_t *, int);
int pam_end(pam_handle_t *, int);
#define V(x) printf("%ld\n", (long)(x))
int main(void) {
    V(PAM_SUCCESS); V(PAM_BUF_ERR); V(PAM_AUTH_ERR); V(PAM_MAXTRIES); V(PAM_CONV_ERR);
    V(PAM_PROMPT_ECHO_OFF); V(PAM_PROMPT_ECHO_ON); V(PAM_ERROR_MSG);
    V(sizeof(struct pam_message)); V(_Alignof(struct pam_message));
    V(offsetof(struct pam_message, msg_style)); V(offsetof(struct pam_message, msg));
    V(sizeof(struct pam_response)); V(_Alignof(struct pam_response));
    V(offsetof(struct pam_response, resp)); V(offsetof(struct pam_response, resp_retcode));
    V(sizeof(struct pam_conv)); V(_Alignof(struct pam_conv));
    V(offsetof(struct pam_conv, conv)); V(offsetof(struct pam_conv, appdata_ptr));
    return 0;
}
"#,
        )
        .unwrap();
        let built = std::process::Command::new("gcc")
            .args(["-Wall", "-Werror", "-o"])
            .arg(&exe)
            .arg(&src)
            .status()
            .is_ok_and(|s| s.success());
        if !built {
            crate::testenv::unavailable(
                "cannot build the PAM layout probe (needs gcc and the libpam headers)",
            );
            return;
        }
        let out = std::process::Command::new(&exe).output().unwrap();
        let c: Vec<i64> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| l.parse().unwrap())
            .collect();
        let rust = [
            PAM_SUCCESS as i64,
            PAM_BUF_ERR as i64,
            PAM_AUTH_ERR as i64,
            PAM_MAXTRIES as i64,
            PAM_CONV_ERR as i64,
            PAM_PROMPT_ECHO_OFF as i64,
            PAM_PROMPT_ECHO_ON as i64,
            PAM_ERROR_MSG as i64,
            size_of::<pam_message>() as i64,
            align_of::<pam_message>() as i64,
            offset_of!(pam_message, msg_style) as i64,
            offset_of!(pam_message, msg) as i64,
            size_of::<pam_response>() as i64,
            align_of::<pam_response>() as i64,
            offset_of!(pam_response, resp) as i64,
            offset_of!(pam_response, resp_retcode) as i64,
            size_of::<pam_conv>() as i64,
            align_of::<pam_conv>() as i64,
            offset_of!(pam_conv, conv) as i64,
            offset_of!(pam_conv, appdata_ptr) as i64,
        ];
        assert_eq!(c, rust);
    }

    #[test]
    fn start_failures_and_relative_config_directory() {
        let d = fixture("fail", "auth required pam_permit.so\n");
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "config: 'missing'")).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        assert!(!rt.get(r, "started").truthy());
        let mut rt = Runtime::new(None);
        let r = rt.load_str(&client(&d, "user: 'no-such-user-ri'")).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        assert!(!rt.get(r, "started").truthy());
        rt.poll_native();
        assert_eq!(rt.get(r, "log").to_js_string(), "");

        // configDirectory resolves against the declaring file (the original: `configDirectory: "pam"`).
        let qml = d.join("Lock.qml");
        std::fs::create_dir_all(d.join("pam")).unwrap();
        std::fs::copy(d.join("t"), d.join("pam/t")).unwrap();
        std::fs::write(&qml, client(std::path::Path::new("pam"), "")).unwrap();
        let mut rt = Runtime::new(None);
        let r = rt.load_file(&qml).unwrap();
        rt.settle();
        step(&mut rt, r, 1.0);
        assert!(rt.get(r, "started").truthy());
        let pam = rt.id("pam").unwrap();
        assert_eq!(
            rt.get(pam, "configDirectory").to_js_string(),
            d.join("pam").to_string_lossy()
        );
        assert_eq!(run(&mut rt, r, "done"), "done:0");
    }

    /// A relative declaring file (`rusty-impulse -p rel/Lock.qml`) resolves `configDirectory` once to an
    /// absolute path; it used to stay relative and be joined again on every poll, growing.
    #[test]
    fn relative_file_resolves_config_directory_once() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str("import Rustshell.Services.Pam\nPamContext { configDirectory: 'pam' }")
            .unwrap();
        {
            let mut w = rt.world.borrow_mut();
            let ctx = w.items[r].ctx.unwrap() as usize;
            w.ctx_files.insert(ctx, "rel/lock/Lock.qml".into());
        }
        for _ in 0..5 {
            rt.world.borrow_mut().revision += 1;
            rt.poll_native();
        }
        let want = std::env::current_dir().unwrap().join("rel/lock/pam");
        assert_eq!(
            rt.get(r, "configDirectory").to_js_string(),
            want.to_string_lossy()
        );
    }

    #[test]
    fn fixtures_sharing_a_name_get_their_own_directory() {
        assert_ne!(fixture("same", ""), fixture("same", ""));
    }

    /// System modules only (no real accounts): prints each result so builds can be diffed.
    #[test]
    fn system_permit_deny() {
        for (name, conf) in [
            ("permit", "auth required pam_permit.so\n"),
            ("deny", "auth required pam_deny.so\n"),
            (
                "deny_then_permit",
                "auth sufficient pam_deny.so\nauth required pam_permit.so\n",
            ),
            (
                "missing_module",
                "auth required pam_the_original_does_not_exist.so\n",
            ),
        ] {
            let d = fixture(name, conf);
            let mut rt = Runtime::new(None);
            let r = rt.load_str(&client(&d, "")).unwrap();
            rt.settle();
            step(&mut rt, r, 1.0);
            println!("PAMLIVE {name} => {}", run(&mut rt, r, "done"));
        }
    }
}
