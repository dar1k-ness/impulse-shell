//! `scripts/musicRecognition/recognize-music.sh`: the audio source from PipeWire (no
//! `pactl`), the timeout and the JSON-line scan here (no FIFO, `sleep`, `grep`). `songrec`
//! still runs as a program.

use super::Io;
use std::io::Read;
use std::os::fd::{AsFd, AsRawFd};
use std::process::Stdio;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub interval: String,
    pub total: String,
    pub source: String,
}

/// PipeWire `default.audio.sink`, `default.audio.source` (what `pactl get-default-sink` and
/// `pactl info`'s Default Source print), once the daemon's initial state is known (empty
/// when no daemon could be reached); `None` when `stop` came first.
pub fn default_names(stop: &std::sync::Arc<crate::svc::Stop>) -> Option<[String; 4]> {
    crate::pw::wait_snapshot_or_stop(stop).map(|s| s.defaults)
}

/// `pactl list short sources` names: pipewire-pulse's `stream_type_from_media_class`, so
/// every sink (and duplex node) gives its monitor and `Audio/Source/Virtual` counts as a
/// source, though quickshell's `PwNodeType` leaves it untyped.
pub fn source_names(s: &crate::pw::Snap) -> Vec<String> {
    let mut v = vec![];
    for n in &s.nodes {
        let class = n
            .props
            .iter()
            .find(|(k, _)| k == "media.class")
            .map(|(_, c)| c.as_str());
        let class = class.unwrap_or(match n.ty {
            17 => "Audio/Sink",
            9 => "Audio/Source",
            25 => "Audio/Duplex",
            _ => "",
        });
        match class {
            "Audio/Sink" | "Audio/Duplex" => v.push(format!("{}.monitor", n.name)),
            "Audio/Source" | "Audio/Source/Virtual" => v.push(n.name.clone()),
            _ => {}
        }
    }
    v
}

/// The device the script would pick, or the exit code it fails with (and its stdout).
pub fn pick(source: &str, s: Option<&crate::pw::Snap>) -> Result<String, (i32, &'static str)> {
    let d = s.map(|s| s.defaults.clone()).unwrap_or_default();
    let dev = match source {
        "monitor" => format!("{}.monitor", d[0]),
        "input" => d[1].clone(),
        _ => return Err((1, "Invalid source type\n")),
    };
    // `grep -q "$AUDIO_DEVICE"`: a match anywhere in a line.
    if dev.is_empty()
        || !s
            .map(source_names)
            .unwrap_or_default()
            .iter()
            .any(|n| grep_match(n, &dev))
    {
        return Err((1, ""));
    }
    Ok(dev)
}

/// Basic-regex `grep` of a device name: `.` matches any character, the rest literally.
fn grep_match(line: &str, pat: &str) -> bool {
    let (l, p): (Vec<char>, Vec<char>) = (line.chars().collect(), pat.chars().collect());
    (0..=l.len().saturating_sub(p.len()))
        .any(|i| l.len() >= p.len() && p.iter().zip(&l[i..]).all(|(&a, &b)| a == '.' || a == b))
}

/// When a `-t` of `total` seconds ends, counted from `start`; `None` when it never does: the
/// script's `sleep "$TOTAL_DURATION"` sleeps for good on `inf` and on a number of seconds no
/// clock can hold (`1e400`, `1e19`).
fn deadline(start: Instant, total: &str) -> Option<Instant> {
    let secs = total.parse::<f64>().unwrap_or(0.0).max(0.0);
    start.checked_add(Duration::try_from_secs_f64(secs).ok()?)
}

/// The first line of `out` with a match, read until EOF or `deadline` (`None`: no deadline),
/// whichever comes first.
fn first_match(mut out: impl Read + AsFd, deadline: Option<Instant>) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        while let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            let line = String::from_utf8_lossy(&line[..i]);
            if line.contains("\"matches\": [") {
                return Some(line.into_owned());
            }
        }
        if !readable_before(&out, deadline) {
            return None;
        }
        match out.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

/// Sleeps until `fd` is readable (data, EOF or error) or `deadline` passes; which came first.
fn readable_before(fd: &impl AsFd, deadline: Option<Instant>) -> bool {
    loop {
        let ms = deadline.map_or(-1, |d| {
            let left = d.saturating_duration_since(Instant::now());
            i32::try_from(left.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX)
        });
        let mut p = libc::pollfd {
            fd: fd.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        match unsafe { libc::poll(&mut p, 1, ms) } {
            n if n > 0 => return true,
            0 if deadline.is_some_and(|d| Instant::now() >= d) => return false,
            0 => {}
            _ if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted => {}
            _ => return true,
        }
    }
}

/// `songrec listen` on `dev`, a request every `interval` seconds.
pub(crate) fn listen(dev: &str, interval: &str) -> std::process::Command {
    let mut c = workpool::userenv::command("songrec");
    c.args([
        "listen",
        "--audio-device",
        dev,
        "--request-interval",
        interval,
        "--json",
        "--disable-mpris",
    ]);
    c
}

pub fn run(a: &Args, io: &Io) -> i32 {
    if !crate::osutil::on_path("songrec") {
        return 1;
    }
    // Stopped before the daemon's state is known: the Process already reported its exit.
    let Some(s) = crate::pw::wait_snapshot_or_stop(&io.stop) else {
        return 143;
    };
    let dev = match pick(&a.source, Some(&s)) {
        Ok(d) => d,
        Err((code, out)) => {
            if !out.is_empty() {
                io.out(out);
            }
            return code;
        }
    };
    let Ok(mut ch) = crate::child::spawn(
        listen(&dev, &a.interval)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    ) else {
        return 1;
    };
    let pid = ch.id() as i32;
    let out = ch.child_mut().stdout.take();
    // Process stop: songrec is ended, which ends the read below; the timeout ends the read.
    let deadline = deadline(Instant::now(), &a.total);
    let watch = super::watch_kill(pid, io.stop.clone());
    if let Some(line) = out.and_then(|o| first_match(o, deadline)) {
        io.out(format!("{line}\n"));
    }
    drop(watch);
    // SAFETY: kill(2) takes plain integers and touches no memory of this process.
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let _ = ch.wait();
    0
}

#[cfg(test)]
mod read_tests {
    use super::{deadline, first_match};
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    #[test]
    fn first_match_skips_other_lines_and_stops_at_eof_or_the_deadline() {
        let (mut w, r) = UnixStream::pair().unwrap();
        w.write_all(b"{\"x\": 1}\n{\"matches\": [1], \"t\": \"\xc3")
            .unwrap();
        w.write_all(b"\xa9\"}\nlater\n").unwrap();
        let far = Some(Instant::now() + Duration::from_secs(3600));
        assert_eq!(
            first_match(r, far).as_deref(),
            Some("{\"matches\": [1], \"t\": \"\u{e9}\"}")
        );

        let (mut w, r) = UnixStream::pair().unwrap();
        w.write_all(b"{\"x\": 1}\n{\"matches\": [").unwrap();
        drop(w);
        assert_eq!(first_match(r, far), None, "a match cut off by EOF");

        // Nothing written and the writer open: only the deadline ends the read.
        let (_w, r) = UnixStream::pair().unwrap();
        assert_eq!(first_match(r, Some(Instant::now())), None);
    }

    /// `-t inf`, `-t 1e400` and `-t 1e19` panicked (`Duration::from_secs_f64` of an infinity,
    /// `Instant + Duration` overflow): like the script's `sleep`, they never end the listening.
    /// A match still ends it, and so does songrec's exit.
    #[test]
    fn a_total_no_clock_can_hold_is_no_deadline() {
        let start = Instant::now();
        for t in ["inf", "1e400", "1e19", "infinity"] {
            assert_eq!(deadline(start, t), None, "-t {t}");
        }
        assert_eq!(deadline(start, "30"), Some(start + Duration::from_secs(30)));
        assert_eq!(
            deadline(start, "0.5"),
            Some(start + Duration::from_millis(500))
        );
        // As before: what is not a number, and a negative one, end it at once.
        assert_eq!(deadline(start, "abc"), Some(start));
        assert_eq!(deadline(start, "-3"), Some(start));

        let (mut w, r) = UnixStream::pair().unwrap();
        w.write_all(b"{\"matches\": [1]}\n").unwrap();
        assert_eq!(first_match(r, None).as_deref(), Some("{\"matches\": [1]}"));
        let (w, r) = UnixStream::pair().unwrap();
        drop(w);
        assert_eq!(first_match(r, None), None, "EOF with no deadline");
    }
}
