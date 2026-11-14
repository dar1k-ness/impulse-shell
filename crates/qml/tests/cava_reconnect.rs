//! A running `CavaVisualizer` follows the PipeWire daemon across a restart: its points clear
//! when the daemon goes away and the bars move again once the restarted daemon plays audio
//! (the capture used to stay attached to the dead connection, freezing the bars). Runs a
//! private pipewire + wireplumber with a null sink and pw-play; fails when they are not
//! installed (`testenv`). Every wait is for an event: the runtime's waker ping.
#![cfg(feature = "pipewire")]

use qml::runtime::Runtime;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use testenv::Wakeups;

/// Children stopped with SIGTERM, as a service manager stops them, and reaped.
struct Kids(Vec<Child>);

impl Kids {
    fn stop(&mut self) {
        for mut c in self.0.drain(..).rev() {
            // SAFETY: kill(2) on our own unreaped child's pid.
            unsafe { libc::kill(c.id() as libc::pid_t, libc::SIGTERM) };
            let _ = c.wait();
        }
    }
}

impl Drop for Kids {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts `argv` with `listen` as its socket-activation fds 3.. (sd_listen_fds(3): LISTEN_FDS,
/// and LISTEN_PID set by the shell that `exec`s it so that it is the daemon's pid).
fn spawn(kids: &mut Kids, argv: &[&str], listen: &[&UnixListener]) -> Option<()> {
    let mut c = Command::new("sh");
    c.args(["-c", r#"LISTEN_PID=$$ exec "$0" "$@""#])
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if listen.is_empty() {
        c.env_remove("LISTEN_FDS");
    } else {
        c.env("LISTEN_FDS", listen.len().to_string());
    }
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
    if !which(argv[0]) {
        testenv::unavailable(format!("no {}", argv[0]));
        return None;
    }
    kids.0
        .push(c.spawn().unwrap_or_else(|e| panic!("{argv:?}: {e}")));
    Some(())
}

fn which(cmd: &str) -> bool {
    cmd.contains('/')
        || std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// In `test`'s own process (see `testenv::run_in_own_process`): the private runtime and
/// config dir the process running the test made, in which the daemon creates a null sink (the
/// default sink the visualizer's monitor capture links to) on every start. libpipewire and
/// wireplumber read the environment with getenv(3), so the dirs and the private session bus
/// are that process's environment, with no outer `PIPEWIRE_REMOTE`/`PIPEWIRE_RUNTIME_DIR`.
/// `None` in the process that ran the test there.
fn sandbox(test: &str) -> Option<PathBuf> {
    if testenv::in_own_process(test) {
        return Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap()));
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let conf = dir.join("config/pipewire/pipewire.conf.d");
    std::fs::create_dir_all(&conf).unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::write(
        conf.join("regnul.conf"),
        "context.objects = [ { factory = adapter args = { factory.name = support.null-audio-sink \
         node.name = regnul media.class = Audio/Sink audio.position = [ FL FR ] } } ]\n",
    )
    .unwrap();
    std::fs::write(dir.join("tone.wav"), tone()).unwrap();
    let mut bus = std::ffi::OsString::from("unix:path=");
    bus.push(dir.join("bus"));
    testenv::run_in_own_process_env(
        test,
        [
            ("XDG_RUNTIME_DIR", dir.into()),
            ("XDG_CONFIG_HOME", dir.join("config").into()),
            ("XDG_STATE_HOME", dir.join("state").into()),
            ("DBUS_SESSION_BUS_ADDRESS", bus),
        ],
        &["PIPEWIRE_REMOTE", "PIPEWIRE_RUNTIME_DIR"],
    );
    None
}

/// Two minutes of a 440 Hz stereo tone, S16LE 48 kHz WAV: plays past the end of the test.
fn tone() -> Vec<u8> {
    const RATE: u32 = 48000;
    let frames = RATE * 120;
    let data = frames * 4;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let t = i as f64 / RATE as f64;
        let s = ((t * 440.0 * std::f64::consts::TAU).sin() * 16000.0) as i16;
        w.extend_from_slice(&s.to_le_bytes());
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

struct Host {
    rt: Runtime,
    wake: Wakeups,
    viz: usize,
}

impl Host {
    /// A runtime with one running visualizer, woken through its waker ping.
    fn new() -> Host {
        let rt = Runtime::new(None);
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let viz = {
            let mut w = rt.world.borrow_mut();
            let viz = qml::cava::new_item(&mut w, None);
            w.write(viz, "running", qml::value::Value::Bool(true));
            viz
        };
        Host { rt, wake, viz }
    }

    fn points(&mut self) -> Vec<f64> {
        let v = self.rt.world.borrow_mut().read(self.viz, "points");
        let s = v.map(|v| v.to_js_string()).unwrap_or_default();
        s.split(',').filter_map(|p| p.parse().ok()).collect()
    }

    /// Runs the runtime on its wakeups until `done` holds for the points.
    fn until(&mut self, what: &str, done: impl Fn(&[f64]) -> bool) {
        eprintln!("waiting for {what}");
        loop {
            self.rt.poll_native();
            self.rt.settle();
            if done(&self.points()) {
                return;
            }
            self.wake.wait(None, None);
        }
    }

    /// One PipeWire lifetime: the daemon, then (once the capture is connected to it, so the
    /// daemon is serving its sockets) wireplumber and a player on the null sink, until the
    /// bars move.
    fn lifetime(&mut self, kids: &mut Kids, dir: &Path, listen: &[&UnixListener]) -> Option<()> {
        spawn(kids, &["pipewire"], listen)?;
        // The frame loop starts once the daemon has the stream: its first (silent) frame.
        self.until("the capture connected", |p| !p.is_empty());
        spawn(kids, &["wireplumber"], &[])?;
        let wav = dir.join("tone.wav");
        spawn(kids, &["pw-play", "--target", "regnul", wav.to_str()?], &[])?;
        self.until("moving bars", |p| p.iter().any(|&v| v > 0.0));
        Some(())
    }

    /// The bars move, clear when the daemon stops and move again once it is back.
    fn follows_a_restart(&mut self, dir: &Path, listen: &[&UnixListener]) {
        let mut kids = Kids(vec![]);
        if self.lifetime(&mut kids, dir, listen).is_none() {
            return;
        }
        kids.stop();
        self.until("points cleared with the daemon gone", <[f64]>::is_empty);
        self.lifetime(&mut kids, dir, listen)
            .expect("the daemons ran before the restart");
    }
}

/// The session bus wireplumber expects, at `DBUS_SESSION_BUS_ADDRESS`.
fn session_bus(kids: &mut Kids) -> Option<()> {
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let addr = format!("--address={bus}");
    spawn(
        kids,
        &["dbus-daemon", "--session", "--nofork", "--nopidfile", &addr],
        &[],
    )
}

/// A socket-activated daemon (systemd's pipewire.socket): the sockets outlive the daemon, so
/// the capture reconnects at once; its connect waits in the socket's backlog until the next
/// daemon (started by systemd on that connect; here, by the test) accepts it.
#[test]
fn follows_a_socket_activated_daemon_restart() {
    let Some(dir) = sandbox(concat!(
        module_path!(),
        "::follows_a_socket_activated_daemon_restart"
    )) else {
        return;
    };
    let listen = [
        UnixListener::bind(dir.join("pipewire-0")).unwrap(),
        UnixListener::bind(dir.join("pipewire-0-manager")).unwrap(),
    ];
    let mut bus = Kids(vec![]);
    if session_bus(&mut bus).is_none() {
        return;
    }
    Host::new().follows_a_restart(&dir, &listen.iter().collect::<Vec<_>>());
}

/// A daemon that binds its own socket: the capture waits for the restarted daemon's socket
/// to appear (inotify), with no retry timer. The visualizer runs before the first daemon too.
#[test]
fn follows_a_self_bound_daemon_restart() {
    let Some(dir) = sandbox(concat!(
        module_path!(),
        "::follows_a_self_bound_daemon_restart"
    )) else {
        return;
    };
    let mut bus = Kids(vec![]);
    if session_bus(&mut bus).is_none() {
        return;
    }
    Host::new().follows_a_restart(&dir, &[]);
}
