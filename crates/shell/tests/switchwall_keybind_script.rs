//! The original's Hyprland keybind for the wallpaper (Ctrl+Super+T) runs a file of the config tree when
//! no shell answers it: `scripts/colors/switchwall.sh`, which asks for the image with kdialog.
//! The tree `tools/install.sh` installs has that file, and it hands the launch to rusty-impulse, found
//! in PATH or where an install for one user puts it (`~/.local/bin`, which a compositor's PATH
//! may lack), with the arguments it was given.
//! What the port does with the chosen image is compared against the original's script in
//! crates/colorgen/tests/switchwall.rs.

use std::path::Path;
use std::process::{Command, Stdio};

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

fn script() -> std::path::PathBuf {
    Path::new(ricompiled::ROOT).join("scripts/colors/switchwall.sh")
}

#[test]
fn the_tree_has_the_file_the_keybind_runs() {
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
fn the_keybind_launch_asks_for_the_wallpaper() {
    if !testenv::require_all(&["sh", "bash"]) {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (bin, home) = (tmp.path().join("bin"), tmp.path().join("home"));
    let walls = home.join("Wallpapers");
    for d in [
        &bin,
        &walls,
        &home.join(".local/bin"),
        &tmp.path().join("run"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    // rusty-impulse is not in PATH, as under a compositor started without ~/.local/bin in it.
    std::os::unix::fs::symlink(NATIVE, home.join(".local/bin/rusty-impulse")).unwrap();
    let log = tmp.path().join("kdialog.log");
    // The dialog writes down how it was asked and is closed without a choice.
    testenv::install_script(
        &bin.join("kdialog"),
        &format!(
            "#!/bin/bash\n{{ printf '%s\\t' \"$@\" \"cwd=$PWD\"; echo; }} >> '{}'\nexit 1\n",
            log.display()
        ),
    );
    let out = Command::new(script())
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", tmp.path().join("run"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let asked = std::fs::read_to_string(&log).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    assert_eq!(
        asked,
        format!(
            "--getopenfilename\t.\t--title\tChoose wallpaper\tcwd={}\t\n",
            walls.display()
        ),
        "{out:?}"
    );
}

#[test]
fn the_script_hands_its_arguments_to_the_rusty_impulse_in_path() {
    if !testenv::require_all(&["sh"]) {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let (bin, home) = (tmp.path().join("bin"), tmp.path().join("home"));
    for d in [&bin, &home, &tmp.path().join("run")] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::os::unix::fs::symlink(NATIVE, bin.join("rusty-impulse")).unwrap();
    // Nothing in this home names a wallpaper or a colour: `--noswitch` has nothing to keep.
    let out = Command::new(script())
        .arg("--noswitch")
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", &home)
        .env("XDG_RUNTIME_DIR", tmp.path().join("run"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        err.contains("--noswitch: no wallpaper or seed color to generate colors from"),
        "{out:?}"
    );
}
