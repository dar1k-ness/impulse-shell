//! A panic of the shell is one record: one write(2) to stderr, and the same text in the
//! instance log by the time the panic hook returns.

use std::io::Read as _;
use std::os::fd::{FromRawFd as _, OwnedFd};
use std::process::{Command, Stdio};

const CHILD: &str = "RI_PANIC_TEST_RUN_DIR";
const TEST: &str = "a_panic_is_one_record_on_stderr_and_in_the_instance_log";
const NOTE: &str =
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n";

/// Two lines, and longer than the 512 bytes std's hook writes at once.
fn message() -> String {
    format!("the worker gave up\n{}", "x".repeat(700))
}

/// The shell as far as a panic goes: the hook, the instance log in `run_dir`, and a thread
/// that panics.
fn child(run_dir: &std::ffi::OsStr) -> ! {
    shell::panics::log_whole();
    let run_dir = std::path::Path::new(run_dir);
    shell::rilog::start_instance_log("shell.qml", "id", "id", run_dir, false, None);
    // This thread holds std's lock on stderr and waits for the one that panics: a hook that
    // took the lock would never return.
    let _held = std::io::stderr().lock();
    let worker = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("{}", message()))
        .unwrap();
    let _ = worker.join();
    // Nothing flushes or closes the log here, as when the shell aborts: what the files hold
    // is what the hook wrote.
    // SAFETY: _exit(2) ends the process and does not return.
    unsafe { libc::_exit(0) }
}

/// A socket pair that keeps each write(2) apart: the reader gets one packet per write.
fn packets() -> (std::fs::File, OwnedFd) {
    let mut fds = [0; 2];
    let kind = libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC;
    // SAFETY: `fds` is room for the two descriptors socketpair(2) creates.
    let made = unsafe { libc::socketpair(libc::AF_UNIX, kind, 0, fds.as_mut_ptr()) };
    assert_eq!(made, 0, "socketpair: {}", std::io::Error::last_os_error());
    // SAFETY: socketpair just created this descriptor; nothing else owns it.
    let ours = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    // SAFETY: as above, for the other end.
    let theirs = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    (ours, theirs)
}

/// Runs the child: each write(2) it made to stderr, and its `log.log`.
fn panic_in_a_shell(backtrace: bool) -> (Vec<String>, String) {
    let dir = tempfile::tempdir().unwrap();
    let (mut ours, theirs) = packets();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, dir.path())
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(theirs);
    if backtrace {
        command.env("RUST_BACKTRACE", "1");
    }
    let mut shell = command.spawn().unwrap();
    // The child's stderr is closed here: the read below ends when the child does.
    drop(command);
    let mut writes = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match ours.read(&mut buf).unwrap() {
            0 => break,
            n => writes.push(String::from_utf8_lossy(&buf[..n]).into_owned()),
        }
    }
    assert!(shell.wait().unwrap().success());
    let log = std::fs::read_to_string(dir.path().join("log.log")).unwrap();
    (writes, log)
}

/// The record in the one write: between the newlines, without std's hint `note`.
fn the_record<'a>(writes: &'a [String], note: &str) -> &'a str {
    assert_eq!(writes.len(), 1, "writes to stderr: {writes:#?}");
    let record = writes[0]
        .strip_prefix('\n')
        .and_then(|t| t.strip_suffix(note))
        .and_then(|t| t.strip_suffix('\n'))
        .unwrap_or_else(|| panic!("not a panic's report: {:?}", writes[0]));
    let head = record.split_once(":\n").unwrap().0;
    assert!(
        head.starts_with("thread 'worker' (")
            && head.contains(") panicked at crates/shell/tests/panic_whole.rs:"),
        "{head:?}"
    );
    record
}

#[test]
fn a_panic_is_one_record_on_stderr_and_in_the_instance_log() {
    if let Some(run_dir) = std::env::var_os(CHILD) {
        child(&run_dir);
    }
    let (writes, log) = panic_in_a_shell(false);
    let record = the_record(&writes, NOTE);
    assert_eq!(record.split_once(":\n").unwrap().1, message());
    assert_eq!(log.matches(record).count(), 1, "log.log: {log:?}");
    let line = log.lines().find(|l| l.contains("panicked at")).unwrap();
    assert!(line.contains(" ERROR"), "{line:?}");
}

/// With `RUST_BACKTRACE=1` the backtrace is in the same write and in the log, and the hint
/// to set the variable is not.
#[test]
fn a_backtrace_is_part_of_the_record() {
    let (writes, log) = panic_in_a_shell(true);
    let record = the_record(&writes, "");
    let (said, stack) = record.split_once("\nstack backtrace:\n").unwrap();
    assert_eq!(said.split_once(":\n").unwrap().1, message());
    assert!(stack.contains("panic_whole::child"), "{stack}");
    assert!(!record.contains("note:"), "{record}");
    assert_eq!(log.matches(record).count(), 1, "log.log: {log:?}");
}
