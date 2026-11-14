//! A log line reaches stderr whole: as one write(2) between the lines another thread writes,
//! and complete on a stderr that does not block and takes no more for a while (`EAGAIN`).
//!
//! Each test starts itself again as the writer, with a pipe as its stderr, and reads the pipe.

use std::io::{BufRead as _, Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::process::{Command, Stdio};

const WRITER: &str = "ELOG_TEST_WRITER";

fn writer(test: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", test, "--nocapture"])
        .env(WRITER, "1")
        .stdin(Stdio::null());
    c
}

/// The line of a thread that does not log through `elog`, as libwayland's `WAYLAND_DEBUG`
/// lines: one write(2) to stderr.
fn other_line(i: usize) -> String {
    format!("[other] wl_display#1.delete_id({i})\n")
}

/// Formatted in the middle of a log line: lets the other thread write its line and waits
/// until it has.
struct WhileOtherWrites<'a> {
    go: &'a std::sync::mpsc::Sender<()>,
    done: &'a std::sync::mpsc::Receiver<()>,
}

impl std::fmt::Display for WhileOtherWrites<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.go.send(()).unwrap();
        self.done.recv().unwrap();
        f.write_str("aaa")
    }
}

const LINES: usize = 200;

#[test]
fn a_line_is_whole_between_the_lines_of_another_thread() {
    const NAME: &str = "a_line_is_whole_between_the_lines_of_another_thread";
    if std::env::var_os(WRITER).is_some() {
        let (go, start) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let other = std::thread::spawn(move || {
            for i in 0..LINES {
                start.recv().unwrap();
                let line = other_line(i);
                // SAFETY: write(2) of the `line.len()` bytes at `line` to stderr.
                let n = unsafe { libc::write(2, line.as_ptr().cast(), line.len()) };
                assert_eq!(n, line.len() as isize);
                finished.send(()).unwrap();
            }
        });
        for i in 0..LINES {
            let between = WhileOtherWrites {
                go: &go,
                done: &done,
            };
            elog::eprintln!("console: TXT {i} {between} end");
        }
        other.join().unwrap();
        std::process::exit(0);
    }

    let (mut r, w) = std::io::pipe().unwrap();
    let mut child = writer(NAME)
        .stdout(Stdio::null())
        .stderr(w)
        .spawn()
        .unwrap();
    let mut got = String::new();
    r.read_to_string(&mut got).unwrap();
    assert!(child.wait().unwrap().success());

    let lines: Vec<&str> = got.split_inclusive('\n').collect();
    let torn: Vec<&&str> = lines
        .iter()
        .filter(|l| {
            !(l.starts_with("[other] ") || l.starts_with("console: "))
                || !l.ends_with(")\n") && !l.ends_with(" end\n")
        })
        .collect();
    assert!(
        torn.is_empty(),
        "{} torn lines, the first: {:?}",
        torn.len(),
        torn[0]
    );
    // Each thread's line was written during the formatting of the other's, so before it.
    let want: String = (0..LINES)
        .map(|i| format!("{}console: TXT {i} aaa end\n", other_line(i)))
        .collect();
    assert_eq!(got, want);
}

/// Says on stdout that the line it is part of has been formatted.
struct Formatted;

impl std::fmt::Display for Formatted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        say("formatted");
        f.write_str("line")
    }
}

fn say(what: &str) {
    let mut o = std::io::stdout().lock();
    writeln!(o, "{what}").unwrap();
    o.flush().unwrap();
}

/// What the writer of the non-blocking test logs: short lines, then one longer than the pipe
/// holds (so it takes several writes), then a last short one.
fn log_lines() -> Vec<String> {
    let mut lines = vec!["first line".to_string()];
    lines.extend((0..2000).map(|i| format!("line {i} {}", "x".repeat(i % 200))));
    lines.push("L".repeat(300_000));
    lines.push("last".to_string());
    lines
}

#[test]
fn a_stderr_that_does_not_block_gets_every_line() {
    const NAME: &str = "a_stderr_that_does_not_block_gets_every_line";
    if std::env::var_os(WRITER).is_some() {
        let lines = log_lines();
        elog::eprintln!("first {Formatted}");
        say("returned");
        // Every other line as `RI_LOG` events are written: through [`elog::Stderr`].
        for (i, l) in lines[1..].iter().enumerate() {
            if i % 2 == 0 {
                elog::eprintln!("{l}");
            } else {
                elog::Stderr.write_all(format!("{l}\n").as_bytes()).unwrap();
            }
        }
        std::process::exit(0);
    }

    let (mut r, mut w) = std::io::pipe().unwrap();
    // SAFETY: fcntl(2) on the write end this test owns; the flags are those just read.
    let flags = unsafe { libc::fcntl(w.as_raw_fd(), libc::F_GETFL) };
    // SAFETY: as above.
    let set = unsafe { libc::fcntl(w.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    assert_eq!(set, 0);
    // Nobody reads the pipe yet: it is full, the writer's first write gets `EAGAIN`.
    let mut filler = 0;
    for chunk in [&[b'#'; 4096][..], b"#"] {
        loop {
            match w.write(chunk) {
                Ok(n) => filler += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => panic!("{e}"),
            }
        }
    }
    assert!(filler > 0);

    let mut child = writer(NAME)
        .stdout(Stdio::piped())
        .stderr(w)
        .spawn()
        .unwrap();
    // The pipe is read once the first line is formatted (it is written next, to the full
    // pipe), or once the call has returned without formatting it.
    let mut said = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
    let said = said
        .find(|l| matches!(l.as_deref(), Ok("formatted" | "returned")))
        .unwrap()
        .unwrap();
    let mut got = Vec::new();
    r.read_to_end(&mut got).unwrap();
    assert!(child.wait().unwrap().success());

    let got = String::from_utf8(got.split_off(filler)).unwrap();
    let want: String = log_lines().iter().map(|l| format!("{l}\n")).collect();
    let (got_lines, want_lines) = (got.lines().count(), want.lines().count());
    assert!(
        got == want,
        "{got_lines} of {want_lines} lines arrived ({} of {} bytes), the writer said {said:?}",
        got.len(),
        want.len()
    );
}
