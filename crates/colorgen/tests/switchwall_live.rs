//! Live A/B of `colorgen::switchwall` against the original's real `scripts/colors/switchwall.sh`: for
//! each argv shape the original launches it with, the original script runs (bash, real matugen and the
//! venv's `generate_colors_material.py`) and the port runs (`run`) in identical sandboxes,
//! with temp HOME or XDG_CONFIG/STATE/CACHE homes and the same argv-recording stubs for every
//! other external tool. Exit code, stdout, the leaf commands and every generated file must
//! match.
use colorgen::switchwall::{Io, Paths, main_io, wait_background};
use colorgen::sys::Sys;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/switchwall");
const II: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml");
/// The original's scripts, which the shell no longer ships: the reference side.
const UNSHIPPED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../qml/tests/fixtures/original-unshipped"
);
const MONITORS: &str = r#"[{"name":"DP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true},{"name":"HDMI-A-1","width":1280,"height":720,"x":1920,"y":0,"scale":1.0,"focused":false}]"#;

/// HOME / XDG_* are process-wide.
static ENV: Mutex<()> = Mutex::new(());

use testenv::{require, unavailable};

fn which(tool: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .find(|d| d.join(tool).is_file())
        .unwrap()
}

fn venv() -> Option<String> {
    let v = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    let ok = !v.is_empty()
        && Command::new(format!("{v}/bin/python3"))
            .args(["-c", "import materialyoucolor, PIL"])
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        unavailable("RUSTY_IMPULSE_VIRTUAL_ENV with materialyoucolor + pillow");
        return None;
    }
    Some(v)
}

/// gen.sh's logging fakes: `<name> $*` to `$SW_LOG`.
fn stubs(bin: &Path) {
    std::fs::create_dir_all(bin).unwrap();
    let fake = |name: &str, body: &str| {
        let p = bin.join(name);
        testenv::install_script(
            &p,
            &format!("#!/usr/bin/env bash\necho \"{name} $*\" >> \"$SW_LOG\"\n{body}\n"),
        );
    };
    fake(
        "gsettings",
        "[[ $1 == get ]] && echo \"'prefer-dark'\"; exit 0",
    );
    fake(
        "hyprctl",
        &format!(
            "case \"$1\" in monitors) echo '{MONITORS}';; cursorpos) echo '{{\"x\":100,\"y\":100}}';; esac"
        ),
    );
    for n in ["pkill", "pgrep", "pidof"] {
        fake(n, "exit 1");
    }
    for n in ["mpvpaper", "notify-send", "kdialog", "upscayl"] {
        fake(n, "");
    }
    fake("hyprpicker", "echo \"#3A5F0B\"");
    fake("ffmpeg", "cp \"$SW_FIX/thumb.jpg\" \"${@: -1}\"");
    fake("xdg-user-dir", "echo \"$HOME/Pictures\"");
    fake(
        "identify",
        "\"$VENV/bin/python3\" -c \"import sys;from PIL import Image;i=Image.open(sys.argv[2]);print(i.width if sys.argv[1]=='%w' else i.height,end='')\" \"$2\" \"$3\"",
    );
}

fn copy_tree(from: &Path, to: &Path) {
    let Ok(rd) = std::fs::read_dir(from) else {
        return;
    };
    std::fs::create_dir_all(to).unwrap();
    for e in rd.flatten() {
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), &dst).unwrap();
        }
    }
}

struct Sandbox {
    h: PathBuf,
    env: Vec<(String, String)>,
    paths: Paths,
}

/// gen.sh's `case_` sandbox; `xdg` moves config/state/cache out of HOME's defaults.
fn sandbox(h: &Path, bin: &Path, venv: &str, xdg: bool, setup: &str) -> Sandbox {
    let _ = std::fs::remove_dir_all(h);
    std::fs::create_dir_all(h).unwrap();
    let fixture = Path::new(DIR).join("fixture/home");
    let (cfg, state, cache) = if xdg {
        (
            h.join("xdg-config"),
            h.join("xdg-state"),
            h.join("xdg-cache"),
        )
    } else {
        (h.join(".config"), h.join(".local/state"), h.join(".cache"))
    };
    for e in std::fs::read_dir(&fixture).unwrap().flatten() {
        let n = e.file_name();
        if n != ".config" && n != ".local" && n != ".cache" {
            copy_tree(&e.path(), &h.join(&n));
        }
    }
    copy_tree(&fixture.join(".config"), &cfg);
    if xdg {
        // The original's matugen config names its templates `~/.config/matugen/templates/...`.
        copy_tree(&fixture.join(".config/matugen"), &h.join(".config/matugen"));
    }
    copy_tree(&fixture.join(".local/state"), &state);
    copy_tree(&fixture.join(".cache"), &cache);
    let qs = cfg.join("rustshell/rusty-impulse");
    // The terminal templates the shell ships, then the original's switchwall.sh and its helpers over
    // them: the shell's own switchwall.sh runs the port, and the reference side is the original's.
    copy_tree(&Path::new(II).join("scripts"), &qs.join("scripts"));
    copy_tree(&Path::new(UNSHIPPED).join("scripts"), &qs.join("scripts"));
    std::fs::create_dir_all(qs.join("assets/images")).unwrap();
    std::fs::copy(
        Path::new(II).join("assets/images/default_wallpaper.png"),
        qs.join("assets/images/default_wallpaper.png"),
    )
    .unwrap();
    let ac = qs.join("scripts/colors/applycolor.sh");
    let s = std::fs::read_to_string(&ac)
        .unwrap()
        .replace("/dev/pts/*", "$SW_PTS/*")
        .replace("^/dev/pts/", "^$SW_PTS/");
    std::fs::write(&ac, s).unwrap();
    std::fs::create_dir_all(h.join("pts")).unwrap();
    for t in ["0", "1", "ptmx"] {
        std::fs::write(h.join("pts").join(t), "").unwrap();
    }
    let matugen = which("matugen");
    let mut env = vec![
        ("HOME".to_string(), h.display().to_string()),
        (
            "PATH".into(),
            format!("{}:{}:/usr/bin:/bin", bin.display(), matugen.display()),
        ),
        ("SW_LOG".into(), h.join("calls.log").display().to_string()),
        ("SW_PTS".into(), h.join("pts").display().to_string()),
        ("SW_FIX".into(), format!("{DIR}/fixture")),
        ("VENV".into(), venv.into()),
        ("RUSTY_IMPULSE_VIRTUAL_ENV".into(), venv.into()),
    ];
    if xdg {
        env.push(("XDG_CONFIG_HOME".into(), cfg.display().to_string()));
        env.push(("XDG_STATE_HOME".into(), state.display().to_string()));
        env.push(("XDG_CACHE_HOME".into(), cache.display().to_string()));
    }
    let st = Command::new("bash")
        .args(["-c", setup])
        .current_dir(h)
        .env_clear()
        .envs(env.iter().cloned())
        .env("W", h.join("Pictures/Wallpapers"))
        .env("CFG", cfg.join("rusty-impulse/config.json"))
        .env("GEN", state.join("rustshell/user/generated"))
        .status()
        .unwrap();
    assert!(st.success(), "setup {setup}");
    let paths = Paths {
        home: h.to_path_buf(),
        config_home: cfg,
        cache_home: cache,
        state_home: state,
        script_dir: qs.join("scripts/colors"),
    };
    Sandbox {
        h: h.to_path_buf(),
        env,
        paths,
    }
}

/// The port's side effects through the same stubs the script's sandbox has.
struct Stubbed {
    h: PathBuf,
    env: Vec<(String, String)>,
}

impl Stubbed {
    fn cmd(&self, argv: &[String], cwd: Option<&Path>) -> Command {
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..])
            .env_clear()
            .envs(self.env.iter().cloned())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(cwd.unwrap_or(&self.h));
        c
    }
    fn call(&self, argv: &[&str]) -> Option<String> {
        let v: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        self.run(&v, None)
    }
}

impl Sys for Stubbed {
    /// The native recolour is A/B'd against the real tool in `tests/kde.rs` and its variant
    /// mapping in `tests/switchwall.rs`; here it is a no-op.
    fn kde_recolor(
        &self,
        dirs: &colorgen::kde::Dirs,
        seed: &str,
        _dark: bool,
        sv: usize,
    ) -> colorgen::kde::Outcome {
        assert!(
            dirs.config.starts_with(&self.h),
            "{}",
            dirs.config.display()
        );
        assert!(
            seed.starts_with('#') && seed.len() == 7 && sv < 9,
            "{seed} {sv}"
        );
        colorgen::kde::Outcome::Applied(Default::default())
    }
    fn hypr_json(&self, cmd: &str) -> Option<serde_json::Value> {
        assert_eq!(cmd, "monitors");
        serde_json::from_str(MONITORS).ok()
    }
    fn gsettings_get(&self, key: &str) -> Option<String> {
        let o = self.call(&["gsettings", "get", "org.gnome.desktop.interface", key])?;
        Some(o.trim().trim_matches('\'').to_string())
    }
    fn gsettings_set(&self, key: &str, value: &str) {
        self.call(&[
            "gsettings",
            "set",
            "org.gnome.desktop.interface",
            key,
            value,
        ]);
    }
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> Option<String> {
        let o = self.cmd(argv, cwd).output().ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }
    fn spawn(&self, argv: &[String]) {
        if let Ok(mut c) = self.cmd(argv, None).stdout(Stdio::null()).spawn() {
            let _ = c.wait();
        }
    }
    fn has_command(&self, name: &str) -> bool {
        let path = &self.env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        std::env::split_paths(path).any(|d| d.join(name).is_file())
    }
    /// Native (`tests/kill_exact.rs`); the script's `pkill -f -9 mpvpaper` is dropped from its
    /// leaf commands.
    fn kill_exact_wait(&self, _: &str, _: i32) {}
    fn signal_named(&self, name: &str, _sig: i32) {
        self.call(&["pgrep", "-f", name]);
    }
    fn ttys(&self) -> Vec<PathBuf> {
        vec![self.h.join("pts/0"), self.h.join("pts/1")]
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        std::fs::write(tty, data).unwrap();
    }
    fn notify(
        &self,
        app: &str,
        category: Option<&str>,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        then: colorgen::sys::OnAction,
    ) {
        let acts: Vec<String> = actions
            .iter()
            .flat_map(|(k, l)| ["-A".to_string(), format!("{k}={l}")])
            .collect();
        let mut argv: Vec<String> = vec!["notify-send".into()];
        match category {
            None => {
                argv.extend([summary.to_string(), body.to_string()]);
                argv.extend(acts);
                argv.extend(["-a".into(), app.into()]);
            }
            Some(c) => {
                argv.extend(["-a".into(), app.into(), "-c".into(), c.into()]);
                argv.extend(acts);
                argv.extend([summary.to_string(), body.to_string()]);
            }
        }
        then(
            self.run(&argv, None)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
}

#[derive(Debug, PartialEq)]
struct Outcome {
    code: i32,
    stdout: String,
    calls: Vec<String>,
    files: Vec<(String, Vec<u8>)>,
    stderr: String,
}

fn outcome(h: &Path, code: i32, stdout: String, stderr: String) -> Outcome {
    let hs = h.to_str().unwrap();
    let mut calls: Vec<String> = std::fs::read_to_string(h.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| {
            // The port does these in-process (hypr socket, dconf, image headers, user-dirs).
            !l.starts_with("hyprctl ")
                && !l.starts_with("gsettings get")
                && !l.starts_with("identify ")
                && !l.starts_with("xdg-user-dir ")
        })
        .map(|l| l.replace(hs, "@HOME@"))
        .collect();
    calls.sort();
    let mut files = Vec::new();
    let mut stack = vec![h.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(h).unwrap().to_string_lossy().into_owned();
            if rel.ends_with("rustshell/rusty-impulse")
                || ["calls.log", "Pictures"].contains(&rel.as_str())
            {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let b = std::fs::read(&p).unwrap();
            let b = match String::from_utf8(b.clone()) {
                Ok(s) => s
                    .replace(hs, "@HOME@")
                    .lines()
                    .map(|l| {
                        if l.starts_with("# Time: ") {
                            "# Time: @DATE@"
                        } else {
                            l
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes(),
                Err(_) => b,
            };
            files.push((rel, b));
        }
    }
    files.sort();
    Outcome {
        code,
        stdout: stdout.replace(hs, "@HOME@"),
        calls,
        files,
        stderr,
    }
}

fn original(s: &Sandbox, args: &[String]) -> Outcome {
    // The script's background jobs (`&`, `& disown`) finish after it exits.
    let o = testenv::output_with_descendants(
        Command::new("bash")
            .arg(s.paths.script_dir.join("switchwall.sh"))
            .args(args)
            .current_dir(&s.h)
            .env_clear()
            .envs(s.env.iter().cloned())
            .stdin(Stdio::null()),
    )
    .unwrap();
    outcome(
        &s.h,
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn native(s: &Sandbox, args: &[String]) -> Outcome {
    let sys: Arc<dyn Sys> = Arc::new(Stubbed {
        h: s.h.clone(),
        env: s.env.clone(),
    });
    let out = Mutex::new(String::new());
    let push = |x: String| out.lock().unwrap().push_str(&x);
    let err = Mutex::new(String::new());
    let push_err = |x: String| err.lock().unwrap().push_str(&x);
    let code = main_io(
        args,
        &s.paths,
        &sys,
        Io {
            out: &push,
            err: &push_err,
        },
    );
    wait_background();
    let stdout = out.lock().unwrap().clone();
    let stderr = err.lock().unwrap().clone();
    outcome(&s.h, code, stdout, stderr)
}

fn cases(xdg: bool) {
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let Some(venv) = venv() else { return };
    if !["bash", "jq", "matugen"].iter().all(|t| require(t)) {
        return;
    }
    let root = std::env::temp_dir().join(format!(
        "switchwall-live-{}-{}",
        if xdg { "xdg" } else { "home" },
        std::process::id()
    ));
    let bin = root.join("bin");
    stubs(&bin);
    let wall_set = r#"jq ".background.wallpaperPath = \"$W/syn_grad.jpg\"" $CFG > c && mv c $CFG"#;
    let wall_expressive =
        format!("{wall_set} && mkdir -p $GEN && echo scheme-expressive > $GEN/scheme_type.txt");
    let table: &[(&str, &str, &[&str])] = &[
        (
            "noswitch keeps recorded scheme",
            &wall_expressive,
            &["--mode", "dark", "--noswitch"],
        ),
        (
            "type auto noswitch re-detects",
            &wall_expressive,
            &["--type", "auto", "--mode", "dark", "--noswitch"],
        ),
        (
            "mode dark noswitch",
            wall_set,
            &["--mode", "dark", "--noswitch"],
        ),
        (
            "mode light noswitch",
            wall_set,
            &["--mode", "light", "--noswitch"],
        ),
        ("mode dark", wall_set, &["--mode", "dark"]),
        ("image", ":", &["--image", "@W@/syn_small.png"]),
        (
            "image mode",
            ":",
            &["--image", "@W@/duo-dark.png", "--mode", "light"],
        ),
        ("color", ":", &["--color", "#A52F48"]),
        ("color noswitch", ":", &["--noswitch", "--color", "4285F4"]),
        ("bare path", ":", &["@W@/syn_grad.jpg"]),
        ("bare gif", ":", &["@W@/syn_anim.gif"]),
        ("noswitch", wall_set, &["--noswitch"]),
        ("noswitch default", ":", &["--noswitch"]),
    ];
    for (tag, setup, args) in table {
        let a = sandbox(&root.join("a"), &bin, &venv, xdg, setup);
        let w = a.h.join("Pictures/Wallpapers").display().to_string();
        let args: Vec<String> = args.iter().map(|x| x.replace("@W@", &w)).collect();
        let mut want = original(&a, &args);
        let b = sandbox(&root.join("b"), &bin, &venv, xdg, setup);
        let wb = b.h.join("Pictures/Wallpapers").display().to_string();
        let args_b: Vec<String> = args.iter().map(|x| x.replace(&w, &wb)).collect();
        let mut got = native(&b, &args_b);
        got.calls = got.calls.iter().map(|c| c.replace(&wb, &w)).collect();
        // The script's KDE step is the fixture wrapper (it only logs, and only exists when
        // matugen installed it); the port recolours natively through `Sys::kde_recolor`.
        want.calls
            .retain(|c| !c.starts_with("kde-material-you-colors-wrapper.sh "));
        // The port kills mpvpaper by exact executable name and waits for it to exit.
        want.calls.retain(|c| c != "pkill -f -9 mpvpaper");
        let fix = |o: &mut Outcome, from: &Path| {
            let _ = from;
            o.files.retain(|(n, _)| !n.starts_with("pts/ptmx"));
        };
        fix(&mut want, &a.h);
        fix(&mut got, &b.h);
        assert!(
            !want.files.is_empty(),
            "{tag}: the script generated nothing"
        );
        let names = |o: &Outcome| o.files.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
        let logs = format!(
            "\n--- script stderr\n{}\n--- port stderr\n{}",
            want.stderr, got.stderr
        );
        assert_eq!(
            (got.code, &got.stdout),
            (want.code, &want.stdout),
            "{tag}: exit/stdout{logs}"
        );
        assert_eq!(names(&got), names(&want), "{tag}: generated file set{logs}");
        let scheme = |o: &Outcome| {
            o.files
                .iter()
                .find(|(n, _)| n.ends_with("scheme_type.txt"))
                .map(|(_, c)| String::from_utf8_lossy(c).trim().to_string())
        };
        match *tag {
            "noswitch keeps recorded scheme" => {
                assert_eq!(scheme(&got).as_deref(), Some("scheme-expressive"), "{tag}")
            }
            "type auto noswitch re-detects" => {
                assert_ne!(scheme(&got).as_deref(), Some("scheme-expressive"), "{tag}")
            }
            _ => {}
        }
        for ((n, x), (_, y)) in want.files.iter().zip(&got.files) {
            assert!(
                x == y,
                "{tag}: {n} differs:\n--- script\n{}\n--- port\n{}",
                String::from_utf8_lossy(x),
                String::from_utf8_lossy(y)
            );
        }
        assert_eq!(
            (got.code, &got.stdout, &got.calls),
            (want.code, &want.stdout, &want.calls),
            "{tag}: exit/stdout/leaf commands{logs}"
        );
    }
}

/// Only RI_EQUIV_SKIP=1 skips (`testenv`): another value still fails on a missing tool.
#[test]
fn missing_tools_fail_unless_skip_is_exactly_one() {
    testenv::assert_fails_when_missing(
        "switchwall_script_matches_native_in_home",
        &[
            ("PATH", Some("/ri-no-such-dir")),
            ("RI_EQUIV_SKIP", Some("yes")),
        ],
    );
}

#[test]
fn switchwall_script_matches_native_in_home() {
    cases(false);
}

#[test]
fn switchwall_script_matches_native_with_xdg_dirs() {
    cases(true);
}
