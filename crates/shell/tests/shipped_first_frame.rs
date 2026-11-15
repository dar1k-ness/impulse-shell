//! The shell as it runs on the PC draws: `impulse-shell -c impulse-shell`, how Hyprland starts it, with the very
//! tree its code was compiled from (`ricompiled::ROOT`) as the config named impulse-shell, and no
//! `RI_INTERP`, so every script runs as compiled code
//! (main.rs `set_compiled`), the only mode the AOT-only build has. Its bar's first frame has to
//! be on screen: a shell whose scripts cannot run stays alive with an empty screen, so a
//! process that merely started proves nothing. RI_SHIPPED_BIN and RI_SHIPPED_TREE name another
//! binary and tree to run (the gate's installed ones).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use testenv::Log;

struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The wl_surface id of the layer surface impulse-shell created for `namespace`.
fn layer_surface(log: &str, namespace: &str) -> Option<String> {
    let ns = format!("\"{namespace}\")");
    log.lines()
        .filter(|l| l.contains(".get_layer_surface(") && l.contains(&ns))
        .find_map(|l| {
            let (_, r) = l.split_once("wl_surface#")?;
            Some(r.split(|c: char| !c.is_ascii_digit()).next()?.to_owned())
        })
}

/// A wp_presentation_feedback requested for `wl_surface#surface` came back `presented`: a frame
/// of that surface is on screen.
fn presented(log: &str, surface: &str) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    let on = format!(".feedback(wl_surface#{surface}, new id wp_presentation_feedback#");
    lines.iter().enumerate().any(|(i, l)| {
        let Some(id) = l.split_once(&on).and_then(|(_, r)| r.split(')').next()) else {
            return false;
        };
        let (shown, dropped) = (
            format!("wp_presentation_feedback#{id}.presented("),
            format!("wp_presentation_feedback#{id}.discarded("),
        );
        lines[i + 1..]
            .iter()
            .find(|l| l.contains(&shown) || l.contains(&dropped))
            .is_some_and(|l| l.contains(&shown))
    })
}

/// Why the shell will never draw, once its log says so: a document it refused to load.
fn refused(log: &str) -> Option<&str> {
    log.lines()
        .find(|l| l.contains("no compiled code") || l.contains("panicked"))
}

/// The distinct RGB colours of sway's whole screen.
fn screen_colours(sway: &testenv::Sway, dir: &Path) -> usize {
    let f = dir.join("screen.ppm");
    eprintln!("waiting for grim");
    let out = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm"])
        .arg(&f)
        .stdin(Stdio::null())
        .output()
        .expect("run grim");
    assert!(
        out.status.success(),
        "grim: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let b = std::fs::read(&f).unwrap();
    // P6\n<w> <h>\n255\n<rgb>: the pixels follow the fourth whitespace-separated field.
    let mut i = 0;
    for _ in 0..4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    let mut seen = std::collections::HashSet::new();
    for px in b[i + 1..].as_chunks::<3>().0 {
        seen.insert(*px);
    }
    seen.len()
}

#[test]
fn compiled_tree_draws_its_bar() {
    if !testenv::require_all(&["sway", "grim", "fc-list"]) {
        return;
    }
    // Without the original's icon font every icon is drawn as its name, and a picture shows no wrong icon.
    let fonts = Command::new("fc-list")
        .arg("Material Symbols Rounded")
        .output();
    assert!(
        fonts.is_ok_and(|o| !o.stdout.is_empty()),
        "no Material Symbols Rounded font (tools/testenv/Containerfile installs it)"
    );
    // tools/testenv/ri-gate sets both to the binary and tree tools/install.sh installed.
    let tree =
        std::env::var_os("RI_SHIPPED_TREE").map_or_else(|| ricompiled::ROOT.into(), PathBuf::from);
    let tree = tree.as_path();
    assert!(tree.join("shell.qml").is_file(), "no QML tree at {tree:?}");
    // The test waits in three places with no deadline of its own (sway's start, the bar's
    // frame, grim): each is named on stderr before it, and sway's and the shell's logs are
    // copied there as they arrive, so a hang nextest ends shows where and after what.
    let sway = testenv::Sway::start_shown();
    let home = tempfile::tempdir().unwrap();
    let configs = home.path().join(".config/rustshell");
    std::fs::create_dir_all(&configs).unwrap();
    std::os::unix::fs::symlink(tree, configs.join("impulse-shell")).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut cmd = Command::new(
        std::env::var_os("RI_SHIPPED_BIN").unwrap_or(env!("CARGO_BIN_EXE_impulse-shell").into()),
    );
    sway.env(&mut cmd)
        .args(["-c", "impulse-shell"])
        .envs(testenv::home_vars(home.path()))
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RI_INTERP")
        .env("WAYLAND_DEBUG", "1")
        .env("RI_LOG", "ri::present=debug")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let shell = Kill(cmd.spawn().unwrap());
    eprintln!("waiting for the bar's first frame");
    let log = Log::capture_shown("impulse-shell", r);
    let waited = log.wait(|s| {
        refused(s).is_some()
            || layer_surface(s, "rustshell:bar").is_some_and(|id| presented(s, &id))
    });
    let text = log.text();
    if let Some(why) = refused(&text) {
        panic!("impulse-shell refused the QML tree it was compiled from: {why}");
    }
    if waited.is_err() {
        panic!(
            "impulse-shell exited before its bar was on screen:\n{}",
            tail(&text)
        );
    }
    let colours = screen_colours(&sway, home.path());
    drop(shell);
    // An empty screen is sway's one background colour; the bar's text alone brings dozens.
    assert!(
        colours >= 16,
        "the bar was presented, yet the screen has {colours} colours"
    );
}

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| !l.contains(" -> ") && !l.contains("@"))
        .collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}
