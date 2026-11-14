//! The gate builds every checkout into one target dir, and cargo knows a changed file by its
//! modification time alone. `tools/ci/tree-times`, which `tools/ci/in-testenv` runs on the
//! checkout before a command, gives what differs from the tree built last a new time: a tree
//! put at the same path with older times (a second worktree mounted at `/src`) is then built
//! as a clean target dir would build it. (Without it the tests of such a tree ran the code of
//! the tree built before.)

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

/// Tree A, and tree B: a source file and an included file changed, a listed file removed.
const A: &[(&str, &str)] = &[
    ("src/part.rs", "pub const NAME: &str = \"part-a\";\n"),
    ("value.txt", "value-a\n"),
    ("list/two", ""),
];
const B: &[(&str, &str)] = &[
    ("src/part.rs", "pub const NAME: &str = \"part-b\";\n"),
    ("value.txt", "value-b\n"),
];
const B_PRINTS: &str = "part-b value-b one\n";

/// Days before now: every time a tree is given is older than the builds of the test.
fn days_ago(n: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(n * 86400)
}

fn set_time(path: &Path, t: SystemTime) {
    std::fs::File::open(path)
        .and_then(|f| f.set_modified(t))
        .unwrap_or_else(|e| panic!("set the time of {}: {e}", path.display()));
}

fn time_of(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

/// Puts the tree `COMMON` + `files` at `dir` in place of what is there, every file and
/// directory with the modification time `t`.
fn put(dir: &Path, files: &[(&str, &str)], t: SystemTime) {
    let _ = std::fs::remove_dir_all(dir);
    let mut made = vec![dir.to_path_buf()];
    for (name, text) in COMMON.iter().chain(files) {
        let path = dir.join(name);
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(&path, text).unwrap();
        made.push(parent.to_path_buf());
        made.push(path);
    }
    for path in made {
        set_time(&path, t);
    }
}

/// `tools/ci/tree-times TREE LIST`: its output.
fn run_tree_times(tree: &Path, list: &Path) -> std::process::Output {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/ci/tree-times");
    Command::new("python3")
        .arg(script)
        .args([tree, list])
        .output()
        .expect("python3")
}

/// `tools/ci/tree-times TREE LIST`, which must succeed.
fn tree_times(tree: &Path, list: &Path) {
    let out = run_tree_times(tree, list);
    assert!(out.status.success(), "tree-times: {out:?}");
}

/// Builds the package at `tree` into `target` and returns what its program prints.
fn build_and_run(tree: &Path, target: &Path) -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.args(["run", "--quiet", "--offline"])
        .current_dir(tree)
        .env("CARGO_TARGET_DIR", target);
    // What the cargo that runs this test set for its own build.
    for var in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_TARGET"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().expect("cargo");
    assert!(out.status.success(), "cargo run: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

struct Dirs {
    _tmp: tempfile::TempDir,
    /// Where the trees are put: the gate's `/src`.
    src: PathBuf,
    target: PathBuf,
    list: PathBuf,
}

/// Tree A at `src`, built into `target` the way the gate does it when `gate` is set.
fn built_a(gate: bool) -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let d = Dirs {
        src: tmp.path().join("src"),
        target: tmp.path().join("target"),
        list: tmp.path().join("target/ri-gate-tree"),
        _tmp: tmp,
    };
    std::fs::create_dir(&d.target).unwrap();
    put(&d.src, A, days_ago(100));
    if gate {
        tree_times(&d.src, &d.list);
    }
    assert_eq!(build_and_run(&d.src, &d.target), "part-a value-a one,two\n");
    d
}

/// What a clean target dir builds from tree B.
fn clean_b() -> String {
    let tmp = tempfile::tempdir().unwrap();
    put(&tmp.path().join("src"), B, days_ago(200));
    build_and_run(&tmp.path().join("src"), &tmp.path().join("target"))
}

#[test]
fn an_older_tree_at_the_same_path_is_built_as_a_clean_target_dir_builds_it() {
    let clean = clean_b();
    assert_eq!(clean, B_PRINTS);
    let d = built_a(true);
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    assert_eq!(build_and_run(&d.src, &d.target), clean);
}

/// The reason for `tools/ci/tree-times`: cargo alone keeps the build of tree A. When this
/// fails cargo has learnt to see such a tree, and the gate no longer needs the script.
#[test]
fn cargo_alone_keeps_the_build_of_the_tree_built_before() {
    let d = built_a(false);
    put(&d.src, B, days_ago(200));
    assert_ne!(build_and_run(&d.src, &d.target), B_PRINTS);
}

/// The same tree put in place once more, with its old times, after a run that built nothing
/// (`--only fmt`): the changed files have the time they were given, not the old one.
#[test]
fn a_tree_put_in_place_again_before_it_was_built_is_still_built() {
    let d = built_a(true);
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    assert_eq!(build_and_run(&d.src, &d.target), B_PRINTS);
}

/// Only what differs from the tree built last is given a new time; the rest has the time it
/// was given before, which is older than what was built from it. So cargo builds again what
/// reads a changed file, not the whole tree.
#[test]
fn a_file_that_is_as_in_the_tree_built_last_has_the_time_it_had() {
    let d = built_a(true);
    // `list` loses a file; `src` holds in B what it holds in A.
    let same = ["Cargo.toml", "build.rs", "src/main.rs", "list/one", "src"];
    let had = same.map(|f| time_of(&d.src.join(f)));
    put(&d.src, B, days_ago(200));
    let before = SystemTime::now();
    tree_times(&d.src, &d.list);
    assert_eq!(same.map(|f| time_of(&d.src.join(f))), had);
    for changed in ["src/part.rs", "value.txt", "list"] {
        assert!(time_of(&d.src.join(changed)) > before, "{changed}");
    }
}

/// A new time is later than every time given before, also when the clock was set back since
/// (the guest of `tools/parity` pins its clock).
#[test]
fn a_new_time_is_later_than_the_ones_given_before() {
    let d = built_a(true);
    let mut list: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&d.list).unwrap()).unwrap();
    let ahead = SystemTime::now() + Duration::from_secs(365 * 86400);
    let secs = ahead
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    list["clock"] = secs.into();
    std::fs::write(&d.list, list.to_string()).unwrap();
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    assert!(time_of(&d.src.join("value.txt")) > ahead);
}

/// A run killed after it gave the changed files their times and before it wrote the list:
/// the list is the old one, the next run finds the same differences, and tree B is built.
/// The killed run is its effect: a run, then the list as it was before it, with tree B put in
/// place once more (its times old again) so that nothing but the next run can help.
#[test]
fn a_run_killed_before_it_wrote_the_list_leaves_no_old_build() {
    let d = built_a(true);
    let before = std::fs::read(&d.list).unwrap();
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    std::fs::write(&d.list, before).unwrap();
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    assert_eq!(build_and_run(&d.src, &d.target), B_PRINTS);
}

/// The list is replaced in one rename, written last: a reader never finds half of one, and
/// the run leaves no other file behind than the list and its lock.
#[test]
fn the_list_is_replaced_whole() {
    let d = built_a(true);
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &d.list);
    let _: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&d.list).unwrap()).unwrap();
    let mut left: Vec<String> = std::fs::read_dir(&d.target)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with("ri-gate-tree"))
        .collect();
    left.sort();
    assert_eq!(left, ["ri-gate-tree", "ri-gate-tree.lock"]);
}

/// Two runs at once on one list (two gates in one target dir from different trees) would
/// each record a tree the other did not see: the second is refused, says why, and changes
/// neither the list nor a time.
#[test]
fn a_second_run_on_a_list_in_use_is_refused() {
    use std::os::fd::AsRawFd;
    let d = built_a(true);
    let list = std::fs::read(&d.list).unwrap();
    put(&d.src, B, days_ago(200));
    let time = time_of(&d.src.join("value.txt"));
    let held = std::fs::File::create(d.list.with_extension("lock")).unwrap();
    // SAFETY: flock(2) on a descriptor this test owns.
    assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
    let out = run_tree_times(&d.src, &d.list);
    let said = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(said.contains("another run is using"), "{said}");
    assert_eq!(std::fs::read(&d.list).unwrap(), list);
    assert_eq!(time_of(&d.src.join("value.txt")), time);
    // The first run is over: the next one is taken.
    drop(held);
    tree_times(&d.src, &d.list);
    assert_eq!(build_and_run(&d.src, &d.target), B_PRINTS);
}

/// One checkout built into two target dirs, each with its own list: each builds what changed
/// since its own last run. The time a run for one dir gives a file is later than the other
/// dir's builds, so nothing is skipped there; and a run that finds the tree as its list has
/// it changes no time, so the other dir builds nothing again for it.
#[test]
fn one_tree_built_into_two_target_dirs_is_right_in_both() {
    let d = built_a(true);
    let second = d.target.with_file_name("target2");
    std::fs::create_dir(&second).unwrap();
    let list2 = second.join("ri-gate-tree");
    tree_times(&d.src, &list2);
    assert_eq!(build_and_run(&d.src, &second), "part-a value-a one,two\n");
    // Tree B, seen first by the run for the second dir, and built there only.
    put(&d.src, B, days_ago(200));
    tree_times(&d.src, &list2);
    assert_eq!(build_and_run(&d.src, &second), B_PRINTS);
    // The first dir still holds tree A's build; its own run sees B too.
    tree_times(&d.src, &d.list);
    let given = ["src/part.rs", "value.txt", "list"].map(|f| time_of(&d.src.join(f)));
    assert_eq!(build_and_run(&d.src, &d.target), B_PRINTS);
    // Nothing changed since: neither run gives a time, so neither dir builds again.
    tree_times(&d.src, &list2);
    tree_times(&d.src, &d.list);
    assert_eq!(
        ["src/part.rs", "value.txt", "list"].map(|f| time_of(&d.src.join(f))),
        given
    );
}
