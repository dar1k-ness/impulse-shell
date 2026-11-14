//! `qml::easyeffects` against what the original runs through `bash -c` (EasyEffects.qml and both
//! EasyEffectsToggle.qml). `easyeffects`/`flatpak` are stubs on PATH that log their argv and
//! environment; `pidof`/`pkill` are the real procps tools against live processes named
//! easyeffects. Native and bash must agree on stdout, stderr, status, the commands run (argv
//! and environment) and which processes get SIGTERM, except where the original's line is wrong and the
//! test asserts the intended result instead (see each test). A Flatpak'd EasyEffects is a
//! process in a sandbox like the one Flatpak makes (`testenv::flatpak_sandbox`). What bash itself adds (its `bash: line
//! 1:` diagnostics and the SHLVL/_/PWD/OLDPWD it exports) is left out: native runs no shell.
//! Skipped without `cc`.
//! Not named easyeffects*: pkill matches the 15-byte comm of the test binary.
use qml::easyeffects as ee;
use qml::runtime::Runtime;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use testenv::Running;

mod runtime_host;
use runtime_host::Host;

static LOCK: Mutex<()> = Mutex::new(());

// The bash lines the original's EasyEffects.qml ran, which the native functions replaced.
const ENABLE: &str = "easyeffects --hide-window --service-mode || flatpak run com.github.wwmm.easyeffects --hide-window --service-mode";
const DISABLE: &str = "pkill easyeffects || flatpak pkill com.github.wwmm.easyeffects";
const AVAILABLE: &str =
    "command -v easyeffects || flatpak info com.github.wwmm.easyeffects > /dev/null 2>&1";
const ACTIVE: &str =
    "pidof easyeffects || flatpak ps | grep com.github.wwmm.easyeffects > /dev/null 2>&1";
const OPEN: &str = "flatpak run com.github.wwmm.easyeffects || easyeffects";

type Probe = fn() -> bool;

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qml-ee-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const STUB: &str = r#"
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <unistd.h>
extern char **environ;
/* The case's answer for this stub, from $STUB_DIR/<name>.<what>; NULL when it has none. */
static char *answer(const char *name, const char *what) {
    char p[4096];
    snprintf(p, sizeof p, "%s/%s.%s", getenv("STUB_DIR"), name, what);
    FILE *f = fopen(p, "r");
    if (!f) return NULL;
    static char bufs[4][4096];
    static int used;
    char *b = bufs[used++ % 4];
    size_t n = fread(b, 1, sizeof bufs[0] - 1, f);
    b[n] = 0;
    fclose(f);
    return b;
}
int main(int argc, char **argv) {
    char exe[4096]; ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    exe[n < 0 ? 0 : n] = 0;
    const char *name = strrchr(exe, '/') ? strrchr(exe, '/') + 1 : exe;
    FILE *f = fopen(getenv("EE_LOG"), "a");
    /* Stubs run at once (`flatpak info` and `flatpak ps`): a record past stdio's buffer takes
       several writes, so each holds the log until fclose has written all of it. */
    flock(fileno(f), LOCK_EX);
    fprintf(f, "%s:", name);
    for (int i = 0; i < argc; i++) fprintf(f, " [%s]", argv[i]);
    fprintf(f, "\n");
    for (char **e = environ; *e; e++) fprintf(f, "  %s\n", *e);
    fprintf(f, "--\n");
    fclose(f);
    char *out = answer(name, "out"), *err = answer(name, "err");
    if (out) fputs(out, stdout);
    if (err) fputs(err, stderr);
    /* A case's $STUB_DIR/<name>.hold FIFO keeps the stub running until the test releases it
       (opens it for writing and closes it). */
    char hold[4096];
    snprintf(hold, sizeof hold, "%s/%s.hold", getenv("STUB_DIR"), name);
    int h = open(hold, O_RDONLY);
    if (h >= 0) {
        char c;
        while (read(h, &c, 1) > 0) {}
        close(h);
    }
    char *rc = answer(name, "exit");
    return rc ? atoi(rc) : 0;
}
"#;

/// A PATH with the stub `easyeffects`/`flatpak` (as asked) and the real tools bash needs.
struct Stubs {
    dir: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Stubs {
    fn at(dir: PathBuf) -> Stubs {
        Stubs {
            bin: dir.join("stub"),
            log: dir.join("log"),
            dir,
        }
    }
}

/// In `test`'s own process (see `testenv::run_in_own_process`): the stubs, which PATH, EE_LOG
/// and STUB_DIR (the case's answers, see `Stubs::setup`) name, as the environment native
/// passes on. `None` in the process that ran it there, or without `cc`.
fn stubs(test: &str) -> Option<Stubs> {
    if testenv::in_own_process(test) {
        return Some(Stubs::at(std::env::var_os("EE_STUBS").unwrap().into()));
    }
    if !testenv::require("cc") {
        return None;
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let s = Stubs::at(tmp.path().to_path_buf());
    let src = s.dir.join("stub.c");
    std::fs::write(&src, STUB).unwrap();
    let st = Command::new("cc").arg(&src).arg("-o").arg(&s.bin).status();
    assert!(st.as_ref().is_ok_and(|s| s.success()), "cc stub.c: {st:?}");
    let tools = s.dir.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    for t in ["bash", "pidof", "pkill", "grep", "sh", "cat"] {
        let p = ["/usr/bin", "/bin"]
            .iter()
            .map(|d| Path::new(d).join(t))
            .find(|p| p.exists())
            .unwrap();
        std::os::unix::fs::symlink(p, tools.join(t)).unwrap();
    }
    let path = format!("{}:{}", s.dir.join("path").display(), tools.display());
    testenv::run_in_own_process(
        test,
        [
            ("EE_STUBS", s.dir.clone().into_os_string()),
            ("PATH", path.into()),
            ("EE_LOG", s.log.clone().into_os_string()),
            ("STUB_DIR", s.dir.join("answers").into_os_string()),
            ("OLDPWD", "/old".into()),
        ],
    );
    None
}

/// Which stubs exist and how they behave.
#[derive(Clone, Debug)]
struct Case {
    ee: Option<bool>,
    ee_exit: i32,
    flatpak: Option<bool>,
    fp_exit: i32,
    ps_out: &'static str,
    shlvl: Option<&'static str>,
}

impl Stubs {
    /// The stubs `c` asks for in PATH's first directory and their answers; its SHLVL is
    /// bash's (`bash`), as native runs no shell.
    fn setup(&self, c: &Case) {
        let path = self.dir.join("path");
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        for (name, how) in [("easyeffects", c.ee), ("flatpak", c.flatpak)] {
            if let Some(exec) = how {
                let p = path.join(name);
                std::fs::copy(&self.bin, &p).unwrap();
                if !exec {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
                }
            }
        }
        let answers = self.dir.join("answers");
        let _ = std::fs::remove_dir_all(&answers);
        std::fs::create_dir_all(&answers).unwrap();
        for (f, v) in [
            ("easyeffects.exit", c.ee_exit.to_string()),
            ("easyeffects.out", "ee out\n".into()),
            ("flatpak.exit", c.fp_exit.to_string()),
            ("flatpak.out", c.ps_out.into()),
            ("flatpak.err", "flatpak says hi\n".into()),
        ] {
            std::fs::write(answers.join(f), v).unwrap();
        }
        let _ = std::fs::remove_file(&self.log);
    }

    /// The logged runs, each environment sorted, without the variables bash sets itself.
    fn runs(&self) -> Vec<String> {
        let s = std::fs::read_to_string(&self.log).unwrap_or_default();
        s.split("--\n")
            .filter(|r| !r.is_empty())
            .map(|r| {
                let mut l: Vec<&str> = r
                    .lines()
                    .enumerate()
                    .filter(|&(i, l)| {
                        i == 0
                            || !["SHLVL=", "_=", "PWD=", "OLDPWD="]
                                .iter()
                                .any(|k| l.trim_start().starts_with(k))
                    })
                    .map(|(_, l)| l)
                    .collect();
                l[1..].sort_unstable();
                l.join("\n")
            })
            .collect()
    }

    /// The runs once native's asynchronous commands are done: `a || b` and the quits run on
    /// the shared pool, whose jobs and reaped children `workpool::wait_idle` waits for, so no
    /// late run (a fallback still being spawned) can leak into the next case's log.
    fn settled_runs(&self) -> Vec<String> {
        workpool::wait_idle();
        self.runs()
    }
}

/// What `flatpak ps --columns=application` prints, plus rows the original's `grep` misreads.
const PS_OUT: &[&str] = &[
    "",
    "com.github.wwmm.easyeffects\n",
    "org.other.App\ncom.github.wwmm.easyeffects\n",
    "com.github.wwmm.easyeffect\n",
    "comXgithubYwwmm.easyeffects\n",
    "com.github.wwmm.easyeffects.Plugin\n",
    "123 4 com.github.wwmm.easyeffects\n",
];

/// Whether the original's `grep com.github.wwmm.easyeffects` matches the row (`.` is any character).
fn grep_matches(row: &str) -> bool {
    let (r, p): (Vec<char>, Vec<char>) = (row.chars().collect(), ee::APP.chars().collect());
    r.windows(p.len())
        .any(|w| w.iter().zip(&p).all(|(a, b)| *b == '.' || a == b))
}

fn cases() -> Vec<Case> {
    let base = Case {
        ee: None,
        ee_exit: 0,
        flatpak: None,
        fp_exit: 0,
        ps_out: "",
        shlvl: Some("2"),
    };
    let mut v = vec![];
    for ee in [None, Some(true), Some(false)] {
        for ee_exit in [0, 3] {
            for flatpak in [None, Some(true), Some(false)] {
                for fp_exit in [0, 1] {
                    for &ps_out in PS_OUT {
                        v.push(Case {
                            ee,
                            ee_exit,
                            flatpak,
                            fp_exit,
                            ps_out,
                            ..base.clone()
                        });
                    }
                }
            }
        }
    }
    for shlvl in [
        None,
        Some("-3"),
        Some(" 7 "),
        Some("x1"),
        Some("998"),
        Some("999"),
    ] {
        v.push(Case {
            ee: Some(true),
            ee_exit: 1,
            flatpak: Some(true),
            shlvl,
            ..base.clone()
        });
    }
    v
}

/// The script's status, stdout and stderr (run with SHLVL `shlvl`) without bash's own `bash: line 1:` diagnostics.
fn bash(script: &str, shlvl: Option<&str>) -> (i32, String, String) {
    let mut cmd = Command::new("/bin/bash");
    cmd.args(["-c", script]).stdin(Stdio::null());
    match shlvl {
        Some(l) => cmd.env("SHLVL", l),
        None => cmd.env_remove("SHLVL"),
    };
    let o = cmd.output().unwrap();
    let err: String = String::from_utf8_lossy(&o.stderr)
        .split_inclusive('\n')
        .filter(|l| !l.starts_with("bash: line 1: "))
        .collect();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        err,
    )
}

/// `available`/`active` give the intended answer and run only the flatpak query they need,
/// in the shell's environment. bash agrees except where
/// the original's line is wrong: `command -v` also reports a non-executable `easyeffects`, and with no
/// easyeffects process the original believes `flatpak ps` (here a stub listing the app, rows that only
/// look like its id included) where native, which finds the sandboxed app with `pidof`
/// (`sandboxed_app_is_found_and_quit_without_flatpak`), runs no `flatpak` at all.
#[test]
fn probes_give_intended_result() {
    let Some(s) = stubs(concat!(module_path!(), "::probes_give_intended_result")) else {
        return;
    };
    let first = |runs: &[String]| -> Vec<String> {
        runs.iter()
            .map(|r| r.lines().next().unwrap().to_string())
            .collect()
    };
    let env = |runs: &[String]| -> Vec<String> {
        runs.iter()
            .map(|r| r.split_once('\n').map_or("", |x| x.1).to_string())
            .collect()
    };
    for c in cases() {
        let fp = c.flatpak == Some(true);
        let installed = c.ee == Some(true) || fp && c.fp_exit == 0;
        let listed = fp && c.ps_out.lines().any(|l| l == ee::APP);
        for (script, f, want, flaw, queries) in [
            (
                AVAILABLE,
                ee::available as Probe,
                installed,
                c.ee == Some(false) && !installed,
                if c.ee != Some(true) && fp {
                    vec![format!("flatpak: [flatpak] [info] [{}]", ee::APP)]
                } else {
                    vec![]
                },
            ),
            (
                ACTIVE,
                ee::active,
                false,
                fp && (listed || c.ps_out.lines().any(grep_matches)),
                vec![],
            ),
        ] {
            s.setup(&c);
            let bash_got = bash(script, c.shlvl);
            let bash_runs = s.runs();
            s.setup(&c);
            let got = f();
            let runs = s.runs();
            assert_eq!(got, want, "{script} {c:?}");
            assert_eq!(bash_got.0 == 0, want != flaw, "bash {script} {c:?}");
            assert_eq!(first(&runs), queries, "{script} {c:?}");
            if !runs.is_empty() && !bash_runs.is_empty() {
                assert_eq!(env(&runs), env(&bash_runs), "environment {script} {c:?}");
            }
        }
    }
}

#[test]
fn detached_commands_match_bash() {
    let Some(s) = stubs(concat!(module_path!(), "::detached_commands_match_bash")) else {
        return;
    };
    for (script, f) in [(ENABLE, (|| ee::enable(None)) as fn()), (OPEN, ee::open)] {
        for c in cases().into_iter().filter(|c| c.ps_out.is_empty()) {
            s.setup(&c);
            bash(script, c.shlvl);
            let want = s.runs();
            s.setup(&c);
            f();
            assert_eq!(s.settled_runs(), want, "{script} {c:?}");
        }
    }
}

/// Children of this process whose executable is this test binary: a forked copy of it.
fn forked_copies() -> Vec<i32> {
    let me = std::process::id() as i32;
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let ppid = stat
                .rsplit(')')
                .next()
                .and_then(|r| r.split_whitespace().nth(1)?.parse::<i32>().ok());
            ppid == Some(me)
                && std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p| p == exe)
        })
        .collect()
}

/// While `easyeffects --service-mode` (the first command of `a || b`) runs, nothing waits on
/// it in a copy of this process holding its memory and fds; `b` still starts once it fails.
#[test]
fn enable_forks_no_copy_of_the_shell() {
    let Some(s) = stubs(concat!(
        module_path!(),
        "::enable_forks_no_copy_of_the_shell"
    )) else {
        return;
    };
    s.setup(&Case {
        ee: Some(true),
        ee_exit: 5,
        flatpak: Some(true),
        fp_exit: 0,
        ps_out: "",
        shlvl: Some("1"),
    });
    let hold = s.dir.join("answers/easyeffects.hold");
    testenv::fifo(&hold);
    let log = testenv::DirWatch::new(&s.dir);
    ee::enable(None);
    log.until(|| std::fs::read_to_string(&s.log).is_ok_and(|l| l.contains("--\n")));
    assert!(
        qml::osutil::pidof("easyeffects").len() == 1,
        "easyeffects is running"
    );
    assert_eq!(forked_copies(), Vec::<i32>::new());
    testenv::release_fifo(&hold);
    let runs = s.settled_runs();
    let argv: Vec<&str> = runs.iter().map(|r| r.lines().next().unwrap()).collect();
    assert_eq!(
        argv,
        [
            "easyeffects: [easyeffects] [--hide-window] [--service-mode]",
            "flatpak: [flatpak] [run] [com.github.wwmm.easyeffects] [--hide-window] [--service-mode]",
        ]
    );
}

/// `cat` running as `argv0` (bash's `exec -a`), once it runs.
fn named(argv0: &str) -> Running {
    Running::spawn(Command::new(testenv::tool_path("cat")).arg0(argv0))
}

/// Live processes pidof/pkill tell apart: comm, argv[0] (also as a login shell `-name`),
/// the exe link, and an argv[0] with a space. Each runs (pidof sees it) when this returns.
fn spawn_targets(d: &Path) -> Vec<(&'static str, Running)> {
    let fake = testenv::cat_copy(&d.join("easyeffects"));
    let decoy = testenv::cat_copy(&d.join("easyeffects-x"));
    vec![
        ("comm", Running::spawn(&mut Command::new(&fake))),
        ("argv0", named("easyeffects")),
        ("login", named("-easyeffects")),
        ("exe", Running::spawn(Command::new(&fake).arg0("foo"))),
        ("space", named("easyeffects x")),
        ("decoy", named("easyeffectsX")),
        ("commdecoy", Running::spawn(&mut Command::new(&decoy))),
    ]
}

/// The signal that ended each target, `None` for one still running. Called once whatever
/// signals them has returned (see `Running::alive`).
fn reap(v: Vec<(&'static str, Running)>) -> Vec<(&'static str, Option<i32>)> {
    v.into_iter()
        .map(|(n, mut c)| {
            use std::os::unix::process::ExitStatusExt;
            (n, c.alive().err().and_then(|s| s.signal()))
        })
        .collect()
}

#[test]
fn process_probes_match_procps() {
    let Some(s) = stubs(concat!(module_path!(), "::process_probes_match_procps")) else {
        return;
    };
    let c = Case {
        ee: None,
        ee_exit: 0,
        flatpak: Some(true),
        fp_exit: 0,
        ps_out: "",
        shlvl: Some("1"),
    };
    s.setup(&c);
    let t = spawn_targets(&s.dir);
    let want = bash(ACTIVE, c.shlvl);
    assert!(ee::active());
    assert_eq!(want.0, 0);
    assert_eq!(want.1.split(' ').count(), 4, "{want:?}");
    assert!(s.runs().is_empty());
    reap(t);

    let killed = |v: Vec<(&'static str, Option<i32>)>| -> Vec<&'static str> {
        v.into_iter()
            .filter(|(_, st)| *st == Some(15))
            .map(|(n, _)| n)
            .collect()
    };
    // The original's `pkill easyeffects` signals every comm containing the name, an unrelated
    // `easyeffects-x` included, and misses the argv[0] matches its own `pidof` counts active.
    s.setup(&c);
    let t = spawn_targets(&s.dir);
    bash(DISABLE, c.shlvl);
    assert_eq!(killed(reap(t)), ["comm", "exe", "commdecoy"]);
    assert!(s.runs().is_empty());
    // Native signals nothing: the host install quits through its own `easyeffects -q`, run
    // once for all the processes the active probe reports running.
    let c = Case {
        ee: Some(true),
        ..c
    };
    s.setup(&c);
    let t = spawn_targets(&s.dir);
    let active = ee::instances();
    assert_eq!(active.len(), 4);
    assert!(active.iter().all(|&(_, i)| i == ee::Install::Host));
    ee::disable(None);
    let runs = s.settled_runs();
    assert_eq!(killed(reap(t)), Vec::<&str>::new());
    assert_eq!(first_lines(&runs), ["easyeffects: [easyeffects] [-q]"]);
}

fn first_lines(runs: &[String]) -> Vec<&str> {
    let mut v: Vec<&str> = runs.iter().map(|r| r.lines().next().unwrap()).collect();
    v.sort_unstable();
    v
}

/// `exec -a easyeffects cat` in a sandbox naming `app` in its `/.flatpak-info`, once it runs
/// (so `pidof` sees it).
fn sandboxed(d: &Path, app: &str) -> Option<Running> {
    let root = d.join(format!("root-{app}"));
    std::fs::create_dir_all(&root).unwrap();
    let before = qml::osutil::pidof("easyeffects");
    let ch = testenv::flatpak_sandbox_running(&root, app, "easyeffects")?;
    let new: Vec<i32> = qml::osutil::pidof("easyeffects")
        .into_iter()
        .filter(|p| !before.contains(p))
        .collect();
    assert!(
        new.iter()
            .any(|&p| ee::flatpak_app(p).as_deref() == Some(app)),
        "sandboxed easyeffects for {app} not found: {new:?}"
    );
    Some(ch)
}

/// The Flatpak'd app runs in its own PID namespace, yet is a process `pidof` lists from the
/// host, and its `/.flatpak-info` names it: `active` answers without running `flatpak`, and
/// `disable` quits it with `flatpak run APP -q` (a host install alongside with `easyeffects
/// -q`), signalling no process. A process named easyeffects in another app's sandbox is no
/// EasyEffects; with none running, `disable` runs nothing, where the original's `flatpak pkill` would
/// kill whatever runs in the app's sandbox.
#[test]
fn sandboxed_app_is_found_and_quit_without_flatpak() {
    let Some(s) = stubs(concat!(
        module_path!(),
        "::sandboxed_app_is_found_and_quit_without_flatpak"
    )) else {
        return;
    };
    let c = Case {
        ee: Some(true),
        ee_exit: 0,
        flatpak: Some(true),
        fp_exit: 0,
        ps_out: "com.github.wwmm.easyeffects\n",
        shlvl: Some("1"),
    };
    let alive = |c: &mut Running| c.alive().is_ok();

    s.setup(&c);
    assert!(!ee::active());
    ee::disable(None);
    assert!(s.settled_runs().is_empty(), "{:?}", s.runs());

    let Some(mut other) = sandboxed(&s.dir, "org.example.Other") else {
        return;
    };
    s.setup(&c);
    assert!(!ee::active());
    ee::disable(None);
    assert!(s.settled_runs().is_empty(), "{:?}", s.runs());

    let mut app = sandboxed(&s.dir, ee::APP).unwrap();
    s.setup(&c);
    let pids = ee::instances();
    assert_eq!(pids.len(), 1);
    assert_eq!(pids[0].1, ee::Install::Flatpak);
    assert!(ee::active());
    assert!(s.runs().is_empty(), "{:?}", s.runs());
    ee::disable(None);
    let runs = s.settled_runs();
    assert_eq!(
        first_lines(&runs),
        ["flatpak: [flatpak] [run] [com.github.wwmm.easyeffects] [-q]"]
    );

    let host = named("easyeffects");
    assert_eq!(ee::instances().len(), 2);
    s.setup(&c);
    ee::disable(None);
    let runs = s.settled_runs();
    assert_eq!(
        first_lines(&runs),
        [
            "easyeffects: [easyeffects] [-q]",
            "flatpak: [flatpak] [run] [com.github.wwmm.easyeffects] [-q]",
        ]
    );
    assert!(alive(&mut app) && alive(&mut other), "nothing is signalled");
    host.kill();
    app.kill();
    other.kill();
}

/// A quit that fails leaves EasyEffects running: native re-probes and turns the toggle back
/// on, where the original leaves it off.
#[test]
fn failed_quit_turns_the_toggle_back_on() {
    let Some(s) = stubs(concat!(
        module_path!(),
        "::failed_quit_turns_the_toggle_back_on"
    )) else {
        return;
    };
    let tree = qml_tree();
    s.setup(&Case {
        ee: Some(true),
        ee_exit: 1,
        flatpak: None,
        fp_exit: 0,
        ps_out: "",
        shlvl: Some("1"),
    });
    let host = named("easyeffects");
    let (mut rt, root) = runtime(&tree, "classic.qml");
    let mut lp = Host::new(&mut rt);
    let ac = |rt: &mut Runtime| rt.get(root, "ac").truthy();
    lp.until(&mut rt, ac);
    rt.call_method(root, "dis", vec![]).unwrap().unwrap();
    rt.settle();
    assert!(!ac(&mut rt));
    // Native re-probes once the quit failed; a hang here is that turning back on missing.
    lp.until(&mut rt, ac);
    assert_eq!(
        first_lines(&s.settled_runs()),
        ["easyeffects: [easyeffects] [-q]"]
    );
    host.kill();
}

/// When neither `easyeffects` nor the flatpak starts, `enable` asks again once both failed
/// and the toggle turns back off (the original's bash line left it on).
#[test]
fn failed_enable_turns_the_toggle_back_off() {
    let Some(s) = stubs(concat!(
        module_path!(),
        "::failed_enable_turns_the_toggle_back_off"
    )) else {
        return;
    };
    let tree = qml_tree();
    let c = Case {
        ee: Some(true),
        ee_exit: 7,
        flatpak: Some(true),
        fp_exit: 1,
        ps_out: "",
        shlvl: Some("1"),
    };
    s.setup(&c);
    let (mut rt, root) = runtime(&tree, "classic.qml");
    let mut lp = Host::new(&mut rt);
    lp.settle(&mut rt);
    assert!(!rt.get(root, "ac").truthy());
    rt.call_method(root, "en", vec![]).unwrap().unwrap();
    rt.settle();
    assert!(rt.get(root, "ac").truthy());
    // Both commands failed and the second look (pool work too) has woken the loop.
    let _ = s.settled_runs();
    lp.settle(&mut rt);
    assert!(!rt.get(root, "ac").truthy(), "active after a failed enable");
}

/// A tree with both toggles verbatim and stubs for what they use.
fn qml_tree() -> PathBuf {
    let t = tmp("tree");
    let w = |p: &str, s: &str| {
        let p = t.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    for f in [
        "modules/rusty/sidebarRight/quickToggles/classicStyle/EasyEffectsToggle.qml",
        "modules/common/models/quickToggles/EasyEffectsToggle.qml",
    ] {
        w(f, &std::fs::read_to_string(ri().join(f)).unwrap());
    }
    w(
        "GlobalStates.qml",
        "pragma Singleton\nimport Quickshell\nSingleton { property bool sidebarRightOpen: true }\n",
    );
    w(
        "services/Translation.qml",
        "pragma Singleton\nimport Quickshell\nSingleton { function tr(s) { return s } }\n",
    );
    w("modules/common/Dummy.qml", "import QtQuick\nItem {}\n");
    w(
        "modules/common/functions/Dummy.qml",
        "import QtQuick\nItem {}\n",
    );
    w(
        "modules/common/widgets/StyledToolTip.qml",
        "import QtQuick\nItem { property string text }\n",
    );
    w(
        "modules/rusty/sidebarRight/quickToggles/classicStyle/QuickToggleButton.qml",
        "import QtQuick\nItem { property bool toggled; property string buttonIcon; \
         property var altAction; signal clicked() }\n",
    );
    w(
        "modules/common/models/quickToggles/QuickToggleModel.qml",
        "import QtQuick\nQtObject { property string name; property bool available; \
         property bool toggled; property string icon; property var mainAction; \
         property var altAction; property string tooltipText }\n",
    );
    let fns = "property bool open: GlobalStates.sidebarRightOpen\n\
               property bool av: EasyEffects.available\n\
               property bool ac: EasyEffects.active\n\
               function alt() { t.altAction() }\n\
               function reopen() { GlobalStates.sidebarRightOpen = true }\n\
               function en() { EasyEffects.enable() }\n\
               function dis() { EasyEffects.disable() }\n\
               function tog() { EasyEffects.toggle() }\n";
    w(
        "classic.qml",
        &format!(
            "import QtQuick\nimport ri\nimport ri.services\n\
             import \"modules/rusty/sidebarRight/quickToggles/classicStyle\"\n\
             Item {{ EasyEffectsToggle {{ id: t }}\n{fns}}}\n"
        ),
    );
    w(
        "model.qml",
        &format!(
            "import QtQuick\nimport ri\nimport ri.services\n\
             import ri.modules.common.models.quickToggles\n\
             Item {{ property var m: EasyEffectsToggle {{ id: t }}\n{fns}}}\n"
        ),
    );
    t
}

fn runtime(tree: &Path, file: &str) -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(tree.to_path_buf()));
    rt.load_singletons(&[tree.to_path_buf(), tree.join("services")]);
    rt.settle();
    let root = rt.load_file(&tree.join(file)).unwrap();
    rt.settle();
    (rt, root)
}

/// The singleton sets `available`/`active` for a host install, a running Flatpak'd app (in a
/// sandbox) and neither, without ever running `flatpak ps`, and the toggle's right click
/// opens the app (the Flatpak first) and closes the sidebar.
#[test]
fn singleton_state_and_alt_action() {
    let Some(s) = stubs(concat!(module_path!(), "::singleton_state_and_alt_action")) else {
        return;
    };
    let tree = qml_tree();
    for (ee, fp_exit, ps_out, want) in [
        (Some(true), 0, "", (true, false)),
        (None, 0, "com.github.wwmm.easyeffects\n", (true, true)),
        (None, 1, "", (false, false)),
    ] {
        let app = if ps_out.is_empty() {
            None
        } else {
            let Some(app) = sandboxed(&s.dir, ee::APP) else {
                return;
            };
            Some(app)
        };
        let c = Case {
            ee,
            ee_exit: 7,
            flatpak: Some(true),
            fp_exit,
            ps_out,
            shlvl: Some("1"),
        };
        s.setup(&c);
        let (mut rt, root) = runtime(&tree, "classic.qml");
        let mut lp = Host::new(&mut rt);
        // Everything the singleton and the toggle started has ended and set its state.
        lp.idle(&mut rt);
        let b = |rt: &mut Runtime, p| rt.get(root, p).truthy();
        assert_eq!((b(&mut rt, "av"), b(&mut rt, "ac")), want, "{c:?}");
        let lines = |runs: Vec<String>| -> Vec<String> {
            runs.iter()
                .map(|r| r.lines().next().unwrap().to_string())
                .collect()
        };
        let probes = lines(s.runs());
        assert!(
            !probes
                .iter()
                .any(|l| l.starts_with("flatpak: [flatpak] [ps]")),
            "{probes:?}"
        );
        let _ = std::fs::remove_file(&s.log);
        rt.call_method(root, "alt", vec![]).unwrap().unwrap();
        rt.settle();
        let alt = lines(s.settled_runs());
        assert_eq!(
            alt.first().map(String::as_str),
            Some("flatpak: [flatpak] [run] [com.github.wwmm.easyeffects]"),
            "{c:?} {alt:?}"
        );
        assert!(!b(&mut rt, "open"), "the sidebar closes");
        if let Some(app) = app {
            app.kill();
        }
    }
}
