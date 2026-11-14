//! The shipped shell carries no JavaScript interpreter: `cargo build --release -p shell` and
//! tools/build.sh (which tools/install.sh runs) build it from compiled code only (a QML tree that differs from the build
//! is refused at startup, not interpreted). `cargo test` unifies features and always builds
//! qml with `interp`; tools/testenv/ri-gate builds and lints the AOT-only shell on its own.
use std::path::Path;
use std::process::Command;

/// Feature flags of the `cargo build` line of tools/build.sh, which tools/install.sh runs.
fn install_features(root: &Path) -> Vec<String> {
    let script = std::fs::read_to_string(root.join("tools/build.sh")).expect("read build.sh");
    let script = script.replace("\\\n", " ");
    let line = script
        .lines()
        .find(|l| l.contains("cargo build"))
        .expect("build.sh runs cargo build");
    let words: Vec<&str> = line[line.find("cargo build").unwrap()..]
        .split_whitespace()
        .take_while(|w| !matches!(*w, "&&" | "||" | ";" | "|"))
        .collect();
    assert!(
        words.windows(2).any(|w| w == ["-p", "shell"]),
        "build.sh does not build the shell: {line}"
    );
    let mut flags = Vec::new();
    let mut it = words.iter();
    while let Some(w) = it.next() {
        match *w {
            "--no-default-features" | "--all-features" => flags.push(w.to_string()),
            "--features" | "-F" => {
                flags.push(w.to_string());
                flags.push(it.next().expect("feature list").to_string());
            }
            w if w.starts_with("--features=") => flags.push(w.to_string()),
            _ => {}
        }
    }
    flags
}

/// `cargo tree -p shell <args> <flags>`.
fn tree(root: &Path, args: &[&str], flags: &[String]) -> String {
    let out = Command::new(env!("CARGO"))
        .current_dir(root)
        .args(["tree", "-p", "shell", "--prefix", "none"])
        .args(args)
        .args(flags)
        .output()
        .expect("run cargo tree");
    assert!(
        out.status.success(),
        "cargo tree failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// qml's features and the crates `cargo build -p shell <flags>` links.
fn qml_tree(root: &Path, flags: &[String]) -> String {
    tree(root, &["-e", "normal,features", "-i", "qml"], flags)
        + &tree(root, &["-e", "normal"], flags)
}

fn assert_no_interpreter(what: &str, tree: &str) {
    for bad in ["qml feature \"interp\"", "oxc_parser "] {
        assert!(
            !tree.lines().any(|l| l.starts_with(bad)),
            "{what} carries the JavaScript interpreter ({bad}):\n{tree}"
        );
    }
}

#[test]
fn release_shell_has_no_interpreter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_no_interpreter("`cargo build --release -p shell`", &qml_tree(&root, &[]));
}

#[test]
fn installed_shell_has_no_interpreter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let flags = install_features(&root);
    assert_no_interpreter(
        &format!("tools/build.sh (cargo build -p shell {})", flags.join(" ")),
        &qml_tree(&root, &flags),
    );
}

/// The check above sees the interpreter where it is: `--features interp` links it.
#[test]
fn interpreter_feature_is_detected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = qml_tree(&root, &["--features".into(), "interp".into()]);
    assert!(
        tree.lines()
            .any(|l| l.starts_with("qml feature \"interp\"")),
        "--features interp does not show qml/interp:\n{tree}"
    );
    assert!(tree.lines().any(|l| l.starts_with("oxc_parser ")), "{tree}");
}
