//! The client's first snapshot is the daemon's complete initial state (its nodes ready, the
//! `default` metadata in), and the nodes of a restarted daemon are new nodes even where it
//! hands out the old ids and `object.serial`s again. Runs a private pipewire, handed its
//! listening sockets like `pipewire.socket` does, so the client waits in the socket's backlog
//! across the restart, and no session manager: the daemon's configuration holds the sink and the
//! defaults, so each daemon has them before it accepts a client (pw_context_new creates
//! `context.objects` before the main loop runs). Fails when pipewire is not installed (`testenv`).
#![cfg(feature = "pipewire")]

use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

const NAME: &str = "ri_restart";
/// A second sink, made the default once the restarted daemon is up, as a session manager
/// started after it does.
const LATE: &str = "ri_restart_late";

/// Killed and reaped when dropped, whether the test passed or panicked.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `pipewire` with `listen` as its socket-activation fds 3.. (sd_listen_fds(3): LISTEN_FDS,
/// and LISTEN_PID set by the shell that `exec`s it so that it is the daemon's pid).
fn pipewire(listen: &[UnixListener]) -> Option<Daemon> {
    let mut c = Command::new("sh");
    c.args(["-c", r#"LISTEN_PID=$$ exec pipewire"#])
        .env("LISTEN_FDS", listen.len().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let fds: Vec<RawFd> = listen.iter().map(|l| l.as_raw_fd()).collect();
    let mut moved = fds.clone();
    let first = 3 + fds.len() as RawFd;
    // Runs in the forked child before exec: it only calls fcntl(2) and dup2(2), which are
    // async-signal-safe, and allocates nothing (`moved` was allocated before the fork).
    let hook = move || {
        // Moved above 3..first first, so that placing one cannot overwrite another.
        for (m, &fd) in moved.iter_mut().zip(&fds) {
            // SAFETY: F_DUPFD_CLOEXEC duplicates the open `fd`; the copy closes at exec.
            *m = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, first) };
            if *m < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        for (i, &m) in moved.iter().enumerate() {
            // SAFETY: dup2 onto 3 + i of the open `m`; the target is left without FD_CLOEXEC,
            // so the daemon inherits it.
            if unsafe { libc::dup2(m, 3 + i as RawFd) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: `hook` is async-signal-safe and allocation-free, as pre_exec requires.
    unsafe { c.pre_exec(hook) };
    c.spawn()
        .map(Daemon)
        .map_err(|e| testenv::unavailable(format!("no sh ({e})")))
        .ok()
}

/// A pipewire.conf(5) drop-in with null sinks named [`NAME`] and [`LATE`] and the `default`
/// metadata (the session manager's; none runs) naming [`NAME`] the default and configured
/// default sink.
fn conf() -> String {
    // Values in strict JSON, as a session manager writes them (`Spa:String:JSON`).
    let value = format!(r#"{{ "name": "{NAME}" }}"#);
    format!(
        r#"context.objects = [
  {{ factory = adapter args = {{ factory.name = support.null-audio-sink media.class = Audio/Sink node.name = {NAME} audio.position = [ FL FR ] }} }}
  {{ factory = adapter args = {{ factory.name = support.null-audio-sink media.class = Audio/Sink node.name = {LATE} audio.position = [ FL FR ] }} }}
  {{ factory = metadata args = {{ metadata.name = default metadata.values = [
    {{ key = default.audio.sink type = "Spa:String:JSON" value = {value} }}
    {{ key = default.configured.audio.sink type = "Spa:String:JSON" value = {value} }}
  ] }} }}
]
"#
    )
}

/// Sets `default.audio.sink` to `name` with `pw-metadata`, which returns once the daemon has
/// answered.
fn set_default(name: &str) -> Option<()> {
    let value = format!(r#"{{ "name": "{name}" }}"#);
    let argv = [
        "-n",
        "default",
        "0",
        "default.audio.sink",
        &value,
        "Spa:String:JSON",
    ];
    let st = Command::new("pw-metadata")
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map_err(|e| testenv::unavailable(format!("no pw-metadata ({e})")))
        .ok()?;
    assert!(st.success(), "pw-metadata {argv:?}: {st}");
    Some(())
}

fn sink(s: &qml::pw::Snap) -> Option<&qml::pw::NodeSnap> {
    s.nodes.iter().map(|n| &**n).find(|n| n.name == NAME)
}

#[test]
fn first_snapshot_is_complete_and_a_restarted_daemon_has_new_nodes() {
    const TEST: &str = concat!(
        module_path!(),
        "::first_snapshot_is_complete_and_a_restarted_daemon_has_new_nodes"
    );
    // libpipewire reads the environment with getenv(3), so the test runs in a process of its
    // own with a private runtime directory and no outer `PIPEWIRE_REMOTE`.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("state")).unwrap();
        testenv::run_in_own_process_env(
            TEST,
            [
                ("XDG_RUNTIME_DIR", d.as_os_str().to_owned()),
                ("PIPEWIRE_RUNTIME_DIR", d.as_os_str().to_owned()),
                ("XDG_STATE_HOME", d.join("state").into()),
                ("XDG_CONFIG_HOME", d.join("config").into()),
            ],
            &["PIPEWIRE_REMOTE"],
        );
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let conf_d = dir.join("config/pipewire/pipewire.conf.d");
    std::fs::create_dir_all(&conf_d).unwrap();
    std::fs::write(conf_d.join("ri-restart.conf"), conf()).unwrap();
    let listen = [
        UnixListener::bind(dir.join("pipewire-0")).unwrap(),
        UnixListener::bind(dir.join("pipewire-0-manager")).unwrap(),
    ];
    let Some(daemon) = pipewire(&listen) else {
        return;
    };

    // Published only once the initial roundtrip is done: the sink with its volumes, ready,
    // and the defaults, in the first snapshot the client publishes.
    let first = qml::pw::wait_snapshot();
    assert!(first.connected, "no daemon reached");
    let old = sink(&first)
        .expect("the sink is missing from the first snapshot")
        .clone();
    assert!(old.ready, "the first snapshot has the sink not ready");
    assert_eq!(
        old.volumes.len(),
        2,
        "the first snapshot lacks the sink's volumes"
    );
    assert!(
        old.serial.is_some_and(|s| s > 0),
        "no object.serial: {:?}",
        old.serial
    );
    assert_eq!(
        first.defaults[0], NAME,
        "the first snapshot lacks default.audio.sink"
    );
    assert_eq!(first.defaults[2], NAME);

    drop(daemon);
    let gone = qml::pw::wait_snapshot_until(|s| !s.connected);
    assert!(gone.nodes.is_empty() && gone.defaults.iter().all(String::is_empty));

    let Some(_daemon) = pipewire(&listen) else {
        return;
    };
    // The first snapshot of the restarted daemon is its complete initial state as well.
    let again = qml::pw::wait_snapshot_until(|s| s.connected);
    let new = sink(&again).expect("the sink is missing from the restarted daemon's first snapshot");
    assert!(new.ready, "the restarted daemon's sink is not ready");
    // The restarted daemon numbers its objects from the start again, so its sink's id and
    // serial can be the old ones (or near them, as here); either way it is another node.
    eprintln!(
        "before: id {} serial {:?}; after: id {} serial {:?}",
        old.id, old.serial, new.id, new.serial
    );
    assert_ne!(
        new.key, old.key,
        "the restarted daemon's sink kept the old node's identity"
    );
    assert_eq!(
        again.defaults[0], NAME,
        "the restarted daemon's first snapshot lacks default.audio.sink"
    );
    assert_eq!(again.defaults[2], NAME);

    // A default written to the restarted daemon later reaches the client as well: the metadata
    // it bound on reconnecting is the live one, not a copy of the initial state.
    if set_default(LATE).is_none() {
        return;
    }
    let late = qml::pw::wait_snapshot_until(|s| s.defaults[0] != NAME);
    assert_eq!(late.defaults[0], LATE);
    assert!(late.connected);
    let _ = std::fs::remove_dir_all(&dir);
}
