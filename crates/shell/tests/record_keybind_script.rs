//! The original's Hyprland keybinds record the screen by running a file of the config tree,
//! `scripts/videos/record.sh --fullscreen` (Ctrl+Alt+R; with `--sound`, Super+Shift+Alt+R).
//! The tree `tools/install.sh` installs has that file, and it hands the launch to rusty-impulse, in
//! PATH or where an install for one user puts it (`~/.local/bin`, which a compositor's PATH
//! may lack): wf-recorder starts for the whole screen in the directory the user's config
//! names, and a second launch stops it. What `rusty-impulse __ri-record` does for every launch of the script is
//! compared against the original's script in crates/qml/tests/script_ports_equiv.rs
//! (`record_program_matches_script`).

use std::path::Path;
use std::process::{Command, Stdio};

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

fn script() -> std::path::PathBuf {
    Path::new(ricompiled::ROOT).join("scripts/videos/record.sh")
}

#[test]
fn the_tree_has_the_file_the_keybinds_run() {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(script())
        .unwrap_or_else(|e| panic!("{}: {e}", script().display()))
        .permissions()
        .mode();
    assert_eq!(
        mode & 0o111,
        0o111,
        "{} is not executable",
        script().display()
    );
}

#[test]
fn the_keybind_launch_starts_and_stops_a_full_screen_recording() {
    if !testenv::require_all(&["sh", "bash"]) {
        return;
    }
    starts_and_stops("bin/rusty-impulse");
    // rusty-impulse is not in PATH and no shell runs, as with a key pressed under a compositor
    // started without ~/.local/bin in its PATH while the shell is down.
    starts_and_stops("home/.local/bin/rusty-impulse");
}

/// The two launches of Ctrl+Alt+R with rusty-impulse installed as `rusty_impulse` under a directory whose
/// `bin` is in PATH and whose `home` is the user's.
fn starts_and_stops(rusty_impulse: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let (bin, home) = (tmp.path().join("bin"), tmp.path().join("home"));
    let saved = home.join("My Videos");
    let cfg = home.join(".config/rusty-impulse");
    let rusty_impulse = tmp.path().join(rusty_impulse);
    for d in [&bin, &cfg, &tmp.path().join("run")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::create_dir_all(rusty_impulse.parent().unwrap()).unwrap();
    std::fs::write(
        cfg.join("config.json"),
        serde_json::json!({"screenRecord": {"savePath": saved}}).to_string(),
    )
    .unwrap();
    std::os::unix::fs::symlink(NATIVE, &rusty_impulse).unwrap();
    // The shells other tests run are not this user's.
    testenv::install_script(&bin.join("pidof"), "#!/bin/sh\nexit 1\n");
    let log = tmp.path().join("wf-recorder.log");
    // `#!/bin/bash` (not `env bash`) keeps the name the stop looks for. It writes down how it
    // was started and then records until it is told to end, as wf-recorder does: opening a
    // FIFO nobody writes to blocks it without a child of its own.
    testenv::install_script(
        &bin.join("wf-recorder"),
        &format!(
            "#!/bin/bash\n{{ printf '%s\\t' \"$@\" \"cwd=$PWD\"; echo; }} >> '{log}.new'\n\
             mkfifo '{log}.hold'\nmv '{log}.new' '{log}'\nread -r _ < '{log}.hold'\n",
            log = log.display()
        ),
    );
    let launch = |args: &[&str]| {
        let mut c = Command::new(script());
        c.args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", tmp.path().join("run"))
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        c
    };
    // Ctrl+Alt+R: the recorder runs, started by the script's process, which waits for it.
    let mut first = launch(&["--fullscreen"]).spawn().unwrap();
    testenv::wait_exists(&log);
    let line = std::fs::read_to_string(&log).unwrap();
    let words: Vec<&str> = line.trim_end_matches(['\t', '\n']).split('\t').collect();
    // No Hyprland here to name the focused monitor, and no PipeWire to name a sink.
    let file = words.get(5).copied().unwrap_or_default();
    assert_eq!(
        words,
        [
            "-o",
            "",
            "--pixel-format",
            "yuv420p",
            "-f",
            file,
            "-t",
            &format!("cwd={}", saved.display()),
        ],
        "{line:?}"
    );
    assert!(
        file.starts_with("./recording_") && file.ends_with(".mp4") && file.len() == 35,
        "{file}"
    );
    // Ctrl+Alt+R again: the recorder is told to end, and nothing else is started.
    let second = launch(&["--fullscreen"]).output().unwrap();
    assert_eq!(second.status.code(), Some(0), "{second:?}");
    // The first launch waited for its recorder: it ends once the recorder has.
    testenv::wait_exit(&mut first);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), line);
}

/// Super+Shift+R is two keybinds: the shell's region picker, and `qs ... ipc call TEST_ALIVE ||
/// record.sh` for a shell that is down. Where rusty-impulse is installed without Quickshell there is
/// no `qs` to ask, and the file runs on every press: it asks rusty-impulse, and starts nothing when a
/// shell answers. With `qs` installed, or with arguments, it does what the original's script does.
#[test]
fn a_region_press_is_left_to_a_shell_that_answers() {
    if !testenv::require_all(&["sh"]) {
        return;
    }
    // (qs installed, the shell answers, the script's arguments) -> what rusty-impulse is run with.
    let alive = "-c rusty-impulse ipc call TEST_ALIVE\n";
    for (qs, answers, args, ran, code) in [
        (false, true, &[][..], alive.to_owned(), 0),
        (false, false, &[][..], format!("{alive}__ri-record\n"), 7),
        (true, true, &[][..], "__ri-record\n".to_owned(), 7),
        (
            false,
            true,
            &["--fullscreen"][..],
            "__ri-record --fullscreen\n".to_owned(),
            7,
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, home) = (tmp.path().join("bin"), tmp.path().join("home"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();
        let log = tmp.path().join("rusty-impulse.log");
        // An rusty-impulse that writes down how it was run: `ipc call` tells whether a shell
        // answered, anything else ends with 7.
        testenv::install_script(
            &home.join(".local/bin/rusty-impulse"),
            &format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\n[ \"$3\" = ipc ] || exit 7\nexit {}\n",
                log.display(),
                i32::from(!answers)
            ),
        );
        testenv::install_script(&bin.join("pidof"), "#!/bin/sh\nexit 1\n");
        if qs {
            testenv::install_script(&bin.join("qs"), "#!/bin/sh\nexit 1\n");
        }
        let out = Command::new(script())
            .args(args)
            .env_clear()
            .env("PATH", &bin)
            .env("HOME", &home)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let case = format!("qs {qs}, answers {answers}, {args:?}: {out:?}");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap_or_default(),
            ran,
            "{case}"
        );
        assert_eq!(out.status.code(), Some(code), "{case}");
    }
}
