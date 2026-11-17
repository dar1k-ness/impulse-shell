//! Work the shell does in its own process where a desktop usually runs a small tool: the
//! clipboard (`wl-copy`, `cliphist`), key events (`ydotool`), screenshots (`grim`, `magick`),
//! screen recording, song recognition and notifications. Standalone programs (`cliphist`,
//! `wf-recorder`, `songrec`, `tesseract`, `swappy`/`satty`) still run as processes.
//!
//! A [`Job`] runs on its own thread with the stdout / exit code contract of a child process,
//! so a slow one (a recording, an upload) never blocks another.

pub mod clip;
pub mod clipsrc;
pub mod record;
pub mod screen;
pub mod shot;
pub mod songrec;
pub mod vkbd;
mod wl;

use crate::native::ProcEv;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Work started from QML, run off the frame thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Clip(clip::Job),
    Shot(shot::Job),
    Record(record::Args),
    SongRec(songrec::Args),
    Screen(screen::Job),
}

/// Output side of a running job: what a child process would write and its exit status.
pub struct Io {
    pub tx: Option<crate::svc::Sender<ProcEv>>,
    pub stop: Arc<crate::svc::Stop>,
}

impl Io {
    pub fn detached() -> Io {
        Io {
            tx: None,
            stop: Default::default(),
        }
    }

    pub fn out(&self, s: impl Into<String>) {
        if let Some(t) = &self.tx {
            let _ = t.send(ProcEv::Out(s.into()));
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

impl Job {
    /// Runs on the calling thread; returns the exit code the original command would have.
    pub fn run(self, io: &Io) -> i32 {
        match self {
            Job::Clip(j) => j.run(io),
            Job::Shot(j) => j.run(io),
            Job::Record(a) => record::run(&a, io),
            Job::SongRec(a) => songrec::run(&a, io),
            Job::Screen(j) => j.run(io),
        }
    }

    /// Starts the job on a [`workpool::long`] thread; `Exit` is sent even if it panics.
    pub fn start(self, tx: Option<crate::svc::Sender<ProcEv>>) -> Arc<crate::svc::Stop> {
        self.start_under(tx, Default::default())
    }

    /// [`Job::start`] for a job `stop` ends: one a Process does not own is given its World's
    /// (`World::jobs_stop`), so it ends with the World.
    pub fn start_under(
        self,
        tx: Option<crate::svc::Sender<ProcEv>>,
        stop: Arc<crate::svc::Stop>,
    ) -> Arc<crate::svc::Stop> {
        let io = Io {
            tx,
            stop: stop.clone(),
        };
        workpool::long(move || {
            let code = crate::jobs::caught(|| crate::child::with_stop(&io.stop, || self.run(&io)));
            if let Some(t) = &io.tx {
                let _ = t.send(ProcEv::Exit(code));
            }
        });
        stop
    }
}

/// How long a `ScreencopyView`'s capture waits for the compositor ([`capture_output`]). A
/// frame is copied within a few refreshes of the output; the window that shows the capture
/// draws nothing until it ends, so a compositor that never answers must not hold it.
pub const CAPTURE_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);

/// One output's RGBA8 pixels: a frozen `ScreencopyView { captureSource: screen }`, with the
/// cursor on it when `cursor` (its `paintCursor`). Fails when the compositor has not answered
/// within [`CAPTURE_LIMIT`].
pub fn capture_output(name: &str, cursor: bool) -> Result<(u32, u32, Vec<u8>), String> {
    let img = wl::capture_within(Some(name), cursor, CAPTURE_LIMIT)?;
    Ok((img.width(), img.height(), img.into_raw()))
}

/// `printf`-free `date '+%Y-%m-%d_%H.%M.%S'`.
pub fn date_stamp() -> String {
    let now = crate::civil::now_utc_secs();
    date_stamp_at(now + i64::from(crate::tz::local_ahead(now)))
}

/// [`date_stamp`] of the local date and time `local`, in seconds as if it were UTC.
pub fn date_stamp_at(local: i64) -> String {
    let at = crate::civil::DateTime::from_timestamp(local, 0).expect("a time in the years held");
    let (d, t) = (at.date, at.time);
    // `%Y` is four digits, and signed past them.
    let year = match d.year() {
        y @ 0..=9999 => format!("{y:04}"),
        y => format!("{y:+05}"),
    };
    let (month, day) = (d.month(), d.day());
    let (hour, minute, second) = (t.hour(), t.minute(), t.second());
    format!("{year}-{month:02}-{day:02}_{hour:02}.{minute:02}.{second:02}")
}

/// `notify-send <summary> <body> -a <app>` over D-Bus from this process, as libnotify's
/// `notify-send` sends it ([`crate::libnotify::notify`]).
pub fn notify(summary: &str, body: &str, app: &str) {
    let (s, b, a) = (summary.to_owned(), body.to_owned(), app.to_owned());
    workpool::run(move || {
        if let Some(c) = crate::dbus::session() {
            notify_on(&c, &s, &b, &a);
        }
    });
}

fn notify_on(c: &zbus::blocking::Connection, summary: &str, body: &str, app: &str) {
    crate::libnotify::notify(c, summary, body, app);
}

#[cfg(test)]
pub(crate) mod tests;

/// Runs `cmd` to its exit as [`crate::child::spawn`] starts it: leading its own group and
/// tracked, so the shell's exit and a reload end it, and the job's stop (`running = false`)
/// SIGTERMs its group. Its stdout and stderr are not collected (see [`crate::child::run`] for
/// that), so a program it leaves behind holding them does not keep the job waiting. `None`
/// when it could not start or the job was stopped.
pub fn status(
    cmd: &mut std::process::Command,
    stop: &Arc<crate::svc::Stop>,
) -> Option<std::process::ExitStatus> {
    let ch = crate::child::spawn(
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null()),
    )
    .ok()?;
    let watch = watch_kill(-(ch.id() as i32), stop.clone());
    let st = ch.wait();
    drop(watch);
    st.ok()
        .filter(|_| !stop.load(std::sync::atomic::Ordering::Acquire))
}

/// Ends `pid` (SIGTERM) when `stop` is set (SIGKILL when it is a kill), with no thread
/// waiting: the stop wakeup signals at once. Dropping the returned guard ends the watch, so a
/// later stop cannot signal a reused pid.
pub fn watch_kill(pid: i32, stop: Arc<crate::svc::Stop>) -> Watch {
    Watch(stop.target(pid, Some(libc::SIGTERM)))
}

/// See [`watch_kill`].
pub struct Watch(Arc<crate::svc::Target>);

impl Drop for Watch {
    fn drop(&mut self) {
        self.0.disarm();
    }
}

#[cfg(test)]
mod stamp_tests {
    use super::date_stamp_at;

    /// `date '+%Y-%m-%d_%H.%M.%S'`: two digits each, the year four and signed past them.
    #[test]
    fn a_stamp_is_the_date_and_the_time_in_digits() {
        for (at, stamp) in [
            (0, "1970-01-01_00.00.00"),
            (-1, "1969-12-31_23.59.59"),
            (1_794_924_123, "2026-11-17_14.02.03"),
            (951_782_400 + 86_399, "2000-02-29_23.59.59"),
            (-30_636_291_111, "0999-03-05_07.08.09"),
            (-62_167_219_200, "0000-01-01_00.00.00"),
            (-62_167_219_201, "-0001-12-31_23.59.59"),
            (253_402_300_799, "9999-12-31_23.59.59"),
            (253_402_300_800, "+10000-01-01_00.00.00"),
        ] {
            assert_eq!(date_stamp_at(at), stamp, "{at}");
        }
    }
}

#[cfg(test)]
mod watch_tests {
    use super::watch_kill;
    use std::os::unix::process::CommandExt;
    use std::os::unix::process::ExitStatusExt;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    /// A child that runs until a signal ends it.
    fn sleeper() -> std::process::Child {
        std::process::Command::new("sleep")
            .arg("infinity")
            .spawn()
            .unwrap()
    }

    /// A [`sleeper`] with SIGTERM blocked: a SIGTERM sent to it stays pending ([`term_pending`]).
    fn term_blocked() -> std::process::Child {
        let mut c = std::process::Command::new("sleep");
        c.arg("infinity");
        let block_term = || {
            // SAFETY: an all-zero sigset_t is a valid value; sigemptyset initializes it below.
            let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
            // SAFETY: `set` is a valid sigset_t owned by this frame.
            unsafe { libc::sigemptyset(&mut set) };
            // SAFETY: as above.
            unsafe { libc::sigaddset(&mut set, libc::SIGTERM) };
            // SAFETY: `set` is valid and the old mask is not asked for (null).
            if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        };
        // SAFETY: `block_term` runs in the forked child before execve and only calls the
        // async-signal-safe sigemptyset/sigaddset/sigprocmask on a local set; the blocked mask
        // is kept across execve(2).
        unsafe { c.pre_exec(block_term) };
        c.spawn().unwrap()
    }

    /// Whether a SIGTERM is pending for `ch` (`ShdPnd` in proc_pid_status(5)): kill(2) has
    /// queued it by the time it returned, so no wait is involved.
    fn term_pending(ch: &std::process::Child) -> bool {
        let st = std::fs::read_to_string(format!("/proc/{}/status", ch.id())).unwrap();
        let mask = st
            .lines()
            .find_map(|l| l.strip_prefix("ShdPnd:"))
            .map(|m| u64::from_str_radix(m.trim(), 16).unwrap())
            .unwrap();
        mask & (1 << (libc::SIGTERM - 1)) != 0
    }

    /// The old 50 ms watcher's three outcomes: a stop and a deadline each end the child with
    /// SIGTERM (now at once instead of on the next tick); a dropped watch never signals.
    #[test]
    fn watch_kill_stop_and_drop() {
        let stop: Arc<crate::svc::Stop> = Default::default();
        let mut ch = sleeper();
        let w = watch_kill(ch.id() as i32, stop.clone());
        stop.store(true, Ordering::Relaxed);
        // `sleep infinity` ends by SIGTERM only if the watch sent it.
        assert_eq!(ch.wait().unwrap().signal(), Some(libc::SIGTERM));
        drop(w);

        let stop: Arc<crate::svc::Stop> = Default::default();
        let mut ch = term_blocked();
        assert!(!term_pending(&ch));
        let w = watch_kill(ch.id() as i32, stop.clone());
        drop(w);
        // The stop runs its wakeups before `store` returns.
        stop.store(true, Ordering::Relaxed);
        assert!(!term_pending(&ch), "dropped watch signalled on the stop");
        ch.kill().unwrap();
        let _ = ch.wait();
    }
}
