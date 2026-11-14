//! `bench/vm.sh copy` (the first step of `sync`) puts the checkout in the guest with the file
//! times it has on the host and keeps the guest's `target`. A checkout older than the one
//! the guest built last (an older commit, a file removed from a directory a build script
//! lists) must still be built as a clean target dir builds it: `copy` ends with
//! `tools/ci/tree-times` in the guest (`gate_tree_times.rs`). Here the guest is a directory:
//! `ssh` is a script that runs the command with that directory as `$HOME`.
//!
//! `bench/vm.sh create` prepares what the stress scenarios run after it has booted the guest
//! again: `the_scenarios_tools_are_prepared_after_the_reboot`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// A package whose program prints a constant of a module, the text of an included file and
/// the names in a directory that its build script lists.
const COMMON: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n",
    ),
    (
        "build.rs",
        r#"fn main() {
    println!("cargo:rerun-if-changed=list");
    let mut names: Vec<String> = std::fs::read_dir("list")
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("list.rs");
    std::fs::write(out, format!("const LIST: &str = {:?};", names.join(","))).unwrap();
}
"#,
    ),
    (
        "src/main.rs",
        r#"mod part;
include!(concat!(env!("OUT_DIR"), "/list.rs"));
fn main() {
    println!("{} {} {LIST}", part::NAME, include_str!("../value.txt").trim());
}
"#,
    ),
    ("list/one", ""),
];

/// Tree A, and tree B: a source file and an included file changed, a listed file removed, a
/// listed file added.
const A: &[(&str, &str)] = &[
    ("src/part.rs", "pub const NAME: &str = \"part-a\";\n"),
    ("value.txt", "value-a\n"),
    ("list/two", ""),
];
const B: &[(&str, &str)] = &[
    ("src/part.rs", "pub const NAME: &str = \"part-b\";\n"),
    ("value.txt", "value-b\n"),
    ("list/three", ""),
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Every file and directory under `dir`, and `dir`, gets the modification time `t`.
fn set_times(dir: &Path, t: SystemTime) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            set_times(&path, t);
        } else {
            std::fs::File::open(&path).unwrap().set_modified(t).unwrap();
        }
    }
    std::fs::File::open(dir).unwrap().set_modified(t).unwrap();
}

/// A checkout at `dir` in place of what is there: the package `COMMON` + `files`, and of
/// this repository `bench/vm.sh`, `tools/build.sh` (which tells vm.sh the commit the guest's
/// build names) and `tools/ci/tree-times`. Everything is `days` days old.
fn put(dir: &Path, files: &[(&str, &str)], days: u64) {
    let _ = std::fs::remove_dir_all(dir);
    for (name, text) in COMMON.iter().chain(files) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for name in ["bench/vm.sh", "tools/build.sh", "tools/ci/tree-times"] {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(repo().join(name), path).unwrap();
    }
    set_times(dir, SystemTime::now() - Duration::from_secs(days * 86400));
}

/// git in a checkout at the commit `checkout` with no file changed: the top of the work tree
/// (`rev-parse --show-prefix` prints nothing there), a `status` that lists nothing.
const GIT: &str = "#!/bin/bash\ncase \"$*\" in\n*--show-prefix* | *status*) ;;\n\
                   *) echo checkout ;;\nesac\n";

struct Bench {
    tmp: tempfile::TempDir,
}

impl Bench {
    /// A host with a guest that "runs": `ssh` and `git` are scripts, the guest's home is a
    /// directory.
    fn new() -> Bench {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        for d in ["bin", "vm", "guest"] {
            std::fs::create_dir(dir.join(d)).unwrap();
        }
        // vm.sh: `ssh OPTIONS dar1k@127.0.0.1 COMMAND`.
        script(
            &dir.join("bin/ssh"),
            "#!/bin/bash\nwhile [ \"$1\" != dar1k@127.0.0.1 ]; do shift; done; shift\n\
             HOME=$GUEST_HOME exec bash -c \"$*\"\n",
        );
        script(&dir.join("bin/git"), GIT);
        // The guest is up for vm.sh while the process named in qemu.pid lives.
        std::fs::write(dir.join("vm/qemu.pid"), std::process::id().to_string()).unwrap();
        Bench { tmp }
    }

    fn checkout(&self) -> PathBuf {
        self.tmp.path().join("checkout")
    }

    /// `bench/vm.sh copy` of the checkout.
    fn copy(&self) {
        let dir = self.tmp.path();
        let path = std::env::var("PATH").unwrap();
        let out = Command::new("bash")
            .arg(self.checkout().join("bench/vm.sh"))
            .arg("copy")
            .env("PATH", format!("{}:{path}", dir.join("bin").display()))
            .env("VM_DIR", dir.join("vm"))
            .env("GUEST_HOME", dir.join("guest"))
            .env_remove("RI_COMMIT")
            .output()
            .expect("bash");
        assert!(out.status.success(), "vm.sh copy: {out:?}");
        // The commit the guest's build names (tools/build.sh there reads it).
        let named = std::fs::read_to_string(dir.join("guest/rusty-impulse/.build-commit"));
        assert_eq!(named.unwrap(), "checkout\n");
    }

    /// Builds guest `~/rusty-impulse` into its `target`, as `tools/install.sh` does there, and
    /// returns what the program prints.
    fn build_and_run(&self) -> String {
        build_and_run(&self.tmp.path().join("guest/rusty-impulse"))
    }
}

/// Builds the package at `tree` into `tree/target` and returns what its program prints.
fn build_and_run(tree: &Path) -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["run", "--quiet", "--offline"])
        .current_dir(tree)
        .env("CARGO_TARGET_DIR", tree.join("target"));
    // What the cargo that runs this test set for its own build.
    for var in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("cargo");
    assert!(out.status.success(), "cargo run: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// What a clean target dir builds from tree B.
fn clean_b() -> String {
    let tmp = tempfile::tempdir().unwrap();
    put(tmp.path(), B, 200);
    build_and_run(tmp.path())
}

/// Tree A copied and built; then tree B, every file of it older than A's: a file changed, an
/// included file changed, a listed file removed and one added.
#[test]
fn an_older_checkout_copied_to_the_guest_is_built_as_a_clean_target_dir_builds_it() {
    let clean = clean_b();
    assert_eq!(clean, "part-b value-b one,three\n");
    let bench = Bench::new();
    put(&bench.checkout(), A, 100);
    bench.copy();
    assert_eq!(bench.build_and_run(), "part-a value-a one,two\n");
    put(&bench.checkout(), B, 200);
    bench.copy();
    assert_eq!(bench.build_and_run(), clean);
    // What the checkout no longer has is gone from the guest; its target dir is kept.
    let guest = bench.tmp.path().join("guest/rusty-impulse");
    assert!(!guest.join("list/two").exists());
    assert!(guest.join("target/debug").is_dir());
}

/// The same checkout copied again: nothing gets a new time, so nothing is built again.
#[test]
fn a_checkout_copied_twice_keeps_its_times_in_the_guest() {
    let bench = Bench::new();
    put(&bench.checkout(), A, 100);
    bench.copy();
    let guest = bench.tmp.path().join("guest/rusty-impulse");
    let files = ["src/part.rs", "value.txt", "list", "build.rs"];
    let times = |()| {
        files.map(|f| {
            std::fs::metadata(guest.join(f))
                .unwrap()
                .modified()
                .unwrap()
        })
    };
    let first = times(());
    bench.copy();
    assert_eq!(times(()), first);
}

/// `bench/vm.sh create` runs `tools/parity/provision.sh` in the guest after the reboot that
/// follows `guest-setup.sh`. Before it, the guest runs the kernel of the image, whose modules
/// the upgrade has removed: `modprobe uinput` of `provision.sh` failed and `create` ended there.
/// Here every program `create` calls is a script, and `ssh` writes down what the guest was asked.
#[test]
fn the_scenarios_tools_are_prepared_after_the_reboot() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    for d in ["bin", "checkout/bench", "checkout/tools/testenv"] {
        std::fs::create_dir_all(dir.join(d)).unwrap();
    }
    for name in [
        "bench/vm.sh",
        "tools/build.sh",
        "tools/testenv/Containerfile",
    ] {
        std::fs::copy(repo().join(name), dir.join("checkout").join(name)).unwrap();
    }
    let log = dir.join("log");
    let bin = dir.join("bin");
    // The power-off ends qemu: its pid file names no process any more.
    script(
        &bin.join("ssh"),
        r#"#!/bin/bash
while [ "$1" != dar1k@127.0.0.1 ]; do shift; done; shift
echo "guest $*" >>"$LOG"
case "$*" in *poweroff*) : >"$VM_DIR/qemu.pid" ;; esac
cat >/dev/null
"#,
    );
    // The guest is up for vm.sh while the process named in qemu.pid lives.
    script(
        &bin.join("qemu-system-x86_64"),
        "#!/bin/bash\necho boot >>\"$LOG\"\necho \"$TEST_PID\" >\"$VM_DIR/qemu.pid\"\n",
    );
    script(
        &bin.join("curl"),
        "#!/bin/bash\nwhile [ \"$1\" != -o ]; do shift; done\n: >\"$2\"\n",
    );
    script(
        &bin.join("ssh-keygen"),
        "#!/bin/bash\nwhile [ \"$1\" != -f ]; do shift; done\n: >\"$2\"; echo key >\"$2.pub\"\n",
    );
    script(&bin.join("git"), GIT);
    // `sha256sum -c -` reads the sum vm.sh pipes to it: one that exits with the sum unread
    // ends the `echo` writing it with SIGPIPE, and `create`, under pipefail, with 141.
    script(&bin.join("sha256sum"), "#!/bin/bash\ncat >/dev/null\n");
    for name in ["qemu-img", "cloud-localds", "tail"] {
        script(&bin.join(name), "#!/bin/bash\nexit 0\n");
    }
    let path = std::env::var("PATH").unwrap();
    let out = Command::new("bash")
        .arg(dir.join("checkout/bench/vm.sh"))
        .arg("create")
        .env("PATH", format!("{}:{path}", bin.display()))
        .env("VM_DIR", dir.join("vm"))
        .env("LOG", &log)
        .env("TEST_PID", std::process::id().to_string())
        .env_remove("RI_COMMIT")
        .output()
        .expect("bash");
    assert!(out.status.success(), "vm.sh create: {out:?}");
    let log = std::fs::read_to_string(log).unwrap();
    let steps: Vec<&str> = log
        .lines()
        .filter_map(|l| {
            ["boot", "guest-setup.sh", "poweroff", "provision.sh"]
                .into_iter()
                .find(|step| l.contains(step))
        })
        .collect();
    assert_eq!(
        steps,
        ["boot", "guest-setup.sh", "poweroff", "boot", "provision.sh"],
        "{log}"
    );
    // The scenarios' own setup comes with it, in the session of the second boot.
    let tools = log.lines().find(|l| l.contains("provision.sh")).unwrap();
    assert!(tools.contains("tools/stress/setup.sh"), "{log}");
    let session = log.lines().position(|l| l.contains("hyprland.lock"));
    let tools = log.lines().position(|l| l.contains("provision.sh"));
    assert!(session < tools, "{log}");
}
