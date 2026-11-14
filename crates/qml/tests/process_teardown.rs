//! What in-process `Process` jobs and `qml::child::run` start ends with the shell, as
//! Quickshell's `~QProcess` kills its child: on `Rustshell.reload()` (handed over at the
//! exec, killed by the next image), when the Runtime is dropped (the shell exiting), and through the child guard
//! when the shell is SIGKILLed. A stopped job also returns while PipeWire never answers.
//!
//! Each wait is an event: a FIFO the fake program writes its pid to, its pidfd turning
//! readable when it exits. The fake writes its pid only once the job read its stdout, which
//! the job does after registering the child, so nothing races the registration.
use qml::runtime::Runtime;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

fn mkfifo(p: &Path) {
    assert!(
        std::process::Command::new("mkfifo")
            .arg(p)
            .status()
            .unwrap()
            .success()
    );
}

/// Installs an executable out of process (install(1)): a write from here could still be open
/// in another thread's fork when it is executed (ETXTBSY).
fn install(dst: &Path, body: &str) {
    let tmp = dst.with_extension("src");
    std::fs::write(&tmp, body).unwrap();
    assert!(
        std::process::Command::new("install")
            .arg("-m755")
            .arg(&tmp)
            .arg(dst)
            .status()
            .unwrap()
            .success()
    );
}

/// A program that fills more than a pipe's buffer of stdout (so it goes on only once its
/// reader drains it), then reports its pid through `fifo` and stays until killed.
fn fake(fifo: &Path) -> String {
    format!(
        "#!/bin/sh\nhead -c 1048576 /dev/zero\necho $$ > '{}'\nexec sleep infinity\n",
        fifo.display()
    )
}

/// The pid written to `fifo` (blocks until the writer opens it).
fn read_pid(fifo: &Path) -> i32 {
    std::fs::read_to_string(fifo)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Blocks until the process behind `fd` exited (its pidfd turns readable).
fn wait_exit(fd: &OwnedFd) {
    let mut p = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `p` is one live pollfd over an open pidfd.
    while unsafe { libc::poll(&mut p, 1, -1) } < 0 {
        assert_eq!(
            std::io::Error::last_os_error().kind(),
            std::io::ErrorKind::Interrupted
        );
    }
}

/// A home with an empty config and `kdialog` (the fake) first on PATH; the vars for the
/// test's own process and the FIFO.
fn picker_env(home: &Path) -> (Vec<(&'static str, std::ffi::OsString)>, PathBuf) {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fifo = home.join("opened");
    mkfifo(&fifo);
    install(&bin.join("kdialog"), &fake(&fifo));
    let cfg = home.join(".config/rusty-impulse");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.json"), "{}").unwrap();
    let mut vars = testenv::home_vars(home);
    vars.push((
        "PATH",
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()).into(),
    ));
    vars.push(("TEARDOWN_FIFO", fifo.clone().into()));
    (vars, fifo)
}

/// The original's singletons, and services/Switchwall.qml's `pick` (the wallpaper button's dialog,
/// run in process by the `picker` job) asked once.
fn open_picker(rt: &mut Runtime) {
    let r = ri();
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    let o = rt
        .load_str(
            "import QtQuick\nimport ri.services\n\
             QtObject { function go() { Switchwall.pick([]); return Switchwall.picking } }",
        )
        .unwrap();
    rt.poll_native();
    let picking = rt.call_method(o, "go", vec![]).unwrap().unwrap();
    assert!(picking.truthy(), "the picker did not start");
}

/// What a reload does to the running groups: handed over at the exec, killed by the next
/// image, which inherits this process's children.
fn reload() {
    let groups = qml::native::hand_over_children();
    qml::native::end_handed_over(&groups, &qml::native::inherited_children());
}

/// The open dialog's pidfd.
fn dialog() -> OwnedFd {
    let pid = read_pid(Path::new(&std::env::var_os("TEARDOWN_FIFO").unwrap()));
    workpool::pidfd_open(pid).unwrap()
}

#[test]
fn reload_kills_the_dialog_of_an_in_process_job() {
    let test = concat!(
        module_path!(),
        "::reload_kills_the_dialog_of_an_in_process_job"
    );
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let (vars, _) = picker_env(home.path());
        testenv::run_in_own_process(test, vars);
        return;
    }
    let mut rt = Runtime::new(Some(ri()));
    open_picker(&mut rt);
    let dialog = dialog();
    reload();
    wait_exit(&dialog);
}

#[test]
fn dropping_the_runtime_ends_the_dialog_of_an_in_process_job() {
    let test = concat!(
        module_path!(),
        "::dropping_the_runtime_ends_the_dialog_of_an_in_process_job"
    );
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let (vars, _) = picker_env(home.path());
        testenv::run_in_own_process(test, vars);
        return;
    }
    let mut rt = Runtime::new(Some(ri()));
    open_picker(&mut rt);
    let dialog = dialog();
    drop(rt);
    wait_exit(&dialog);
}

/// The child guard's process: this binary re-run with `TEARDOWN_GUARD` set.
#[test]
fn guard_main() {
    if std::env::var_os("TEARDOWN_GUARD").is_some() {
        std::process::exit(qml::childguard::serve());
    }
}

fn guard_command() -> std::process::Command {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.args(["--exact", "guard_main", "--nocapture"])
        .env("TEARDOWN_GUARD", "1");
    c
}

#[test]
fn sigkilled_shell_takes_the_dialog_of_an_in_process_job_along() {
    const NAME: &str = "sigkilled_shell_takes_the_dialog_of_an_in_process_job_along";
    if std::env::var_os("TEARDOWN_SHELL").is_some() {
        // The shell: guarded children, the dialog open, then waiting for its SIGKILL.
        qml::childguard::enable(guard_command);
        let mut rt = Runtime::new(Some(ri()));
        open_picker(&mut rt);
        loop {
            std::thread::park();
        }
    }
    let home = tempfile::tempdir().unwrap();
    let (vars, fifo) = picker_env(home.path());
    let mut shell = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME, "--nocapture"])
        .envs(vars)
        .env("TEARDOWN_SHELL", "1")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let dialog = workpool::pidfd_open(read_pid(&fifo)).unwrap();
    shell.kill().unwrap();
    shell.wait().unwrap();
    wait_exit(&dialog);
}

#[test]
fn reload_kills_a_child_run_group() {
    let test = concat!(module_path!(), "::reload_kills_a_child_run_group");
    if !testenv::in_own_process(test) {
        testenv::run_in_own_process(test, [("TEARDOWN", "1")]);
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let fifo = d.path().join("up");
    mkfifo(&fifo);
    let script = fake(&fifo);
    let stop: std::sync::Arc<qml::svc::Stop> = Default::default();
    let st = stop.clone();
    let run = std::thread::spawn(move || {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &script]);
        qml::child::run(&mut c, &st).unwrap()
    });
    let child = workpool::pidfd_open(read_pid(&fifo)).unwrap();
    reload();
    wait_exit(&child);
    let out = run.join().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        out.status.and_then(|s| s.signal()),
        Some(libc::SIGKILL),
        "the run must end by the reload's SIGKILL, not its own stop"
    );
}

/// songrec.sh's job waits for PipeWire's initial state; a socket that takes the connection but
/// never answers never publishes it. Stopping the SongRec Process must still end the job.
#[cfg(feature = "pipewire")]
#[test]
fn stopped_songrec_job_returns_while_pipewire_never_answers() {
    let test = concat!(
        module_path!(),
        "::stopped_songrec_job_returns_while_pipewire_never_answers"
    );
    if !testenv::in_own_process(test) {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        install(&bin.join("songrec"), "#!/bin/sh\nexit 0\n");
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        testenv::run_in_own_process(
            test,
            [
                ("PIPEWIRE_RUNTIME_DIR", d.path().as_os_str().to_owned()),
                ("XDG_RUNTIME_DIR", d.path().as_os_str().to_owned()),
                ("PIPEWIRE_REMOTE", "pipewire-0".into()),
                ("PATH", path.into()),
            ],
        );
        return;
    }
    let dir = std::env::var_os("PIPEWIRE_RUNTIME_DIR").unwrap();
    // Listening, never accepting: connect(2) succeeds on the backlog, nothing answers.
    let _daemon =
        std::os::unix::net::UnixListener::bind(Path::new(&dir).join("pipewire-0")).unwrap();
    let stop: std::sync::Arc<qml::svc::Stop> = Default::default();
    let io = qml::ports::Io {
        tx: None,
        stop: stop.clone(),
    };
    let job = std::thread::spawn(move || {
        let a = qml::ports::songrec::Args {
            interval: "2".into(),
            total: "30".into(),
            source: "monitor".into(),
        };
        qml::ports::songrec::run(&a, &io)
    });
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(job.join().unwrap(), 143);
}
