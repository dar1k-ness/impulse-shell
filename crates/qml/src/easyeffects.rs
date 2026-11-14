//! The `EasyEffects` singleton of module `Impulse` (`Impulse/easyeffects.qmltypes`): whether
//! EasyEffects is installed (`available`) and running (`active`), and `enable`, `disable`,
//! `toggle` and `open`. No shell runs: only `easyeffects` and `flatpak` themselves, found on
//! PATH, with this process's environment; a fallback command starts once the first has failed.
//! `disable` stops only what `pidof` counts as easyeffects, stopping a running service does
//! not launch the fallback, an `enable` or `disable` that does nothing re-probes `active`
//! instead of leaving the toggle wrong, and only an executable `easyeffects` counts as
//! installed.
//! `active` and `disable` start no `flatpak` to look for the sandboxed app: a Flatpak'd
//! EasyEffects is a process of this session like any other, its argv[0] the `easyeffects` of
//! the app's metadata, so `pidof` already finds it, and its `/.flatpak-info`
//! (flatpak-metadata(5), read through `/proc/PID/root`) names the app. `disable` quits each
//! running install through EasyEffects' own `-q` ("Quit Easy Effects. Useful when running in
//! service mode."): EasyEffects 8.0–8.3.0 install no SIGTERM handler, so a `pkill` kills them
//! before `aboutToQuit` saves the database, and 7.x (GApplication) handles only SIGINT. No
//! version can be watched on D-Bus instead: 8.x owns no bus name (single instance through a
//! lock file and a local socket, which `-q` talks to), and it is started with
//! `--service-mode`, an option 7.x does not have.
use crate::eval::{Interp, R};
use crate::impulse::Replies;
use crate::value::Value;
use std::path::Path;

pub const APP: &str = "com.github.wwmm.easyeffects";
const QUIT: &[&str] = &["easyeffects", "-q"];
const QUIT_FLATPAK: &[&str] = &["flatpak", "run", APP, "-q"];

pub use crate::osutil::command_v;

/// The Flatpak app `pid` runs as: `[Application] name` of the `/.flatpak-info` Flatpak puts
/// in every sandbox; None for a process outside one.
pub fn flatpak_app(pid: i32) -> Option<String> {
    let p = format!("/proc/{pid}/root/.flatpak-info");
    let text = crate::osutil::read_regular(Path::new(&p), 1 << 20).ok()?;
    crate::ini::value(&String::from_utf8_lossy(&text), "Application", "name")
}

/// A running EasyEffects and the install it was started from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    Host,
    Flatpak,
}

/// Every running EasyEffects, newest first like `pidof easyeffects`: host processes and the
/// app's own sandbox, not a process of that name inside some other Flatpak.
pub fn instances() -> Vec<(i32, Install)> {
    crate::osutil::pidof("easyeffects")
        .into_iter()
        .filter_map(|pid| match flatpak_app(pid) {
            None => Some((pid, Install::Host)),
            Some(app) if app == APP => Some((pid, Install::Flatpak)),
            Some(_) => None,
        })
        .collect()
}

/// A program run from PATH with the shell's own environment, stdin/stdout/stderr on
/// /dev/null and its own process group, like `Rustshell.execDetached`.
pub(crate) fn program(argv: &[&str]) -> Option<std::process::Command> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let mut c = workpool::userenv::command(command_v(argv[0])?);
    c.arg0(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // No fd of the shell, CLOEXEC or not, may stay open in the long-lived service.
    let close_fds = || -> std::io::Result<()> {
        // SAFETY: close_range(2) takes plain integers and only marks this process's fds.
        unsafe { libc::close_range(3, u32::MAX, libc::CLOSE_RANGE_CLOEXEC as i32) };
        Ok(())
    };
    // SAFETY: the closure runs in the forked child before exec and calls only close_range(2),
    // which is async-signal-safe (no allocation, no lock), as `pre_exec` requires.
    unsafe { c.pre_exec(close_fds) };
    Some(c)
}

/// A finished command failed by itself: exited non-zero. Killed by a signal (a logout) is no
/// failure to fall back from or report.
fn failed(st: std::process::ExitStatus) -> bool {
    st.code().is_some_and(|c| c != 0)
}

/// Writes `name` of the `EasyEffects` singleton.
fn set(w: &mut crate::runtime::World, name: &str, on: bool) {
    if let Some(&root) = w.singletons.get(NAME) {
        w.write(root, name, Value::Bool(on));
    }
}

/// Probes `active` on a pool thread and writes it on the main thread: a
/// `fetchActiveState`, or a start or quit that failed.
fn resync(back: Replies) {
    workpool::run(move || {
        let on = active();
        back.send(move |w| set(w, "active", on));
    });
}

/// Run `last` detached and reap it; with `report`, a `last` that cannot start or fails
/// re-probes the active state.
fn spawn_last(last: Vec<&'static str>, report: Option<Replies>) {
    match (program(&last).map(|mut c| c.spawn()), report) {
        (Some(Ok(ch)), None) => workpool::reap(ch),
        // An unknown exit status (wait(2) failed) is probed as well.
        (Some(Ok(ch)), Some(h)) => workpool::reap_then(ch, move |st| {
            if st.is_none_or(failed) {
                resync(h);
            }
        }),
        (_, Some(h)) => resync(h),
        (_, None) => {}
    }
}

/// `A || B` (or just `B`) detached: starts A and, when it cannot run or exits non-zero, B
/// once A has exited. Nothing of this process is forked: the reaper
/// waits for A without a thread of its own. Unlike bash, A killed by a signal (the running
/// service stopped by `disable`) does not start B.
fn detached(
    first: Option<&'static [&'static str]>,
    last: &'static [&'static str],
    report: Option<Replies>,
) {
    let last = last.to_vec();
    workpool::run(move || {
        let Some(Ok(ch)) = first.and_then(program).map(|mut c| c.spawn()) else {
            spawn_last(last, report);
            return;
        };
        workpool::reap_then(ch, move |st| match (st, report) {
            (Some(st), report) if failed(st) => {
                workpool::run(move || spawn_last(last, report));
            }
            // How A ended is unknown (wait(2) failed): B is not started for it, the active
            // state is probed.
            (None, Some(h)) => resync(h),
            _ => {}
        });
    });
}

/// The probe's program `argv` from PATH, stdin on /dev/null.
pub(crate) fn command(argv: &[&str]) -> Option<std::process::Command> {
    use std::os::unix::process::CommandExt;
    let mut c = workpool::userenv::command(command_v(argv[0])?);
    c.arg0(argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null());
    Some(c)
}

/// Whether EasyEffects is installed: an executable `easyeffects` in PATH, or the app
/// `flatpak info` knows.
pub fn available() -> bool {
    if command_v("easyeffects").is_some_and(|p| crate::osutil::access_x(&p)) {
        return true;
    }
    command(&["flatpak", "info", APP]).is_some_and(|mut c| {
        c.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Whether EasyEffects runs ([`instances`]): the sandboxed app is one of the processes found,
/// so no `flatpak` runs.
pub fn active() -> bool {
    !instances().is_empty()
}

/// Starts the service: `easyeffects --hide-window --service-mode`, or when that cannot start
/// `flatpak run … --hide-window --service-mode`. When neither starts it, `report` gets the
/// active state re-probed instead of `active` staying true for nothing.
pub fn enable(report: Option<Replies>) {
    detached(
        Some(&["easyeffects", "--hide-window", "--service-mode"]),
        &["flatpak", "run", APP, "--hide-window", "--service-mode"],
        report,
    );
}

/// Quits EasyEffects its own way: `easyeffects -q` for a running host install, `flatpak run
/// … -q` for the running app's sandbox; nothing when none runs. When a quit cannot start or
/// fails, `report` gets the active state re-probed.
pub fn disable(report: Option<Replies>) {
    workpool::run(move || {
        let running = instances();
        for (install, argv) in [(Install::Host, QUIT), (Install::Flatpak, QUIT_FLATPAK)] {
            if running.iter().any(|&(_, i)| i == install) {
                detached(None, argv, report.clone());
            }
        }
    });
}

/// Opens the EasyEffects window: `flatpak run com.github.wwmm.easyeffects || easyeffects`.
pub fn open() {
    detached(Some(&["flatpak", "run", APP]), &["easyeffects"], None);
}

/// The QML name of the singleton (`Impulse/easyeffects.qmltypes`).
pub const NAME: &str = "EasyEffects";

/// False under a spawn hook (aotcheck, tests that record commands), which gets `argv`
/// instead: nothing of the real system is started there, and nothing is probed.
fn real(argv: &[&str]) -> bool {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    crate::native::hooked_spawn(&argv).is_none()
}

fn fetch_availability(w: &mut crate::runtime::World) {
    if crate::native::sandboxed() {
        return;
    }
    let back = crate::impulse::replies(w);
    workpool::run(move || {
        let on = available();
        back.send(move |w| set(w, "available", on));
    });
}

fn fetch_active_state(w: &mut crate::runtime::World) {
    if !crate::native::sandboxed() {
        resync(crate::impulse::replies(w));
    }
}

fn set_active(w: &mut crate::runtime::World, on: bool) {
    set(w, "active", on);
    let report = Some(crate::impulse::replies(w));
    if on {
        if real(&["easyeffects", "--hide-window", "--service-mode"]) {
            enable(report);
        }
    } else if real(QUIT) {
        disable(report);
    }
}

fn with_world(it: &mut Interp, f: impl FnOnce(&mut crate::runtime::World)) -> R<Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        f(&mut w.borrow_mut());
    }
    Ok(Value::Undefined)
}

const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("fetchAvailability", |it, _, _| {
        with_world(it, fetch_availability)
    }),
    ("fetchActiveState", |it, _, _| {
        with_world(it, fetch_active_state)
    }),
    ("enable", |it, _, _| with_world(it, |w| set_active(w, true))),
    ("disable", |it, _, _| {
        with_world(it, |w| set_active(w, false))
    }),
    ("toggle", |it, _, _| {
        with_world(it, |w| {
            let on = w
                .singletons
                .get(NAME)
                .and_then(|&root| w.items[root].props.get("active"))
                .is_some_and(|p| p.value.truthy());
            set_active(w, !on);
        })
    }),
    ("open", |it, _, _| {
        with_world(it, |_| {
            if real(&["flatpak", "run", APP]) {
                open();
            }
        })
    }),
];

/// The new singleton looks for both states once, as soon as something names it.
fn init(w: &mut crate::runtime::World, _id: usize) {
    fetch_availability(w);
    fetch_active_state(w);
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: METHODS,
    init: Some(init),
};
