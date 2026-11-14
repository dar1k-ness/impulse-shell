//! `tools/ci/gate` reads its arguments before it starts anything: `--help` and an argument
//! the gate does not take print the usage and run no step, build no image and start no
//! container. (`tools/ci/gate --help` used to hand `--help` to nextest and then run every
//! other step of the gate.) The arguments a run does take reach the container as given, the
//! ones for nextest reach the tests step only, and the CI workflows' jobs are held to the same
//! check.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `tools/ci/gate ARGS` with a `docker` and a `git` first on PATH that only record that they
/// were started: the output, and what was started.
fn gate(args: &[&str]) -> (Output, String) {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("started");
    for tool in ["docker", "git"] {
        testenv::install_script(
            &dir.path().join(tool),
            &format!(
                "#!/bin/sh\necho {tool} \"$@\" >> '{}'\nexit 1\n",
                log.display()
            ),
        );
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let out = Command::new("sh")
        .arg(root().join("tools/ci/gate"))
        .args(args)
        .env("PATH", format!("{}:{path}", dir.path().display()))
        .env("RI_GATE_DIR", dir.path().join("cache"))
        .output()
        .unwrap();
    let started = std::fs::read_to_string(&log).unwrap_or_default();
    (out, started)
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn help_prints_the_usage_and_starts_nothing() {
    for args in [
        &["--help"][..],
        &["-h"],
        &["--workspace", "--help"],
        &["--only", "tests", "--help"],
    ] {
        let (out, started) = gate(args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        let usage = text(&out.stdout);
        assert!(usage.starts_with("usage: ri-gate "), "{args:?}: {usage}");
        assert!(
            usage.contains("steps: tests no-interp "),
            "{args:?}: {usage}"
        );
        assert_eq!(started, "", "{args:?}");
    }
}

#[test]
fn an_argument_the_gate_does_not_take_is_refused_and_starts_nothing() {
    for (args, why) in [
        (&["--nope"][..], "unknown argument '--nope'"),
        (&["--workspace", "--nope"], "unknown argument '--nope'"),
        // Arguments for nextest are taken only by a run narrowed to the tests step.
        (&["--workspace", "--lib"], "unknown argument '--lib'"),
        (&["--only", "clippy", "--lib"], "unknown argument '--lib'"),
        // The order counts: what stands before the gate's options is not for nextest.
        (
            &["-p", "shell", "--only", "tests"],
            "unknown argument '-p': the gate's options come first",
        ),
        (
            &[
                "--no-fail-fast",
                "--only",
                "tests",
                "--test",
                "keyboard_focus",
            ],
            "unknown argument '--no-fail-fast': the gate's options come first",
        ),
        // As the usage says: aot is a step of the --workspace gate only.
        (&["--only", "aot"], "unknown step 'aot'"),
        (&["--only", "nope"], "unknown step 'nope'"),
        (&["--skip=fmt,nope"], "unknown step 'nope'"),
        (&["--only"], "--only needs a list of steps"),
        (&["--workspace", "--skip"], "--skip needs a list of steps"),
        (
            &["--only", "fmt", "--skip", "fmt"],
            "no step is left to run",
        ),
        (&["--run"], "--run needs a command"),
    ] {
        let (out, started) = gate(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        let err = text(&out.stderr);
        assert!(err.contains(why), "{args:?}: {err}");
        assert_eq!(text(&out.stdout), "", "{args:?}");
        assert_eq!(started, "", "{args:?}");
    }
}

/// The arguments the workflows, the pre-push hook and docs/ci.md give still start a run: the
/// gate goes on to ask git for the checkout.
#[test]
fn the_arguments_in_use_still_start_a_run() {
    for args in [
        &["--workspace"][..],
        &["--workspace", "--only", "clippy,shipped-clippy"],
        &["--workspace", "--only", "tests", "--partition", "hash:1/4"],
        &[
            "--workspace",
            "--only",
            "tests,no-interp,shipped-build,shipped-tests",
            "--lib",
        ],
        &[
            "--workspace",
            "--skip",
            "tests,aot,clippy,shipped-clippy,shipped-build,shipped-tests",
        ],
        &["--workspace", "--only", "fmt,clippy,shipped-clippy,aot"],
        &["--only", "tests", "--test", "keyboard_focus"],
        &[
            "--only",
            "tests",
            "--no-fail-fast",
            "--test",
            "keyboard_focus",
        ],
        &["--only", "tests", "--workspace"],
        &[
            "--only",
            "tests",
            "-p",
            "qsg",
            "-E",
            "package(shell) | package(qsg)",
        ],
        &["--run", "bash"],
        &[
            "--run",
            "as-tester",
            "env",
            "RI_REQUIRE_GL=1",
            "cargo",
            "nextest",
            "run",
            "--help",
        ],
    ] {
        let (out, started) = gate(args);
        assert!(
            started.starts_with("git rev-parse --show-toplevel\n"),
            "{args:?}: {started:?} {out:?}"
        );
    }
}

/// The usage says what the gate does with its arguments: every step it names is one `--only`
/// takes (the list printed is the list the steps run from), and it says that the order counts.
#[test]
fn the_usage_names_the_steps_the_gate_takes() {
    let (out, _) = gate(&["--help"]);
    let usage = text(&out.stdout);
    assert!(usage.contains("The order counts: the options above come first"));
    assert!(usage.contains("(aot only with --workspace)"), "{usage}");
    let steps: Vec<&str> = usage
        .lines()
        .find_map(|l| l.strip_prefix("steps: "))
        .expect("the usage lists the steps")
        .split_whitespace()
        .collect();
    assert!(steps.len() >= 11 && steps.contains(&"aot"), "{steps:?}");
    for step in &steps {
        for option in ["--only", "--skip"] {
            let (out, started) = gate(&["--workspace", option, step]);
            assert!(
                started.starts_with("git rev-parse --show-toplevel\n"),
                "{option} {step}: {started:?} {out:?}"
            );
        }
        let (out, started) = gate(&["--only", step]);
        assert_eq!(
            started.starts_with("git rev-parse --show-toplevel\n"),
            *step != "aot",
            "--only {step}: {started:?} {out:?}"
        );
    }
}

/// A git repository of its own holding the gate's scripts, with a `docker` and an `as-tester`
/// first on PATH that only write down how they were called: a run goes all the way.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        for f in [
            "tools/ci/gate",
            "tools/ci/in-testenv",
            "tools/ci/image-tag",
            "tools/ci/skipped-tests",
            "tools/testenv/ri-gate",
        ] {
            let to = repo.join(f);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(root().join(f), &to).unwrap();
        }
        // ri-gate fetches the dependencies of this workspace too.
        std::fs::create_dir_all(repo.join("crates/rigen/check")).unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        for tool in ["docker", "as-tester"] {
            testenv::install_script(
                &dir.path().join("bin").join(tool),
                &format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
                    dir.path().join(tool).display()
                ),
            );
        }
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(init.status.success(), "git init: {init:?}");
        Sandbox { dir }
    }

    /// `script ARGS` in the repository.
    fn run(&self, script: &str, args: &[&str]) -> Output {
        let repo = self.dir.path().join("repo");
        let path = std::env::var("PATH").unwrap_or_default();
        Command::new("sh")
            .arg(repo.join(script))
            .args(args)
            .current_dir(&repo)
            .env(
                "PATH",
                format!("{}:{path}", self.dir.path().join("bin").display()),
            )
            .env("RI_GATE_DIR", self.dir.path().join("cache"))
            .output()
            .unwrap()
    }

    /// The calls of the stand-in `tool` so far, one argument list per line.
    fn calls(&self, tool: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(tool)).unwrap_or_default()
    }
}

#[test]
fn the_arguments_of_a_run_reach_the_container_as_given() {
    let sandbox = Sandbox::new();
    let args = [
        "--workspace",
        "--only",
        "tests,aot",
        "--lib",
        "--partition",
        "hash:1/4",
    ];
    let out = sandbox.run("tools/ci/gate", &args);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let calls = sandbox.calls("docker");
    assert!(calls.starts_with("image inspect ri-testenv:"), "{calls}");
    assert!(
        calls.ends_with(" sh ri-gate --workspace --only tests,aot --lib --partition hash:1/4\n"),
        "{calls}"
    );

    // `--run` takes any command, also one with a `--help` of its own.
    let out = sandbox.run("tools/ci/gate", &["--run", "as-tester", "cargo", "--help"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let calls = sandbox.calls("docker");
    assert!(calls.ends_with(" sh as-tester cargo --help\n"), "{calls}");
}

#[test]
fn nextest_arguments_go_to_the_tests_step_only() {
    let sandbox = Sandbox::new();
    let args = ["--only", "no-interp,tests", "--test", "keyboard_focus"];
    let out = sandbox.run("tools/testenv/ri-gate", &args);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let calls = sandbox.calls("as-tester");
    let runs: Vec<&str> = calls
        .lines()
        .filter(|l| l.contains("nextest run"))
        .collect();
    assert_eq!(runs.len(), 2, "{calls}");
    assert!(
        runs[0].ends_with("-p qml -p shell --features shell/interp --test keyboard_focus"),
        "{calls}"
    );
    assert!(
        runs[1].ends_with("-p qml --no-default-features --test no_interp"),
        "{calls}"
    );
    // The steps ran in the gate's order, whatever order --only names them in.
    let stdout = text(&out.stdout);
    let summary = stdout.split("==> ri-gate: summary\n").nth(1).unwrap();
    let steps: Vec<&str> = summary
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    assert_eq!(steps, ["tests", "no-interp"], "{stdout}");
}

/// The steps `ri-gate --workspace ARGS` runs, by the usage's list and ARGS' `--only` and
/// `--skip`.
fn steps_of(usage: &str, args: &str) -> Vec<String> {
    let list = |option: &str| -> Vec<&str> {
        let mut words = args.split_whitespace();
        match words.find(|w| *w == option) {
            Some(_) => words.next().unwrap().split(',').collect(),
            None => Vec::new(),
        }
    };
    let (only, skip) = (list("--only"), list("--skip"));
    let steps = usage.lines().find_map(|l| l.strip_prefix("steps: "));
    steps
        .expect("the usage lists the steps")
        .split_whitespace()
        .filter(|s| (only.is_empty() || only.contains(s)) && !skip.contains(s))
        .map(str::to_string)
        .collect()
}

/// Every job of the workflows (`{"name": ..., "args": ...}` in a caller of gate-jobs.yml,
/// which runs `ri-gate --workspace ARGS` from an empty or the job's own cache) names
/// arguments the gate takes, and one that runs shipped-tests also runs shipped-build, whose
/// installed binary shipped-tests needs. (The unit-tests job ran shipped-tests alone.)
#[test]
fn workflow_jobs_select_steps_that_can_pass() {
    let usage = text(&gate(&["--help"]).0.stdout);
    let mut jobs = 0;
    let mut whole = Vec::new();
    for workflow in ["checks.yml", "ri-gate.yml", "gpu-precision.yml"] {
        let path = root().join(".github/workflows").join(workflow);
        let yaml = std::fs::read_to_string(&path).unwrap();
        for job in yaml.split("{\"name\": \"").skip(1) {
            let name = job.split('"').next().unwrap();
            let args = job.split("\"args\": \"").nth(1).unwrap();
            let args = args.split('"').next().unwrap();
            let what = format!("{workflow} job {name}: ri-gate --workspace {args}");
            let mut list = vec!["--workspace"];
            list.extend(args.split_whitespace());
            let (out, started) = gate(&list);
            assert!(
                started.starts_with("git rev-parse --show-toplevel\n"),
                "{what}: {started:?} {out:?}"
            );
            let steps = steps_of(&usage, args);
            assert!(!steps.is_empty(), "{what}: no step");
            let runs = |step: &str| steps.iter().any(|s| s == step);
            assert!(
                !runs("shipped-tests") || runs("shipped-build"),
                "{what}: shipped-tests without shipped-build"
            );
            if workflow == "ri-gate.yml" {
                whole.extend(steps);
            }
            jobs += 1;
        }
    }
    assert_eq!(jobs, 13, "jobs read from the workflows");
    // ri-gate.yml is the whole gate: each step in exactly one job (the tests in four parts).
    whole.sort();
    let mut all = steps_of(&usage, "");
    all.extend(["tests"; 3].map(str::to_string));
    all.sort();
    assert_eq!(whole, all);
}
