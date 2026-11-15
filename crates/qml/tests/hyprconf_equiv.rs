//! `qml::hyprconf` (native HyprlandConfig.set/reset) against the original
//! `scripts/hyprland/hyprconfigurator.py` it replaces: same file text and mode for every case.
//! Fails when python3 is unavailable (`testenv`).

use qml::hyprconf::{Edit, run};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    match std::env::var_os("HYPRCONF_SCRIPT") {
        Some(p) => p.into(),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/original-unshipped/scripts/hyprland/hyprconfigurator.py"),
    }
}

fn python() -> Option<&'static str> {
    ["/usr/bin/python3", "python3"].into_iter().find(|p| {
        Command::new(p)
            .arg("-c")
            .arg("pass")
            .status()
            .is_ok_and(|s| s.success())
    })
}

const BASE: &str = "-- shell overrides\n\
hl.config({general={gaps_in=5}})\n\
hl.config({ general = { gaps_out = 10 } })\n\
  hl.config({animations={enabled=true}})\n\
hl.config({decoration={blur={enabled=false}}})\n\
hl.config({decoration={rounding=8}})\n\
hl.config({decoration={screen_shader=\"/tmp/a.frag\"}})\n\
hl.config({misc={vfr=1}})\n\
hl.bind(\"SUPER\", \"Q\", hl.dsp.kill())\n\
hl.config({general={gaps_in=7}})";

fn cases() -> Vec<(&'static str, Option<String>, Edit)> {
    let e = |set: &[(&str, &str)], reset: &[&str]| Edit {
        file: String::new(),
        set: set
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        reset: reset.iter().map(|k| k.to_string()).collect(),
    };
    let base = Some(BASE.to_string());
    vec![
        (
            "replace nested",
            base.clone(),
            e(&[("general:gaps_in", "0")], &[]),
        ),
        (
            "append missing",
            base.clone(),
            e(&[("general:border_size", "1")], &[]),
        ),
        (
            "bool",
            base.clone(),
            e(&[("animations:enabled", "false")], &[]),
        ),
        (
            "string value",
            base.clone(),
            e(&[("decoration:screen_shader", "/home/u/shader.frag")], &[]),
        ),
        (
            "deep key",
            base.clone(),
            e(&[("decoration:blur:enabled", "true")], &[]),
        ),
        (
            "top-level key",
            base.clone(),
            e(&[("monitor", "DP-1,1920x1080,0x0,1")], &[]),
        ),
        (
            "reset",
            base.clone(),
            e(&[], &["decoration:rounding", "general:gaps_in"]),
        ),
        ("reset missing", base.clone(), e(&[], &["nope:nothing"])),
        (
            "[[EMPTY]]",
            base.clone(),
            e(&[("decoration:screen_shader", "[[EMPTY]]")], &[]),
        ),
        (
            "game mode on",
            base.clone(),
            e(
                &[
                    ("animations:enabled", "false"),
                    ("decoration:shadow:enabled", "false"),
                    ("decoration:blur:enabled", "false"),
                    ("general:gaps_in", "0"),
                    ("general:gaps_out", "0"),
                    ("general:border_size", "1"),
                    ("decoration:rounding", "0"),
                    ("general:allow_tearing", "true"),
                ],
                &[],
            ),
        ),
        (
            "game mode off",
            base.clone(),
            e(
                &[],
                &[
                    "animations:enabled",
                    "decoration:shadow:enabled",
                    "decoration:blur:enabled",
                    "general:gaps_in",
                    "general:gaps_out",
                    "general:border_size",
                    "decoration:rounding",
                    "general:allow_tearing",
                ],
            ),
        ),
        (
            "duplicate set keys",
            base.clone(),
            e(
                &[
                    ("general:gaps_in", "1"),
                    ("misc:vfr", "0"),
                    ("general:gaps_in", "2"),
                ],
                &[],
            ),
        ),
        (
            "set and reset",
            base.clone(),
            e(&[("misc:vfr", "0")], &["misc:vfr"]),
        ),
        (
            "numbers",
            base.clone(),
            e(
                &[
                    ("a:b", "1e5"),
                    ("a:c", " 3 "),
                    ("a:d", "1_000"),
                    ("a:e", "-.5"),
                    ("a:f", "inf"),
                    ("a:g", "NaN"),
                    ("a:h", "1."),
                    ("a:i", "0x10"),
                    ("a:j", "1__0"),
                    ("a:k", "+"),
                ],
                &[],
            ),
        ),
        (
            "regex chars in key",
            base.clone(),
            e(&[("we.ird:k(e)y+", "v")], &[]),
        ),
        (
            "crlf and cr",
            Some("hl.config({x=1})\r\nhl.config({y=2})\rlast".into()),
            e(&[("y", "3")], &[]),
        ),
        (
            "no trailing newline append",
            Some("hl.config({x=1})".into()),
            e(&[("z", "abc")], &[]),
        ),
        (
            "empty file",
            Some(String::new()),
            e(&[("general:gaps_in", "4")], &[]),
        ),
        ("missing file", None, e(&[("general:gaps_in", "4")], &[])),
        ("missing file reset", None, e(&[], &["general:gaps_in"])),
        (
            "unicode",
            Some("-- ключ\nhl.config({input={kb_layout=\"us\"}})\n".into()),
            e(
                &[("input:kb_layout", "us,ru"), ("input:kb_variant", "ëx")],
                &[],
            ),
        ),
        ("nothing", base.clone(), e(&[], &[])),
    ]
}

#[test]
fn matches_hyprconfigurator_py() {
    let Some(py) = python() else {
        testenv::unavailable("python3 not found");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    for (i, (name, init, edit)) in cases().into_iter().enumerate() {
        let a = tmp.path().join(format!("{i}/py/sub/main.lua"));
        let b = tmp.path().join(format!("{i}/rs/sub/main.lua"));
        if let Some(t) = &init {
            for f in [&a, &b] {
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(f, t).unwrap();
                std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        let mut cmd = Command::new(py);
        cmd.arg(script()).arg("--file").arg(&a);
        for (k, v) in &edit.set {
            cmd.arg("--set").arg(k).arg(v);
        }
        for k in &edit.reset {
            cmd.arg("--reset").arg(k);
        }
        let st = cmd.output().unwrap();
        assert!(
            st.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&st.stderr)
        );
        let _ = run(&Edit {
            file: b.to_string_lossy().into(),
            ..edit
        });
        let read = |f: &Path| std::fs::read(f).ok();
        let mode = |f: &Path| std::fs::metadata(f).ok().map(|m| m.permissions().mode());
        assert_eq!(read(&b), read(&a), "{name}: contents");
        assert_eq!(mode(&b), mode(&a), "{name}: mode");
        let left = |f: &Path| {
            std::fs::read_dir(f.parent().unwrap())
                .map(|d| d.count())
                .unwrap_or(0)
        };
        assert_eq!(left(&b), left(&a), "{name}: files in the directory");
    }
}

/// The edits the original makes (anti-flashbang on/off, game mode on/off) on a copy of a real overrides
/// file: `HYPRCONF_FILE=~/.config/hypr/hyprland/shellOverrides/main.lua cargo test --features live --test
/// hyprconf_equiv -- real_file`. The file itself is never written.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn real_file() {
    let (Some(py), Some(src)) = (python(), std::env::var_os("HYPRCONF_FILE")) else {
        testenv::unavailable("needs python3 and HYPRCONF_FILE");
        return;
    };
    let text = std::fs::read(&src).ok();
    let e = |set: &[(&str, &str)], reset: &[&str]| Edit {
        file: String::new(),
        set: set
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        reset: reset.iter().map(|k| k.to_string()).collect(),
    };
    let shader = "/home/u/.config/rustshell/impulse-shell/services/hyprlandAntiFlashbangShader/anti-flashbang.glsl";
    let game = [
        ("animations:enabled", "false"),
        ("decoration:shadow:enabled", "false"),
        ("decoration:blur:enabled", "false"),
        ("general:gaps_in", "0"),
        ("general:gaps_out", "0"),
        ("general:border_size", "1"),
        ("decoration:rounding", "0"),
        ("general:allow_tearing", "true"),
    ];
    let keys: Vec<&str> = game.iter().map(|g| g.0).collect();
    let steps = [
        e(
            &[
                ("decoration:screen_shader", shader),
                ("debug:damage_tracking", "1"),
            ],
            &[],
        ),
        e(&[], &["decoration:screen_shader", "debug:damage_tracking"]),
        e(&game, &[]),
        e(&[], &keys),
    ];
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("py/main.lua");
    let b = tmp.path().join("rs/main.lua");
    for f in [&a, &b] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        if let Some(t) = &text {
            std::fs::write(f, t).unwrap();
        }
    }
    for (i, edit) in steps.into_iter().enumerate() {
        let mut cmd = Command::new(py);
        cmd.arg(script()).arg("--file").arg(&a);
        for (k, v) in &edit.set {
            cmd.arg("--set").arg(k).arg(v);
        }
        for k in &edit.reset {
            cmd.arg("--reset").arg(k);
        }
        assert!(cmd.status().unwrap().success());
        let _ = run(&Edit {
            file: b.to_string_lossy().into(),
            ..edit
        });
        assert_eq!(std::fs::read(&b).ok(), std::fs::read(&a).ok(), "step {i}");
        println!(
            "real_file step {i}: identical ({} bytes)",
            std::fs::metadata(&b).map_or(0, |m| m.len())
        );
    }
    assert_eq!(std::fs::read(&src).ok(), text, "source file untouched");
}
