//! The shell's panic hook changes what a panic writes, not how the process ends: a panic of
//! the main thread ends it with code 101, one of another thread leaves it running, as with
//! std's hook. With stderr closed or its reader gone the hook returns all the same, and the
//! record is in the instance log.
//!
//! The process that panics is this test binary (`harness = false`): only so is the panic in
//! the main thread the process's own, not one of libtest's test threads.

use std::process::{Command, ExitStatus, Stdio};

/// What the child does, a `+`-separated list: `hook`, `closed`, and `worker` or `main`.
const CHILD: &str = "RI_PANIC_ENDS";
const RUN_DIR: &str = "RI_PANIC_ENDS_RUN_DIR";
/// What a process that went on after its worker's panic writes to stdout.
const WENT_ON: &str = "the worker's panic was joined\n";

fn child(does: &str) {
    let does = |what| does.split('+').any(|d| d == what);
    if does("hook") {
        shell::panics::log_whole();
    }
    if let Some(run_dir) = std::env::var_os(RUN_DIR) {
        let run_dir = std::path::Path::new(&run_dir);
        shell::rilog::start_instance_log("shell.qml", "id", "id", run_dir, false, None);
    }
    if does("closed") {
        // SAFETY: closes this process's stderr; nothing here holds the descriptor open.
        let closed = unsafe { libc::close(libc::STDERR_FILENO) };
        assert_eq!(closed, 0);
    }
    if does("main") {
        panic!("the main thread gave up");
    }
    let worker = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("the worker gave up"))
        .unwrap();
    assert!(worker.join().is_err());
    print!("{WENT_ON}");
}

enum Stderr {
    /// A pipe this test reads.
    Read,
    /// The child closes it before it panics: `EBADF`.
    Closed,
    /// A pipe nobody reads: `EPIPE`.
    NoReader,
}

struct Ended {
    status: ExitStatus,
    stdout: String,
    stderr: String,
    log: String,
}

fn panic_in(thread: &str, hook: bool, stderr: Stderr) -> Ended {
    let dir = tempfile::tempdir().unwrap();
    let mut does = vec![thread];
    if hook {
        does.push("hook");
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env(RUN_DIR, dir.path())
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    match stderr {
        Stderr::Read => {
            command.stderr(Stdio::piped());
        }
        Stderr::Closed => {
            does.push("closed");
            command.stderr(Stdio::null());
        }
        Stderr::NoReader => {
            let (reader, writer) = std::io::pipe().unwrap();
            drop(reader);
            command.stderr(writer);
        }
    }
    let shell = command.env(CHILD, does.join("+")).spawn().unwrap();
    // The pipe nobody reads is closed here too: only the child holds its writing end.
    drop(command);
    let out = shell.wait_with_output().unwrap();
    Ended {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        log: std::fs::read_to_string(dir.path().join("log.log")).unwrap(),
    }
}

/// Code 101 after a panic of the main thread; code 0 with the process gone on after a panic
/// of another thread.
fn ends_as_rust_does(thread: &str, ended: &Ended) {
    use std::os::unix::process::ExitStatusExt as _;
    let (code, stdout) = match thread {
        "main" => (101, ""),
        _ => (0, WENT_ON),
    };
    assert_eq!(ended.status.signal(), None, "{thread}");
    assert_eq!(ended.status.code(), Some(code), "{thread}");
    assert_eq!(ended.stdout, stdout, "{thread}");
}

fn records(text: &str, thread: &str) -> usize {
    let who = format!("thread '{thread}' (");
    let said = "panicked at crates/shell/tests/panic_ends.rs:";
    text.lines()
        .filter(|l| l.contains(&who) && l.contains(said))
        .count()
}

fn a_panic_ends_the_process_as_with_the_hook_of_std() {
    for thread in ["worker", "main"] {
        let std = panic_in(thread, false, Stderr::Read);
        ends_as_rust_does(thread, &std);
        assert_eq!(records(&std.stderr, thread), 1, "{}", std.stderr);
        assert_eq!(records(&std.log, thread), 0, "{}", std.log);

        let ours = panic_in(thread, true, Stderr::Read);
        ends_as_rust_does(thread, &ours);
        assert_eq!(ours.status, std.status, "{thread}");
        assert_eq!(records(&ours.stderr, thread), 1, "{}", ours.stderr);
        assert_eq!(records(&ours.log, thread), 1, "{}", ours.log);
    }
}

fn a_panic_with_stderr_closed_is_in_the_log_and_ends_the_same() {
    for thread in ["worker", "main"] {
        let ended = panic_in(thread, true, Stderr::Closed);
        ends_as_rust_does(thread, &ended);
        assert_eq!(records(&ended.log, thread), 1, "{}", ended.log);
    }
}

fn a_panic_with_no_reader_of_stderr_is_in_the_log_and_ends_the_same() {
    for thread in ["worker", "main"] {
        let ended = panic_in(thread, true, Stderr::NoReader);
        ends_as_rust_does(thread, &ended);
        assert_eq!(records(&ended.log, thread), 1, "{}", ended.log);
    }
}

const TESTS: &[(&str, fn())] = &[
    (
        "a_panic_ends_the_process_as_with_the_hook_of_std",
        a_panic_ends_the_process_as_with_the_hook_of_std,
    ),
    (
        "a_panic_with_stderr_closed_is_in_the_log_and_ends_the_same",
        a_panic_with_stderr_closed_is_in_the_log_and_ends_the_same,
    ),
    (
        "a_panic_with_no_reader_of_stderr_is_in_the_log_and_ends_the_same",
        a_panic_with_no_reader_of_stderr_is_in_the_log_and_ends_the_same,
    ),
];

fn main() {
    if let Ok(does) = std::env::var(CHILD) {
        return child(&does);
    }
    // libtest's command line and output, as cargo test and cargo-nextest use them
    // (https://nexte.st/docs/design/custom-test-harnesses/).
    let args = libtest_mimic::Arguments::from_args();
    let tests = TESTS
        .iter()
        .map(|&(name, test)| {
            libtest_mimic::Trial::test(name, move || {
                test();
                Ok(())
            })
        })
        .collect();
    libtest_mimic::run(&args, tests).exit();
}
