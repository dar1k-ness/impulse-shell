//! The original's process and directory questions at the QML level (`Impulse.Processes`, `Files`,
//! `System`, the probe `Process`es and ported bodies left): the values the handlers set must
//! be the ones the commands the original used to run give (computed here through bash first, on the
//! caller's PATH). The runtime's PATH holds only fake `hyprlock`/`hyprsunset`, so neither a
//! shell nor the real tools can be what answers.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use testenv::Running;

mod runtime_host;
use runtime_host::Host;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The caller's PATH, which the original's command lines run with ([`bash`]); this process's own PATH
/// holds only the fakes.
const TOOLS_PATH: &str = "PROBE_QML_TOOLS_PATH";

fn bash(script: &str) -> (String, i32) {
    let o = Command::new("bash")
        .env("PATH", std::env::var_os(TOOLS_PATH).unwrap())
        .args(["-c", script])
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

/// Runs `ms` of the loop's virtual time, until every Process and pool job started meanwhile
/// (detached starts and kills included) has ended.
fn run(lp: &mut Host, rt: &mut Runtime, ms: f64) {
    lp.advance(rt, ms);
    lp.idle(rt);
}

/// A long-running process called `name` (a renamed `cat`), running when this returns.
fn fake(dir: &Path, name: &str) -> Running {
    Running::spawn(&mut Command::new(testenv::cat_copy(&dir.join(name))))
}

fn pids(name: &str) -> Vec<String> {
    qml::probe::pidof(&[name.to_string()])
        .out
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Called once whatever was to signal it has returned (see `testenv::Running::alive`).
fn gone(c: &mut Running) -> bool {
    c.alive().is_err()
}

/// In its own process (`testenv::run_in_own_process`), whose HOME, XDG config and state
/// homes are in a fresh directory and whose PATH is only its `bin`.
#[test]
fn probes_set_what_the_commands_give() {
    const TEST: &str = concat!(module_path!(), "::probes_set_what_the_commands_give");
    if !testenv::in_own_process(TEST) {
        let base = tempfile::tempdir().unwrap();
        let b = base.path();
        // Started by lock() / startHyprsunset() (`pidof P || P`): each forks the long-running
        // P and exits once it runs. Installed here, where PATH has install(1).
        for f in ["hyprlock", "hyprsunset"] {
            std::fs::create_dir_all(b.join("bin")).unwrap();
            testenv::install_daemon(&b.join("bin").join(f), "");
        }
        testenv::run_in_own_process(
            TEST,
            [
                ("HOME", b.join("home").into_os_string()),
                ("XDG_CONFIG_HOME", b.join("config").into_os_string()),
                ("XDG_STATE_HOME", b.join("state").into_os_string()),
                ("PATH", b.join("bin").into_os_string()),
                (TOOLS_PATH, std::env::var_os("PATH").unwrap_or_default()),
            ],
        );
        return;
    }
    let var = |k| PathBuf::from(std::env::var_os(k).unwrap());
    let (home, cfg, bin) = (var("HOME"), var("XDG_CONFIG_HOME"), var("PATH"));
    let base = home.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(home.join("Downloads")).unwrap();
    std::fs::create_dir_all(cfg.join("impulse-shell")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        cfg.join("impulse-shell/config.json"),
        r#"{"conflictKiller": {"autoKillNotificationDaemons": true}, "lock": {"useHyprlock": true}}"#,
    )
    .unwrap();
    std::fs::write(home.join("Downloads/x.part"), "").unwrap();
    let (mut q1, mut q2, mut mako) = (
        fake(&base, "ri-probe-q1"),
        fake(&base, "ri-probe-q2"),
        fake(&base, "mako"),
    );

    // What the original's own command lines give, through bash on the caller's PATH.
    let sh = |s: &str| bash(&format!("HOME='{}'; {s}", home.display()));
    let pkg = sh("pidof yay paru dnf zypper apt apx xbps snap apk yum epsi pikman || ls /var/lib/pacman/db.lck").1 == 0;
    let dl =
        sh("pidof curl wget aria2c yt-dlp || ls ~/Downloads | grep -E '\\.crdownload$|\\.part$'").1
            == 0;
    let user = bash("whoami").0.trim().to_string();
    let de = bash("echo $XDG_CURRENT_DESKTOP,$WAYLAND_DISPLAY").0;
    let (desktop, wayland) = de.split_once(',').unwrap();
    let cpu = bash("LANG=C LC_ALL=C lscpu | grep 'CPU max MHz' | awk '{print $4}'").0;
    let pid_found = bash("pidof ri-probe-q1 ri-probe-q2").1;
    let pid_none = bash("pidof ri-probe-q3").1;

    // Midday: Hyprsunset's night schedule has not started hyprsunset before
    // `startHyprsunset()` below does.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(ri()));
    rt.load_singletons(&[
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    // killDialog's group and LockScreen's lock(), as the original writes them on `Impulse.Processes`.
    let group = |programs: &str, result: &str| {
        format!(
            "Item {{ id: conflictGroup\n\
             property list<string> programs: [{programs}]\n\
             property int query: -1\n\
             signal alwaysSelected\n\
             signal yes\n\
             signal always\n\
             onAlwaysSelected: root.alwaysSelected = true\n\
             onYes: {{ Processes.terminate(conflictGroup.programs); conflictGroup.visible = false }}\n\
             onAlways: {{ Processes.terminate(conflictGroup.programs); conflictGroup.alwaysSelected(); conflictGroup.visible = false }}\n\
             Component.onCompleted: query = Processes.query(conflictGroup.programs)\n\
             Connections {{ target: Processes\n\
             function onAnswered(id, running) {{\n\
             if (id === conflictGroup.query) root.{result} = running.length > 0 ? 0 : 1 }} }} }}\n"
        )
    };
    let head = "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nimport Impulse\nimport ri\nimport ri.services\nimport ri.modules.common\n";
    let r = rt
        .load_str(&format!(
            "{head}Item {{ id: root\n\
             property var sw: SessionWarnings\n\
             property var si: SystemInfo\n\
             property var ru: ResourceUsage\n\
             property var ck: ConflictKiller\n\
             property var hs: Hyprsunset\n\
             property int found: -1\n\
             property bool alwaysSelected: false\n\
             property var grp: conflictGroup\n\
             Component.onCompleted: SessionWarnings.refresh()\n\
             function lock() {{ Processes.startUnlessRunning(\"hyprlock\") }}\n\
             {}\
             }}",
            group("\"ri-probe-q1\", \"ri-probe-q2\"", "found"),
        ))
        .unwrap();
    let r2 = rt
        .load_str(&format!(
            "{head}Item {{ id: root\n property int none: -1\n property bool alwaysSelected: false\n\
             property var grp: conflictGroup\n {}}}",
            group("\"ri-probe-q3\"", "none")
        ))
        .unwrap();
    let mut lp = Host::new(&mut rt);
    run(&mut lp, &mut rt, 2000.0);
    let item = |rt: &Runtime, r: usize, o: &str| match rt.get(r, o) {
        Value::Item(i) => i,
        v => panic!("{o}: {v:?} {:?}", rt.errors()),
    };
    let get = |rt: &Runtime, o: &str, k: &str| rt.get(item(rt, r, o), k);
    assert_eq!(get(&rt, "sw", "packageManagerRunning").truthy(), pkg);
    assert_eq!(get(&rt, "sw", "downloadRunning").truthy(), dl);
    assert!(dl, "x.part must count as a download");
    assert_eq!(get(&rt, "si", "username").to_js_string(), user);
    assert_eq!(
        get(&rt, "si", "desktopEnvironment").to_js_string(),
        desktop.trim()
    );
    let ws = if wayland.trim().is_empty() {
        "X11"
    } else {
        "Wayland"
    };
    assert_eq!(get(&rt, "si", "windowingSystem").to_js_string(), ws);
    let ghz = match cpu.trim().parse::<f64>() {
        Ok(m) => format!("{} GHz", (m / 1000.0).round()),
        // No cpufreq (a VM): the label keeps "--" rather than the original's "NaN GHz".
        Err(_) => "--".into(),
    };
    assert_eq!(get(&rt, "ru", "maxAvailableCpuString").to_js_string(), ghz);
    assert_eq!(rt.get(r, "found").to_number() as i32, pid_found);
    assert_eq!(rt.get(r2, "none").to_number() as i32, pid_none);

    // ConflictKiller found `mako` and, with autoKillNotificationDaemons, killed it.
    assert!(
        gone(&mut mako),
        "ConflictKiller must killall mako {:?}",
        rt.errors()
    );

    // killDialog "Yes" / "Always".
    let (g, g2) = (item(&rt, r, "grp"), item(&rt, r2, "grp"));
    rt.fire(g, "onYes", &[], vec![]);
    run(&mut lp, &mut rt, 100.0);
    assert!(gone(&mut q1) && gone(&mut q2), "Yes must killall the group");
    assert!(!rt.get(g, "visible").truthy());
    assert!(!rt.get(r, "alwaysSelected").truthy());
    rt.fire(g2, "onAlways", &[], vec![]);
    run(&mut lp, &mut rt, 100.0);
    assert!(!rt.get(g2, "visible").truthy());
    assert!(rt.get(r2, "alwaysSelected").truthy());

    // lock() / startHyprsunset(): start the program once, not again while it runs.
    for (target, f, prog) in [
        (r, "lock", "hyprlock"),
        (item(&rt, r, "hs"), "startHyprsunset", "hyprsunset"),
    ] {
        let before = pids(prog).len();
        for _ in 0..2 {
            rt.call_method(target, f, vec![]).unwrap().unwrap();
            run(&mut lp, &mut rt, 300.0);
        }
        let now = pids(prog);
        assert_eq!(now.len(), before.max(1), "{prog}: {now:?}");
        for p in now {
            let _ = Command::new("/bin/kill").arg(p).status();
        }
    }
}
