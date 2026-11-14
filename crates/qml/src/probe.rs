//! Process probes: what `pidof`, `killall` and the `bash -c` lines built from them
//! (Hyprsunset, Weather) did, as Rust. A probe is called by a native type, never by parsing a
//! command line, and gives the stdout and exit code the command printed. `Impulse.Processes`
//! (`crate::processes`) uses [`killall`].

/// What the original command wrote and its exit status.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Out {
    pub out: String,
    pub err: String,
    pub code: i32,
}

/// `GeoClueCheck` (crate::procs), what `busctl --system --no-pager list | grep -q
/// org.freedesktop.GeoClue2` checked: 0 when GeoClue2 owns a name on the system
/// bus or the bus can activate it, the names `busctl list` prints; 1 otherwise or with no bus.
/// grep matched the name anywhere in busctl's table; here only the bus name itself counts.
pub fn geoclue() -> Out {
    let listed = crate::dbus::system().is_some_and(|c| {
        geoclue_listed(|method, limit| {
            crate::dbus::call_timeout(
                &c,
                limit,
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                method,
                &(),
            )
            .ok()
            .and_then(|r| r.body().deserialize::<Vec<String>>().ok())
        })
    });
    Out {
        code: if listed { 0 } else { 1 },
        ..Out::default()
    }
}

/// Whether GeoClue2 is among the names the bus owns or can activate: `names` asks the bus
/// daemon for one of its two lists and gives it the time written here to answer.
fn geoclue_listed(mut names: impl FnMut(&str, std::time::Duration) -> Option<Vec<String>>) -> bool {
    const GEOCLUE: &str = "org.freedesktop.GeoClue2";
    ["ListNames", "ListActivatableNames"].into_iter().any(|m| {
        names(m, std::time::Duration::from_secs(5))
            .is_some_and(|names| names.iter().any(|n| n == GEOCLUE))
    })
}

/// Hyprsunset's `pidof hyprsunset || hyprsunset` with its requests waiting for it.
///
/// The original fires `startHyprsunset()` and its `hyprctl hyprsunset ...` requests together, so on a
/// fresh start several `pidof || hyprsunset` race: two hyprsunsets start, the second one
/// replaces `.hyprsunset.sock` with its own and exits when Hyprland refuses it a second CTM
/// manager, leaving a socket nobody listens on; and a request sent before hyprsunset listens
/// hits the socket file a killed hyprsunset left, or the new one between its bind(2) and
/// listen(2) (connect(2) is refused there). Here starts run one at a time (the next `pidof`
/// sees the one started), and a request waits for the starts asked for before it to settle:
/// hyprsunset found running, or the one started printed the line its IPC thread logs right
/// after listen(2) (`hyprsunset socket started at ...`, IPCSocket.cpp; `--verbose`, 0.3.0+), or
/// it exited.
pub mod sunset {
    use std::path::Path;
    use std::sync::{Condvar, Mutex, MutexGuard};

    struct Gate {
        asked: u64,
        settled: u64,
    }

    static GATE: Mutex<Gate> = Mutex::new(Gate {
        asked: 0,
        settled: 0,
    });
    static SETTLED: Condvar = Condvar::new();
    static START: Mutex<()> = Mutex::new(());

    fn gate() -> MutexGuard<'static, Gate> {
        GATE.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A start asked for now: requests made from now on wait for it.
    pub fn ask() -> u64 {
        let mut g = gate();
        g.asked += 1;
        g.asked
    }

    /// The starts a request made now waits for.
    pub fn ticket() -> u64 {
        gate().asked
    }

    /// Until every start up to `ticket` settled.
    pub fn wait(ticket: u64) {
        let mut g = gate();
        while g.settled < ticket {
            g = SETTLED.wait(g).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn settle(ticket: u64) {
        let mut g = gate();
        g.settled = g.settled.max(ticket);
        SETTLED.notify_all();
    }

    /// Start `prog` unless one runs (`pidof`'s output), as start `ticket`.
    pub fn ensure(prog: &Path, ticket: u64) -> super::Out {
        let name = prog
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let p = {
            let _one = START.lock().unwrap_or_else(|e| e.into_inner());
            let p = super::pidof(&[name]);
            if p.code != 0 {
                start(prog);
            }
            p
        };
        settle(ticket);
        p
    }

    /// What hyprsunset's IPC thread logs once it listens (`Debug::log(LOG, ...)`, printed with
    /// `--verbose` and flushed by `std::endl`).
    const LISTENING: &[u8] = b"[LOG] hyprsunset socket started at ";

    /// Until the started hyprsunset listens on its socket or exits.
    pub(crate) fn start(prog: &Path) {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;
        let mut c = workpool::userenv::command(prog);
        c.arg("--verbose")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .process_group(0);
        // The pipe is read only until that line, and hyprsunset outlives this process: with
        // SIGPIPE ignored (systemd's IgnoreSIGPIPE= default for services) its later log lines
        // fail with EPIPE, which iostream drops, instead of killing it.
        fn ignore_sigpipe() -> std::io::Result<()> {
            // SAFETY: signal(2) takes plain integers and touches no memory of this process.
            unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
            Ok(())
        }
        // SAFETY: the hook runs in the forked child before execve(2) and only calls
        // signal(2), which is async-signal-safe (signal-safety(7)).
        unsafe { c.pre_exec(ignore_sigpipe) };
        let Ok(mut child) = c.spawn() else {
            return;
        };
        if let Some(out) = child.stdout.take() {
            let mut out = std::io::BufReader::new(out);
            let mut line = Vec::new();
            // No timeout: a hyprsunset still connecting to the compositor has not settled, and
            // its requests queue until it listens or exits (EOF).
            while out.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
                if line.starts_with(LISTENING) {
                    break;
                }
                line.clear();
            }
        }
        workpool::reap(child);
    }
}

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// A process as procps/psmisc read it (/proc order: ascending pid).
struct Task {
    pid: i32,
    comm: String,
    cmdline: Vec<String>,
}

fn tasks() -> Vec<Task> {
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    crate::osutil::pids()
        .into_iter()
        .filter_map(|pid| {
            Some(Task {
                pid,
                comm: s(&crate::osutil::comm(pid)?),
                cmdline: crate::osutil::cmdline(pid).iter().map(|a| s(a)).collect(),
            })
        })
        .collect()
}

/// procps-ng `pidof NAME...` (no options): `select_procs`, pids of each name newest first.
pub fn pidof(progs: &[String]) -> Out {
    let mut all: Vec<i32> = Vec::new();
    for prog in progs.iter().filter(|p| !p.is_empty()) {
        all.extend(crate::osutil::pidof(prog));
    }
    if all.is_empty() {
        return Out {
            code: 1,
            ..Out::default()
        };
    }
    let s: Vec<String> = all.iter().map(i32::to_string).collect();
    Out {
        out: s.join(" ") + "\n",
        ..Out::default()
    }
}

/// psmisc `killall NAME...` (SIGTERM, no options).
pub fn killall(names: &[String]) -> Out {
    let me = std::process::id() as i32;
    let mut found = vec![false; names.len()];
    let mut err = String::new();
    for t in tasks().iter().filter(|t| t.pid != me) {
        // `match_process_name`: a 15-char comm may be truncated, then argv[0]'s basename
        // (when it extends comm) is compared instead.
        let long = (t.comm.len() == 15)
            .then(|| t.cmdline.first().map(|c| basename(c).to_string()))
            .flatten()
            .filter(|b| b.starts_with(&t.comm));
        let Some(j) = names.iter().position(|n| match &long {
            _ if t.comm.len() == 15 && n.len() >= 15 => match &long {
                Some(l) => n == l,
                None => n.as_bytes()[..15] == *t.comm.as_bytes(),
            },
            Some(l) => n == l,
            None => *n == t.comm,
        }) else {
            continue;
        };
        // SAFETY: kill(2) takes plain integers and touches no memory of ours.
        if unsafe { libc::kill(t.pid, libc::SIGTERM) } == 0 {
            found[j] = true;
        } else {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                let what = long.as_deref().unwrap_or(&t.comm);
                err += &format!("{what}({}): {}\n", t.pid, os_msg(&e));
            }
        }
    }
    for (n, f) in names.iter().zip(&found) {
        if !f {
            err += &format!("{n}: no process found\n");
        }
    }
    Out {
        out: String::new(),
        err,
        code: found.iter().any(|f| !f) as i32,
    }
}

/// strerror text without Rust's " (os error N)" suffix.
fn os_msg(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// The GeoClue check gives the bus daemon 5 s for each of its two lists, and a list that
    /// never came counts as one without GeoClue2 in it.
    #[test]
    fn geoclue_check_gives_each_list_five_seconds() {
        let mut asked = Vec::new();
        let listed = super::geoclue_listed(|method, limit| {
            asked.push((method.to_owned(), limit));
            None
        });
        assert!(!listed);
        let five = Duration::from_secs(5);
        assert_eq!(
            asked,
            [
                ("ListNames".to_owned(), five),
                ("ListActivatableNames".to_owned(), five)
            ]
        );
    }

    /// The name itself counts, in either list; the second list is not asked for once the
    /// first has it.
    #[test]
    fn geoclue_check_reads_both_lists_for_the_name() {
        let own = |names: &[&str]| Some(names.iter().map(|n| (*n).to_owned()).collect());
        assert!(super::geoclue_listed(|method, _| match method {
            "ListNames" => own(&["org.freedesktop.GeoClue2"]),
            other => unreachable!("{other} asked after the name was found"),
        }));
        assert!(super::geoclue_listed(|method, _| match method {
            "ListNames" => own(&["org.freedesktop.DBus"]),
            _ => own(&["org.freedesktop.GeoClue2"]),
        }));
        assert!(!super::geoclue_listed(|_, _| own(&[
            "org.freedesktop.GeoClue2.Agent"
        ])));
    }
}
