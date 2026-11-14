//! tools/install.sh builds the repository's tree and makes the rusty-impulse config a copy of it, keeps
//! the release it replaces (the binary and the tree it was compiled from), `--rollback`
//! switches back to it, and an install whose binary was not compiled from the tree replaces
//! nothing. Each test installs into a prefix, a home and a rusty-impulse config of its own, from a tree
//! of its own; `rusty-impulse --check-tree` is what the script asks, so the real binary answers it
//! here too. Whether the installed shell starts is asked the way Hyprland starts it,
//! `rusty-impulse -c rusty-impulse` ([`Sandbox::starts`]).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

/// A throwaway prefix, home, rusty-impulse config and tree to install.
struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ri-install-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        Sandbox { dir }
    }

    fn prefix(&self) -> PathBuf {
        self.dir.join("prefix")
    }

    fn lib(&self) -> PathBuf {
        self.prefix().join("lib/rusty-impulse")
    }

    /// The rusty-impulse config `rusty-impulse -c rusty-impulse` runs.
    fn config(&self) -> PathBuf {
        self.dir.join("config/rustshell/rusty-impulse")
    }

    /// The tree an install is given as the repository's (`RI_ROOT`).
    fn source(&self) -> PathBuf {
        self.dir.join("source")
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// The user's data directory, where `XDG_DATA_HOME` names none.
    fn data(&self) -> PathBuf {
        self.dir.join("home/.local/share")
    }

    /// `tools/install.sh ARGS` on this sandbox.
    fn command(&self, args: &[&str]) -> Command {
        self.command_of(&script(), args)
    }

    /// The same for the copy `script` of tools/install.sh.
    fn command_of(&self, script: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new("bash");
        cmd.arg(script)
            .args(args)
            .env("PREFIX", self.prefix())
            .env("HOME", self.dir.join("home"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_STATE_HOME", self.state())
            .env_remove("XDG_DATA_HOME")
            .env("RI_ROOT", self.source())
            .env_remove("NOBUILD")
            .env_remove("BIN")
            .env_remove("RI_COMMIT")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        cmd
    }

    /// Runs `tools/install.sh ARGS`, installing `bin` if given.
    fn script(&self, args: &[&str], bin: Option<&Path>) -> Output {
        let mut cmd = self.command(args);
        if let Some(bin) = bin {
            cmd.env("BIN", bin);
        }
        cmd.output().unwrap()
    }

    fn install(&self, bin: &Path) -> Output {
        self.script(&[], Some(bin))
    }

    fn rollback(&self) -> Output {
        self.script(&["--rollback"], None)
    }

    /// `$PREFIX/bin/rusty-impulse ARGS` with this sandbox's home and config directories.
    fn installed_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(self.prefix().join("bin/rusty-impulse"));
        cmd.args(args)
            .env("HOME", self.dir.join("home"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env_remove("XDG_CONFIG_DIRS");
        cmd
    }

    /// Whether the installed binary, started as Hyprland starts it (`rusty-impulse -c rusty-impulse`), finds the
    /// rusty-impulse config and runs it.
    fn starts(&self) -> bool {
        self.installed_command(&["-c", "rusty-impulse", "--check-tree"])
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// The installed `rusty-impulse`, read through its links.
    fn installed(&self) -> Vec<u8> {
        std::fs::read(self.prefix().join("bin/rusty-impulse")).unwrap()
    }

    /// The release directory the link `name` (current, previous) points at.
    fn release(&self, name: &str) -> Option<PathBuf> {
        let target = std::fs::read_link(self.lib().join(name)).ok()?;
        Some(self.lib().join(target))
    }

    fn releases(&self) -> Vec<String> {
        names(&self.lib().join("releases"))
    }

    /// A stand-in for an `rusty-impulse` compiled from the tree [`write_tree`] writes for `tree`: it
    /// answers `--check-tree -p DIR` and `-c rusty-impulse --check-tree` as the real one does and does
    /// nothing else.
    fn fake(&self, name: &str, tree: &str) -> PathBuf {
        self.executable(
            name,
            &format!(
                "#!/bin/sh\n# {name}\n\
                 if [ \"$1\" = --version ]; then echo 'rusty-impulse {name}'; echo 'JavaScript interpreter: none'; exit 0; fi\n\
                 {AS_LAUNCHED}\
                 [ \"$1\" = --check-tree ] && [ \"$2\" = -p ] || exit 2\n\
                 if [ \"$(cat \"$3/shell.qml\")\" = '// tree {tree}' ]; then\n\
                 \x20   echo \"rusty-impulse: $3 has the scripts this binary was compiled from\"\n\
                 \x20   exit 0\n\
                 fi\n\
                 echo \"rusty-impulse: $3 differs from the tree this binary was compiled from:\" >&2\n\
                 exit 1\n"
            ),
        )
    }

    fn executable(&self, name: &str, script: &str) -> PathBuf {
        let p = self.dir.join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.dir).is_err() {
            // A prefix whose directories the user may not write (the sudo test's).
            writable_again(&self.dir);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Lets the user write every directory under `dir` again.
fn writable_again(dir: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        writable_again(&e.path());
    }
}

/// What a stand-in `rusty-impulse` does with `-c rusty-impulse --check-tree`: the real one's lookup of the config
/// named rusty-impulse (Quickshell's `locateConfigFile`, with the XDG Base Directory Specification's rule
/// for a relative `XDG_CONFIG_HOME`), then the same answer as for `--check-tree -p` on it.
const AS_LAUNCHED: &str = r#"if [ "$1 $2 $3" = '-c rusty-impulse --check-tree' ]; then
    case ${XDG_CONFIG_HOME-} in /*) base=$XDG_CONFIG_HOME ;; *) base=$HOME/.config ;; esac
    if [ -f "$base/rustshell/shell.qml" ] || [ ! -f "$base/rustshell/rusty-impulse/shell.qml" ]; then
        echo 'Could not find "rusty-impulse" config directory in any valid config path.' >&2
        exit 255
    fi
    set -- --check-tree -p "$(realpath "$base/rustshell/rusty-impulse")"
fi
"#;

/// tools/install.sh of this checkout.
fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/install.sh")
}

/// The QML tree named `tree` at `dir`, replacing what is there.
fn write_tree(dir: &Path, tree: &str) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("modules")).unwrap();
    std::fs::write(dir.join("shell.qml"), format!("// tree {tree}\n")).unwrap();
    std::fs::write(dir.join("modules/Bar.qml"), format!("// bar of {tree}\n")).unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Everything under `dir`: each path with its link target, or its content.
fn listing(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let (p, kind) = (e.path(), e.file_type().unwrap());
            let what = if kind.is_symlink() {
                std::fs::read_link(&p)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
                    .into()
            } else if kind.is_dir() {
                dirs.push(p.clone());
                Vec::new()
            } else {
                std::fs::read(&p).unwrap()
            };
            out.push((p.strip_prefix(dir).unwrap().to_path_buf(), what));
        }
    }
    out.sort();
    out
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[track_caller]
fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "install.sh failed:\n{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
}

#[track_caller]
fn refused(out: &Output, why: &str) {
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let err = text(&out.stderr);
    assert!(err.contains(why), "no `{why}` in:\n{err}");
}

/// libthai is optional: an install on a machine without it goes through and says what the
/// user gets without it, in the binary's own words (`rusty-impulse --version`, which
/// `line_break_equiv` checks on a machine with the library and on one without). With the
/// library, or with a binary from before it told, nothing is said.
#[test]
fn an_install_names_a_missing_libthai_and_goes_on() {
    const NOTE: &str =
        "libthai (libthai.so.0) is not installed: Thai text is broken at spaces only";
    let s = Sandbox::new("libthai");
    write_tree(&s.source(), "A");
    let fake = std::fs::read_to_string(s.fake("rusty-impulse-a", "A")).unwrap();
    let telling = |name: &str, thai: &str| {
        let says =
            format!("echo 'JavaScript interpreter: none'; echo 'Thai line breaking: {thai}'");
        let script = fake.replace("echo 'JavaScript interpreter: none'", &says);
        s.executable(
            name,
            &script.replace("# rusty-impulse-a", &format!("# {name}")),
        )
    };
    let without = telling(
        "rusty-impulse-without",
        "at spaces only (libthai.so.0 is not installed)",
    );
    let with = telling("rusty-impulse-with", "between words (libthai)");
    let silent = s.fake("rusty-impulse-silent", "A");

    let out = s.install(&without);
    ok(&out);
    assert!(text(&out.stderr).contains(NOTE), "{}", text(&out.stderr));
    assert_eq!(s.installed(), std::fs::read(&without).unwrap());
    // Again, as the build that is already installed.
    let out = s.install(&without);
    ok(&out);
    assert!(text(&out.stdout).contains("is already this build"));
    assert!(text(&out.stderr).contains(NOTE), "{}", text(&out.stderr));
    for bin in [&with, &silent] {
        let out = s.install(bin);
        ok(&out);
        assert!(
            !text(&out.stderr).contains("libthai"),
            "{}",
            text(&out.stderr)
        );
        assert_eq!(s.installed(), std::fs::read(bin).unwrap());
    }
}

#[test]
fn first_install_links_the_release_and_has_nothing_to_roll_back_to() {
    let s = Sandbox::new("first");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let link = std::fs::read_link(s.prefix().join("bin/rusty-impulse")).unwrap();
    assert_eq!(
        link,
        Path::new("../lib/rusty-impulse/current/rusty-impulse")
    );
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    let current = s.release("current").unwrap();
    assert_eq!(listing(&current.join("tree")), listing(&s.source()));
    assert_eq!(listing(&s.config()), listing(&s.source()));
    assert_eq!(s.release("previous"), None);
    let before = listing(&s.dir);
    refused(&s.rollback(), "no previous release to roll back to");
    assert_eq!(listing(&s.dir), before);
}

#[test]
fn install_replaces_nothing_when_the_binary_was_not_compiled_from_the_tree() {
    let s = Sandbox::new("refused");
    write_tree(&s.source(), "A");
    let b = s.fake("rusty-impulse-b", "B");
    refused(&s.install(&b), "nothing was installed");
    assert!(!s.prefix().exists(), "{:?}", listing(&s.prefix()));
    assert!(!s.config().exists(), "{:?}", listing(&s.config()));
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    let before = listing(&s.dir);
    refused(&s.install(&b), "nothing was installed");
    assert_eq!(listing(&s.dir), before);
}

#[test]
fn rollback_restores_the_binary_and_its_tree_and_rolls_forward_again() {
    let s = Sandbox::new("rollback");
    write_tree(&s.source(), "A");
    let mode = std::fs::metadata(s.source()).unwrap().permissions().mode();
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let tree_a = listing(&s.config());
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    let tree_b = listing(&s.config());
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    let (release_a, release_b) = (
        s.release("previous").unwrap(),
        s.release("current").unwrap(),
    );
    assert_eq!(listing(&release_a.join("tree")), tree_a);
    assert_eq!(listing(&release_b.join("tree")), tree_b);

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), tree_a);
    assert_eq!(
        std::fs::metadata(s.config()).unwrap().permissions().mode(),
        mode
    );
    assert_eq!(s.release("current").unwrap(), release_a);
    assert_eq!(s.release("previous").unwrap(), release_b);
    // The installed binary runs the config it now finds.
    let check = Command::new(s.prefix().join("bin/rusty-impulse"))
        .args(["--check-tree", "-p"])
        .arg(s.config())
        .output()
        .unwrap();
    assert!(check.status.success(), "{}", text(&check.stderr));
    // The replaced tree was release B's as stored: nothing else is left behind.
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    assert_eq!(listing(&s.config()), tree_b);
    assert_eq!(s.release("current").unwrap(), release_b);
    assert_eq!(s.release("previous").unwrap(), release_a);
}

/// A file time long before any build.
fn long_ago() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

/// Sets the modification time of `p` and of everything under it.
fn set_times(p: &Path, to: SystemTime) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).unwrap() {
            set_times(&e.unwrap().path(), to);
        }
    }
    File::open(p).unwrap().set_modified(to).unwrap();
}

fn modified(p: &Path) -> SystemTime {
    std::fs::metadata(p).unwrap().modified().unwrap()
}

/// A stand-in for cargo with cargo's rule for compiling the QML tree again (Cargo Book, build
/// scripts, `rerun-if-changed` and `rerun-if-env-changed`): a file of RI_ROOT is newer than
/// the last build, or RI_ROOT or RI_ROOT_SUM has another value than at the last build. Its
/// `rusty-impulse` runs the tree whose shell.qml it compiled and names the commit and the tree's
/// checksum the last build was given, as the real one does (`--version`). It logs each call
/// and each compilation, and of the last build its arguments and its CARGO_ENCODED_RUSTFLAGS.
const CARGO: &str = r#"#!/bin/sh
d='@DIR@'
echo "$1" >> "$d/calls"
if [ "$1" = metadata ]; then printf '{"target_directory":"%s"}\n' "$d/target"; exit 0; fi
[ "$1" = build ] || exit 0
printf '%s\n' "$*" > "$d/build-args"
printf %s "${CARGO_ENCODED_RUSTFLAGS-unset}" > "$d/build-flags"
printf 'rusty-impulse 0.1.0 (commit %s)\nQML tree: sum %s\n' "${RI_COMMIT-unknown}" "${RI_ROOT_SUM-unknown}" > "$d/version"
key="$RI_ROOT ${RI_ROOT_SUM-unset}"
if [ ! -e "$d/last" ] || [ "$(cat "$d/last")" != "$key" ] || [ -n "$(find "$RI_ROOT" -newer "$d/last")" ]; then
    echo compile >> "$d/calls"
    {
        echo '#!/bin/sh'
        echo "if [ \"\$1\" = --version ]; then cat '$d/version'; echo 'JavaScript interpreter: none'; exit 0; fi"
        # What a test has the binary hold besides its script.
        [ ! -e "$d/names" ] || sed 's/^/# /' "$d/names"
        cat <<'EOS'
@AS_LAUNCHED@
EOS
        echo '[ "$1" = --check-tree ] && [ "$2" = -p ] || exit 2'
        echo "[ \"\$(cat \"\$3/shell.qml\")\" = '$(cat "$RI_ROOT/shell.qml")' ] || exit 1"
        echo 'echo "rusty-impulse: $3 has the scripts this binary was compiled from"'
    } > "$d/rusty-impulse"
    chmod +x "$d/rusty-impulse"
    printf %s "$key" > "$d/last"
fi
printf '{"reason":"compiler-artifact","target":{"name":"rusty-impulse"},"executable":"%s"}\n' "$d/rusty-impulse"
"#;

/// The commit the installs of a [`Building`] name.
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A [`Sandbox`] whose installs build with the stand-in [`CARGO`].
struct Building {
    s: Sandbox,
    path: String,
}

impl Building {
    /// None (after the report testenv makes of it) on a machine without jq.
    fn new(test: &str) -> Option<Self> {
        if !testenv::require("jq") {
            return None;
        }
        let s = Sandbox::new(test);
        let dir = s.dir.join("cargo");
        std::fs::create_dir(&dir).unwrap();
        s.executable(
            "cargo/cargo",
            &CARGO
                .replace("@DIR@", &dir.display().to_string())
                .replace("@AS_LAUNCHED@", AS_LAUNCHED.trim_end()),
        );
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
        let tools = s.dir.join("checkout/tools");
        std::fs::create_dir_all(&tools).unwrap();
        for name in ["install.sh", "build.sh"] {
            std::fs::copy(script().with_file_name(name), tools.join(name)).unwrap();
        }
        Some(Building { s, path })
    }

    /// `tools/install.sh` of a copy of the sources that holds only the scripts, which is no
    /// git checkout (whatever this one is): the caller names its commit, [`COMMIT`].
    fn script(&self) -> Command {
        let mut cmd = self
            .s
            .command_of(&self.s.dir.join("checkout/tools/install.sh"), &[]);
        cmd.env("PATH", &self.path).env("RI_COMMIT", COMMIT);
        cmd
    }

    /// `tools/install.sh`: build, then install.
    fn install(&self) -> Output {
        self.script().output().unwrap()
    }

    /// That copy's own `qml`.
    fn checkout_tree(&self) -> PathBuf {
        self.s.dir.join("checkout/qml")
    }

    /// `tools/install.sh` of that copy as a user runs it: no `RI_ROOT`.
    fn install_checkout(&self) -> Output {
        self.script().env_remove("RI_ROOT").output().unwrap()
    }

    /// The same with `vars` in the environment.
    fn install_with(&self, vars: &[(&str, &str)]) -> Output {
        self.script().envs(vars.iter().copied()).output().unwrap()
    }

    /// The arguments of the last `cargo build`, and the rustc flags its
    /// CARGO_ENCODED_RUSTFLAGS held ("unset" without the variable).
    fn last_build(&self) -> (String, Vec<String>) {
        let read = |name: &str| std::fs::read_to_string(self.s.dir.join("cargo").join(name));
        let flags = read("build-flags").unwrap();
        (
            read("build-args").unwrap(),
            flags.split('\x1f').map(str::to_string).collect(),
        )
    }

    /// What cargo was called for and when it compiled, in order.
    fn calls(&self) -> String {
        std::fs::read_to_string(self.s.dir.join("cargo/calls")).unwrap()
    }

    /// Whether the installed binary runs the rusty-impulse config.
    fn runs_the_config(&self) -> bool {
        self.s.starts()
    }
}

/// A rollback puts the older tree back in the config with the file times it was stored with.
/// The install after it builds the repository's tree, not the config, and puts it back.
#[test]
fn an_install_after_a_rollback_puts_the_repository_tree_back() {
    let Some(b) = Building::new("build-restored") else {
        return;
    };
    let s = &b.s;
    write_tree(&s.source(), "A");
    set_times(&s.source().join("shell.qml"), long_ago());
    ok(&b.install());
    write_tree(&s.source(), "B");
    ok(&b.install());
    assert_eq!(
        b.calls(),
        "metadata\nbuild\ncompile\nmetadata\nbuild\ncompile\n"
    );

    ok(&s.rollback());
    let file = s.config().join("shell.qml");
    assert_eq!(modified(&file), long_ago(), "restored with another time");
    assert!(b.runs_the_config());
    ok(&b.install());
    // Tree B is as it was at the last build: not compiled again.
    assert_eq!(
        b.calls(),
        "metadata\nbuild\ncompile\nmetadata\nbuild\ncompile\nmetadata\nbuild\n"
    );
    assert!(b.runs_the_config());
    assert_eq!(listing(&s.config()), listing(&s.source()));
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));
}

/// A tree with file times older than cargo's last build (a checkout put back with its times)
/// is built all the same, in the one build of the install: the build is told what the tree
/// holds, not only when its files changed.
#[test]
fn a_tree_with_older_file_times_than_the_last_build_is_built() {
    let Some(b) = Building::new("build-older") else {
        return;
    };
    let s = &b.s;
    write_tree(&s.source(), "A");
    ok(&b.install());
    write_tree(&s.source(), "B");
    set_times(&s.source(), long_ago());
    ok(&b.install());
    assert_eq!(
        b.calls(),
        "metadata\nbuild\ncompile\nmetadata\nbuild\ncompile\n"
    );
    assert!(b.runs_the_config());
    assert_eq!(
        std::fs::read_to_string(s.config().join("shell.qml")).unwrap(),
        "// tree B\n"
    );
    // A tree that is as it was at the last build is not compiled again.
    ok(&b.install());
    assert_eq!(
        b.calls(),
        "metadata\nbuild\ncompile\nmetadata\nbuild\ncompile\nmetadata\nbuild\n"
    );
}

#[test]
fn rollback_leaves_a_config_the_older_binary_runs_alone() {
    let s = Sandbox::new("same-tree");
    write_tree(&s.source(), "A");
    let first = s.fake("rusty-impulse-first", "A");
    ok(&s.install(&first));
    ok(&s.install(&s.fake("rusty-impulse-second", "A")));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let config = listing(&s.config());
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&first).unwrap());
    assert_eq!(listing(&s.config()), config);
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));
}

#[test]
fn rollback_keeps_a_config_edited_since_its_install() {
    let s = Sandbox::new("edited");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    let tree_a = listing(&s.config());
    write_tree(&s.source(), "B");
    ok(&s.install(&s.fake("rusty-impulse-b", "B")));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let edited = listing(&s.config());
    let out = s.rollback();
    ok(&out);
    assert_eq!(listing(&s.config()), tree_a);
    let kept = names(&s.state().join("rusty-impulse"));
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].starts_with("replaced-"), "{kept:?}");
    let kept = s.state().join("rusty-impulse").join(&kept[0]);
    assert_eq!(listing(&kept), edited);
    assert!(text(&out.stdout).contains(kept.to_str().unwrap()));
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
}

/// An install replaces the config and keeps one edited since its install, as a rollback does.
#[test]
fn install_keeps_a_config_edited_since_its_install() {
    let s = Sandbox::new("install-edited");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let edited = listing(&s.config());
    write_tree(&s.source(), "B");
    let out = s.install(&s.fake("rusty-impulse-b", "B"));
    ok(&out);
    assert_eq!(listing(&s.config()), listing(&s.source()));
    let kept = names(&s.state().join("rusty-impulse"));
    assert_eq!(kept.len(), 1, "{kept:?}");
    let kept = s.state().join("rusty-impulse").join(&kept[0]);
    assert_eq!(listing(&kept), edited);
    assert!(text(&out.stdout).contains(kept.to_str().unwrap()));
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
}

/// A config no install put there (the original's own, before the first install) is kept too.
#[test]
fn first_install_keeps_the_config_it_replaces() {
    let s = Sandbox::new("first-over");
    write_tree(&s.config(), "theirs");
    let theirs = listing(&s.config());
    write_tree(&s.source(), "A");
    let out = s.install(&s.fake("rusty-impulse-a", "A"));
    ok(&out);
    assert_eq!(listing(&s.config()), listing(&s.source()));
    let kept = names(&s.state().join("rusty-impulse"));
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(
        listing(&s.state().join("rusty-impulse").join(&kept[0])),
        theirs
    );
    // No install put that config there, and the message does not say one did.
    let said = text(&out.stdout);
    assert!(
        said.contains("was there before the first install"),
        "{said}"
    );
    assert!(!said.contains("since its install"), "{said}");
}

#[test]
fn install_and_rollback_replace_a_config_that_is_a_link_in_its_target() {
    let s = Sandbox::new("linked");
    let dots = s.dir.join("dots/rusty-impulse");
    write_tree(&dots, "A");
    write_tree(&s.source(), "A");
    std::fs::create_dir_all(s.config().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&dots, s.config()).unwrap();
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    let tree_a = listing(&dots);
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    assert_eq!(listing(&dots), listing(&s.source()));
    let again = s.install(&b);
    ok(&again);
    assert!(text(&again.stdout).contains("is already this build"));
    ok(&s.rollback());
    assert_eq!(std::fs::read_link(s.config()).unwrap(), dots);
    assert_eq!(listing(&dots), tree_a);
    assert_eq!(names(dots.parent().unwrap()), ["rusty-impulse"]);
}

/// A binary installed by the script before it kept releases: a plain file that does not know
/// `--check-tree`, so it is never run to ask.
#[test]
fn a_binary_installed_before_releases_is_kept_and_rolled_back_to() {
    let s = Sandbox::new("legacy");
    write_tree(&s.config(), "L");
    write_tree(&s.source(), "A");
    let config = listing(&s.config());
    let ran = s.dir.join("legacy-ran");
    let legacy = format!("#!/bin/sh\n# legacy\ntouch '{}'\n", ran.display());
    std::fs::create_dir_all(s.prefix().join("bin")).unwrap();
    let installed = s.prefix().join("bin/rusty-impulse");
    std::fs::write(&installed, &legacy).unwrap();
    std::fs::set_permissions(&installed, std::fs::Permissions::from_mode(0o755)).unwrap();
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    let previous = s.release("previous").unwrap();
    assert_eq!(
        std::fs::read(previous.join("rusty-impulse")).unwrap(),
        legacy.as_bytes()
    );
    assert_eq!(listing(&previous.join("tree")), config);
    assert_eq!(listing(&s.config()), listing(&s.source()));
    ok(&s.rollback());
    assert_eq!(s.installed(), legacy.as_bytes());
    assert_eq!(listing(&s.config()), config);
    assert!(!ran.exists(), "the binary without --check-tree was run");
}

/// A plain `rusty-impulse` at the name, as an install before releases left it: never run to ask.
fn a_binary_from_before_releases(s: &Sandbox) -> String {
    let legacy = "#!/bin/sh\n# legacy\nexit 3\n".to_string();
    std::fs::create_dir_all(s.prefix().join("bin")).unwrap();
    std::fs::write(bin_name(s), &legacy).unwrap();
    set_mode(&bin_name(s), 0o755);
    legacy
}

/// What `diff -rq --no-dereference` says of the trees `a` and `b`: nothing if they hold the
/// same files and the same links.
fn differences(a: &Path, b: &Path) -> String {
    let out = Command::new("diff")
        .args(["-rq", "--no-dereference"])
        .args([a, b])
        .output()
        .unwrap();
    assert!(out.status.code().is_some_and(|c| c < 2), "{out:?}");
    text(&out.stdout)
}

/// The config a binary from before releases ran has links to its own files, as the original's tree has
/// (four icons that are links to other icons, one of them from a directory below, and here a
/// link to a directory too). The install over it lays files; the rollback puts the config
/// back as it was, each link a link again: no difference from a copy made before the install.
#[test]
fn a_rollback_puts_the_config_it_kept_back_with_its_links_as_links() {
    let s = Sandbox::new("legacy-links");
    let link = std::os::unix::fs::symlink;
    write_tree(&s.config(), "L");
    let icons = s.config().join("assets/icons");
    std::fs::create_dir_all(icons.join("fluent")).unwrap();
    for name in ["openai", "spark", "cloudflare-dns"] {
        std::fs::write(icons.join(format!("{name}-symbolic.svg")), name).unwrap();
    }
    let links = [
        ("assets/icons/ai-openai-symbolic.svg", "openai-symbolic.svg"),
        (
            "assets/icons/google-gemini-symbolic.svg",
            "spark-symbolic.svg",
        ),
        (
            "assets/icons/fluent/cloudflare.svg",
            "../cloudflare-dns-symbolic.svg",
        ),
        (
            "assets/icons/fluent/cloudflare-filled.svg",
            "../cloudflare-dns-symbolic.svg",
        ),
        ("icons", "assets/icons"),
    ];
    for (at, to) in links {
        link(to, s.config().join(at)).unwrap();
    }
    let before = s.dir.join("before");
    let copied = Command::new("cp")
        .arg("-a")
        .args([s.config(), before.clone()])
        .status()
        .unwrap();
    assert!(copied.success());
    assert_eq!(differences(&before, &s.config()), "");
    let legacy = a_binary_from_before_releases(&s);

    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    assert_eq!(listing(&s.config()), listing(&s.source()));

    ok(&s.rollback());
    assert_eq!(s.installed(), legacy.as_bytes());
    assert_eq!(differences(&before, &s.config()), "");
    for (at, to) in links {
        assert_eq!(
            std::fs::read_link(s.config().join(at)).ok(),
            Some(PathBuf::from(to)),
            "{at}"
        );
    }
    // And forward again: the install's tree, then back to the same config once more.
    ok(&s.rollback());
    assert_eq!(listing(&s.config()), listing(&s.source()));
    ok(&s.rollback());
    assert_eq!(differences(&before, &s.config()), "");
}

/// A release holds all that it runs: a link of that config that leads out of the tree (a
/// file in a dotfiles directory, by a relative or an absolute path) is stored as the file it
/// leads to, and so is a link to the tree's own file by an absolute path. The rollback's
/// config reads the same as the one that was there.
#[test]
fn a_link_out_of_the_config_a_rollback_kept_is_stored_as_its_file() {
    let s = Sandbox::new("legacy-stow");
    use std::os::unix::fs::symlink as link;
    write_tree(&s.config(), "L");
    let dots = s.dir.join("dots");
    std::fs::create_dir_all(&dots).unwrap();
    std::fs::write(dots.join("Stowed.qml"), "// stowed\n").unwrap();
    link("../../../dots/Stowed.qml", s.config().join("Relative.qml")).unwrap();
    link(dots.join("Stowed.qml"), s.config().join("Absolute.qml")).unwrap();
    link(s.config().join("shell.qml"), s.config().join("Own.qml")).unwrap();
    link("modules/Bar.qml", s.config().join("Bar.qml")).unwrap();
    let read = |dir: &Path| {
        ["Relative.qml", "Absolute.qml", "Own.qml", "Bar.qml"]
            .map(|name| std::fs::read(dir.join(name)).unwrap())
    };
    let before = read(&s.config());
    a_binary_from_before_releases(&s);

    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    let stored = s.release("previous").unwrap().join("tree");
    for name in ["Relative.qml", "Absolute.qml", "Own.qml"] {
        assert!(!stored.join(name).is_symlink(), "{name}");
    }
    assert_eq!(
        std::fs::read_link(stored.join("Bar.qml")).ok(),
        Some(PathBuf::from("modules/Bar.qml"))
    );
    assert_eq!(read(&stored), before);

    ok(&s.rollback());
    assert_eq!(listing(&s.config()), listing(&stored));
    assert_eq!(read(&s.config()), before);
}

#[test]
fn only_the_current_and_the_previous_release_are_kept() {
    let s = Sandbox::new("pruned");
    write_tree(&s.source(), "A");
    let (a, b, c) = (
        s.fake("rusty-impulse-a", "A"),
        s.fake("rusty-impulse-b", "A"),
        s.fake("rusty-impulse-c", "A"),
    );
    ok(&s.install(&a));
    ok(&s.install(&b));
    ok(&s.install(&c));
    assert_eq!(s.releases().len(), 2, "{:?}", s.releases());
    assert_eq!(s.installed(), std::fs::read(&c).unwrap());
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
}

#[test]
fn installing_the_installed_build_again_keeps_the_previous_release() {
    let s = Sandbox::new("again");
    write_tree(&s.source(), "A");
    let (a, b) = (
        s.fake("rusty-impulse-a", "A"),
        s.fake("rusty-impulse-b", "A"),
    );
    ok(&s.install(&a));
    ok(&s.install(&b));
    let before = listing(&s.prefix());
    let out = s.install(&b);
    ok(&out);
    assert!(text(&out.stdout).contains("is already this build"));
    assert_eq!(listing(&s.prefix()), before);
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
}

#[test]
fn status_names_the_releases_and_uninstall_removes_them() {
    let s = Sandbox::new("status");
    write_tree(&s.source(), "A");
    refused(&s.script(&["--status"], None), "nothing is installed");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    ok(&s.install(&s.fake("rusty-impulse-b", "A")));
    let out = s.script(&["--status"], None);
    ok(&out);
    let current = s.release("current").unwrap();
    let previous = s.release("previous").unwrap();
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let status = text(&out.stdout);
    assert!(
        status.contains(&format!("current:  {}", name(&current))),
        "{status}"
    );
    assert!(
        status.contains(&format!("previous: {}", name(&previous))),
        "{status}"
    );
    // An edited config that an install replaced is kept, the
    // script never removes these copies, and --status and --uninstall say where they are.
    // --uninstall leaves the rusty-impulse config and says so.
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    write_tree(&s.source(), "C");
    ok(&s.install(&s.fake("rusty-impulse-c", "C")));
    let state = s.state().join("rusty-impulse");
    let status = s.script(&["--status"], None);
    ok(&status);
    let status = text(&status.stdout);
    let copies = format!(
        "replaced rusty-impulse configs: 1 kept in {}",
        state.display()
    );
    assert!(status.contains(&copies), "{status}");
    let config = listing(&s.config());
    let out = s.script(&["--uninstall"], None);
    ok(&out);
    assert_eq!(names(&s.prefix().join("bin")), [""; 0]);
    assert!(!s.lib().exists());
    assert_eq!(listing(&s.config()), config);
    let said = text(&out.stdout);
    let left = format!("left: the rusty-impulse config {}", s.config().display());
    assert!(said.contains(&left), "{said}");
    assert!(said.contains(&format!("in {}", state.display())), "{said}");
    assert_eq!(names(&state).len(), 1);
}

/// The app id of the shell's own windows (Settings, Welcome, the conflicts window), as
/// crates/shell/src/main.rs has it.
fn app_id() -> String {
    let main = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let main = std::fs::read_to_string(main).unwrap();
    main.lines()
        .find_map(|l| {
            l.strip_prefix("const APP_ID: &str = \"")?
                .strip_suffix("\";")
        })
        .expect("APP_ID in crates/shell/src/main.rs")
        .to_string()
}

/// The desktop entry and the icon an install writes for [`app_id`] under the data directory
/// `data`.
fn app_icon_files(data: &Path) -> (PathBuf, PathBuf) {
    let id = app_id();
    (
        data.join(format!("applications/{id}.desktop")),
        data.join(format!("icons/hicolor/scalable/apps/{id}.svg")),
    )
}

/// The bar, the overview and the dock show for a window the icon that the desktop entry of
/// its app id names. For the original's own windows Quickshell's package has that entry and
/// the icon, under Quickshell's app id; nothing has them for this shell's, so its own windows
/// showed a placeholder. An install writes both into the user's data directory, where the
/// shell looks first, also when the build is the installed one already; a rollback leaves
/// them, and --uninstall removes them and nothing else of the user's.
#[test]
fn an_install_writes_the_icon_of_the_shells_own_windows() {
    let s = Sandbox::new("app-icon");
    let id = app_id();
    let (entry, icon) = app_icon_files(&s.data());
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let entry_text = std::fs::read_to_string(&entry).unwrap();
    let lines: Vec<&str> = entry_text.lines().collect();
    assert_eq!(lines[0], "[Desktop Entry]", "{entry_text}");
    for line in ["Type=Application", "NoDisplay=true", &format!("Icon={id}")] {
        assert!(lines.contains(&line), "no `{line}` in:\n{entry_text}");
    }
    let svg = std::fs::read_to_string(&icon).unwrap();
    assert!(
        svg.starts_with("<svg ") && svg.ends_with("</svg>\n"),
        "{svg}"
    );
    assert_eq!(names(icon.parent().unwrap()), [format!("{id}.svg")]);
    assert_eq!(names(entry.parent().unwrap()), [format!("{id}.desktop")]);

    // The same build again (an install from before the two files were written): they are
    // written, and a changed one is the script's again.
    std::fs::remove_file(&icon).unwrap();
    std::fs::write(&entry, "[Desktop Entry]\n").unwrap();
    let out = s.install(&a);
    ok(&out);
    assert!(text(&out.stdout).contains("is already this build"));
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), entry_text);
    assert_eq!(std::fs::read_to_string(&icon).unwrap(), svg);

    // A rollback leaves them.
    write_tree(&s.source(), "B");
    ok(&s.install(&s.fake("rusty-impulse-b", "B")));
    ok(&s.rollback());
    assert_eq!(std::fs::read_to_string(&entry).unwrap(), entry_text);
    assert_eq!(std::fs::read_to_string(&icon).unwrap(), svg);

    // --uninstall removes the two files; the user's own beside them stay.
    let mine = [
        entry.with_file_name("mine.desktop"),
        icon.with_file_name("mine.svg"),
    ];
    for f in &mine {
        std::fs::write(f, "mine").unwrap();
    }
    ok(&s.script(&["--uninstall"], None));
    assert_eq!(names(entry.parent().unwrap()), ["mine.desktop"]);
    assert_eq!(names(icon.parent().unwrap()), ["mine.svg"]);

    // With nothing else in them, the directories made for the icon go as well.
    std::fs::remove_file(&mine[1]).unwrap();
    write_tree(&s.source(), "A");
    ok(&s.install(&a));
    assert!(icon.exists() && entry.exists());
    ok(&s.script(&["--uninstall"], None));
    assert_eq!(names(&s.data().join("icons")), [""; 0]);
    assert_eq!(names(entry.parent().unwrap()), ["mine.desktop"]);
}

/// The two files go into `XDG_DATA_HOME` if it is an absolute path, as the shell reads it
/// (crates/qml/src/xdg.rs), and an install that cannot write them is still an install: the
/// shell runs without them.
#[test]
fn the_icon_follows_xdg_data_home_and_an_install_does_not_need_it() {
    let s = Sandbox::new("app-icon-data-home");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    let run = |args: &[&str], data_home: &Path| {
        let mut cmd = s.command(args);
        cmd.env("BIN", &a).env("XDG_DATA_HOME", data_home);
        cmd.output().unwrap()
    };
    let exist = |data: &Path| {
        let (entry, icon) = app_icon_files(data);
        (entry.exists(), icon.exists())
    };

    let data = s.dir.join("data");
    ok(&run(&[], &data));
    assert_eq!(exist(&data), (true, true));
    assert!(!s.data().exists());
    ok(&run(&["--uninstall"], &data));
    assert_eq!(exist(&data), (false, false));

    // A relative path is ignored, and the script says so.
    let out = run(&[], Path::new("data"));
    ok(&out);
    let err = text(&out.stderr);
    assert!(
        err.contains("XDG_DATA_HOME=data is not an absolute path"),
        "{err}"
    );
    assert_eq!(exist(&s.data()), (true, true));
    ok(&run(&["--uninstall"], Path::new("data")));
    assert_eq!(exist(&s.data()), (false, false));

    // A data directory that cannot be made.
    std::fs::write(s.dir.join("file"), "").unwrap();
    let out = run(&[], &s.dir.join("file/data"));
    ok(&out);
    let err = text(&out.stderr);
    assert!(err.contains("show a placeholder as their icon"), "{err}");
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert!(s.starts());
    ok(&run(&["--uninstall"], &s.dir.join("file/data")));
    assert!(!s.lib().exists());
}

/// Through sudo, $HOME and the rusty-impulse config are root's, not the user's:
/// --status and --uninstall refuse as an install and a rollback do. A stand-in `id` says the
/// script runs as root.
#[test]
fn nothing_runs_through_sudo() {
    let s = Sandbox::new("sudo");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let before = listing(&s.dir);
    s.executable("id", "#!/bin/sh\necho 0\n");
    let path = format!("{}:{}", s.dir.display(), std::env::var("PATH").unwrap());
    for args in [&[][..], &["--rollback"], &["--status"], &["--uninstall"]] {
        let out = s
            .command(args)
            .env("BIN", &a)
            .env("PATH", &path)
            .env("SUDO_USER", "someone")
            .output()
            .unwrap();
        refused(&out, "run it as someone, not through sudo");
    }
    std::fs::remove_file(s.dir.join("id")).unwrap();
    assert_eq!(listing(&s.dir), before);
}

/// `rusty-impulse --check-tree -p DIR` with the run dir `run`.
fn check_tree(tree: &Path, run: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .args(["--check-tree", "-p"])
        .arg(tree)
        .env("XDG_RUNTIME_DIR", run)
        .output()
        .unwrap()
}

/// The shell itself answers the script: it accepts the tree it was compiled from, names the
/// files of one that differs, and neither starts nor registers an instance to do so.
#[test]
fn the_shell_checks_the_tree_it_was_compiled_from() {
    const ADDED: &str = "modules/common/functions/added.js";
    let s = Sandbox::new("real");
    let real = Path::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    let copied = Command::new("cp")
        .arg("-a")
        .arg(ricompiled::ROOT)
        .arg(s.source())
        .status()
        .unwrap();
    assert!(copied.success());
    let run = s.dir.join("run");
    std::fs::create_dir_all(&run).unwrap();

    let out = check_tree(&s.source(), &run);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("has the scripts this binary was compiled from"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(names(&run), [""; 0], "the check left files in the run dir");
    // `cargo test` links the interpreter into this binary (features unify); the shipped build
    // (tools/testenv/ri-gate, shipped-tests) has none and is installed.
    let out = s.install(real);
    if qml::INTERP {
        refused(&out, "has a JavaScript interpreter");
        assert!(
            !s.lib().exists(),
            "a refused install left {}",
            s.lib().display()
        );
        return;
    }
    ok(&out);
    assert_eq!(s.installed(), std::fs::read(real).unwrap());
    let out = check_tree(&s.config(), &run);
    assert!(out.status.success(), "{}", text(&out.stderr));

    std::fs::write(s.source().join(ADDED), "function added() { return 1; }\n").unwrap();
    let missing = ricompiled::SOURCES.last().unwrap().0;
    std::fs::remove_file(s.source().join(missing)).unwrap();
    let out = check_tree(&s.source(), &run);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains(&format!("{ADDED}: 1/1 scripts")), "{err}");
    assert!(err.contains(&format!("{missing}: missing")), "{err}");
    assert_eq!(names(&run), [""; 0], "the check left files in the run dir");

    // The same binary with the changed tree: refused, with the files, and nothing replaced.
    let before = (listing(&s.prefix()), listing(&s.config()));
    let out = s.install(real);
    refused(&out, "nothing was installed");
    assert!(text(&out.stderr).contains(&format!("{ADDED}: 1/1 scripts")));
    assert_eq!((listing(&s.prefix()), listing(&s.config())), before);
}

/// A release whose binary runs the older
/// tree's scripts too (only other files changed: a picture, a shell script, a removed file)
/// still gets its own tree back.
#[test]
fn rollback_restores_a_tree_whose_other_files_changed() {
    let s = Sandbox::new("other-files");
    write_tree(&s.source(), "A");
    std::fs::create_dir_all(s.source().join("assets")).unwrap();
    std::fs::write(s.source().join("assets/icon.svg"), "<svg>a</svg>").unwrap();
    std::fs::write(s.source().join("scripts.sh"), "echo a\n").unwrap();
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let tree_a = listing(&s.config());
    assert_eq!(tree_a, listing(&s.source()));
    std::fs::write(s.source().join("assets/icon.svg"), "<svg>b</svg>").unwrap();
    std::fs::write(s.source().join("scripts.sh"), "echo b\n").unwrap();
    std::fs::write(s.source().join("added.sh"), "echo added\n").unwrap();
    std::fs::remove_file(s.source().join("modules/Bar.qml")).unwrap();
    ok(&s.install(&s.fake("rusty-impulse-b", "A")));
    assert_eq!(listing(&s.config()), listing(&s.source()));
    let status = s.script(&["--status"], None);
    ok(&status);
    assert!(
        text(&status.stdout).contains("is the tree stored with"),
        "{}",
        text(&status.stdout)
    );
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), tree_a);
    // Forward again: the newer tree comes back with its binary.
    ok(&s.rollback());
    assert!(s.config().join("added.sh").exists());
    std::fs::write(s.config().join("added.sh"), "echo edited\n").unwrap();
    let status = s.script(&["--status"], None);
    ok(&status);
    let out = text(&status.stdout);
    assert!(out.contains("differs from the tree stored with"), "{out}");
    assert!(out.contains("added.sh"), "{out}");
}

/// `BIN=` and `NOBUILD=1` install only a binary that says it has no JavaScript
/// interpreter; one with it, or one that does not say, replaces nothing.
#[test]
fn an_install_refuses_a_binary_with_the_interpreter() {
    let Some(b) = Building::new("interp") else {
        return;
    };
    let s = &b.s;
    write_tree(&s.source(), "A");
    let fake = std::fs::read_to_string(s.fake("rusty-impulse-a", "A")).unwrap();
    let linked = s.executable(
        "rusty-impulse-linked",
        &fake.replace(
            "JavaScript interpreter: none",
            "JavaScript interpreter: linked",
        ),
    );
    let silent = s.executable(
        "rusty-impulse-silent",
        &fake.replace("--version ]; then", "--never ]; then"),
    );
    refused(&s.install(&linked), "has a JavaScript interpreter");
    refused(&s.install(&silent), "does not answer --version");
    let target = s.dir.join("cargo/target/release");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::copy(&linked, target.join("rusty-impulse")).unwrap();
    let out = s
        .command(&[])
        .env("PATH", &b.path)
        .env("NOBUILD", "1")
        .output()
        .unwrap();
    refused(&out, "has a JavaScript interpreter");
    assert!(!s.prefix().exists(), "{:?}", listing(&s.prefix()));
}

/// The same binary installed again over a tree whose other files changed replaces
/// the current release, not the one to roll back to.
#[test]
fn the_same_binary_with_another_tree_keeps_the_release_to_roll_back_to() {
    let s = Sandbox::new("same-binary");
    write_tree(&s.source(), "A");
    let (a, b) = (
        s.fake("rusty-impulse-a", "A"),
        s.fake("rusty-impulse-b", "A"),
    );
    ok(&s.install(&a));
    let tree_a = listing(&s.config());
    ok(&s.install(&b));
    std::fs::write(s.source().join("note.txt"), "new").unwrap();
    let out = s.install(&b);
    ok(&out);
    assert_eq!(listing(&s.config()), listing(&s.source()));
    assert!(
        text(&out.stdout).contains("which has the same binary"),
        "{}",
        text(&out.stdout)
    );
    let previous = s.release("previous").unwrap();
    assert_eq!(
        std::fs::read(previous.join("rusty-impulse")).unwrap(),
        std::fs::read(&a).unwrap()
    );
    assert_eq!(s.releases().len(), 2, "{:?}", s.releases());
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), tree_a);
}

/// A rollback that cannot keep the edited config changes nothing: neither the
/// config, nor the links, and it leaves no copy beside the config.
#[test]
fn a_rollback_that_cannot_keep_the_edited_config_changes_nothing() {
    let s = Sandbox::new("atomic");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let edited = listing(&s.config());
    let links = (s.release("current"), s.release("previous"));
    std::fs::write(s.state(), "not a directory").unwrap();
    refused(&s.rollback(), "nothing changed");
    assert_eq!(listing(&s.config()), edited);
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    assert_eq!((s.release("current"), s.release("previous")), links);
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
}

/// An install that cannot keep the edited config changes nothing either: the release it
/// would have installed is not left behind.
#[test]
fn an_install_that_cannot_keep_the_edited_config_changes_nothing() {
    let s = Sandbox::new("atomic-install");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let edited = listing(&s.config());
    let before = (listing(&s.prefix()), s.release("current"));
    write_tree(&s.source(), "B");
    std::fs::write(s.state(), "not a directory").unwrap();
    refused(
        &s.install(&s.fake("rusty-impulse-b", "B")),
        "nothing changed",
    );
    assert_eq!(listing(&s.config()), edited);
    assert_eq!((listing(&s.prefix()), s.release("current")), before);
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
}

/// With no rusty-impulse config (another HOME, or none yet) the install builds the repository's tree and
/// makes the config from it. Without that tree nothing is built, and run through sudo the
/// script refuses instead of writing root's config.
#[test]
fn an_install_without_a_rusty_impulse_config_makes_one_from_the_repository_tree() {
    let Some(b) = Building::new("no-config") else {
        return;
    };
    let s = &b.s;
    refused(&b.install(), "no directory RI_ROOT=");
    assert!(
        !s.dir.join("cargo/calls").exists(),
        "cargo ran: {}",
        b.calls()
    );
    assert!(!s.prefix().exists());

    write_tree(&s.source(), "A");
    s.executable(
        "cargo/id",
        "#!/bin/sh\n[ \"$1\" = -u ] && { echo 0; exit 0; }\nexec /usr/bin/id \"$@\"\n",
    );
    let out = s
        .command(&[])
        .env("PATH", &b.path)
        .env("SUDO_USER", "someone")
        .output()
        .unwrap();
    refused(&out, "not through sudo");
    assert!(!s.prefix().exists());
    assert!(!s.config().exists());

    std::fs::remove_file(s.dir.join("cargo/id")).unwrap();
    ok(&b.install());
    assert_eq!(b.calls(), "metadata\nbuild\ncompile\n");
    assert_eq!(listing(&s.config()), listing(&s.source()));
    assert!(b.runs_the_config());
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));
}

/// What `git pull` then `tools/install.sh` does: the checkout's tree is newer than the copy
/// an earlier install left in the rusty-impulse config. The install compiles the checkout's tree, not
/// that copy, and the config becomes the new tree.
#[test]
fn an_install_builds_the_checkout_tree_not_the_older_copy_in_the_config() {
    let Some(b) = Building::new("pulled") else {
        return;
    };
    let s = &b.s;
    let tree = b.checkout_tree();
    write_tree(&tree, "A");
    ok(&b.install_checkout());
    let tree_a = listing(&s.config());
    assert_eq!(tree_a, listing(&tree));
    // The pull: only the checkout changes.
    write_tree(&tree, "B");
    std::fs::write(tree.join("modules/New.qml"), "// new\n").unwrap();
    ok(&b.install_checkout());
    assert_eq!(
        b.calls(),
        "metadata\nbuild\ncompile\nmetadata\nbuild\ncompile\n"
    );
    let current = s.release("current").unwrap();
    assert_eq!(listing(&current.join("tree")), listing(&tree));
    assert_eq!(listing(&s.config()), listing(&tree));
    assert!(b.runs_the_config());
    // The installed binary is the one compiled from tree B: it refuses tree A.
    let previous = s.release("previous").unwrap();
    assert_eq!(listing(&previous.join("tree")), tree_a);
    let runs_a = Command::new(s.prefix().join("bin/rusty-impulse"))
        .args(["--check-tree", "-p"])
        .arg(previous.join("tree"))
        .status()
        .unwrap();
    assert!(!runs_a.success());
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));
}

/// A tree whose files are relative links into another directory (stow) is stored
/// as the files they lead to, the config made from it holds files, and it is rolled back to.
#[test]
fn a_tree_of_relative_links_is_stored_as_files() {
    let s = Sandbox::new("stow");
    let dots = s.dir.join("dotfiles/rusty-impulse");
    write_tree(&dots, "A");
    std::fs::create_dir_all(s.source().join("modules")).unwrap();
    std::os::unix::fs::symlink(
        "../dotfiles/rusty-impulse/shell.qml",
        s.source().join("shell.qml"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        "../../dotfiles/rusty-impulse/modules/Bar.qml",
        s.source().join("modules/Bar.qml"),
    )
    .unwrap();
    assert!(s.source().join("modules/Bar.qml").is_file());
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let stored = s.release("current").unwrap().join("tree");
    assert_eq!(listing(&stored), listing(&dots));
    assert_eq!(listing(&s.config()), listing(&dots));
    assert!(!s.config().join("shell.qml").is_symlink());
    ok(&s.install(&s.fake("rusty-impulse-b", "A")));
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
}

/// A rollback reads the links once it holds the lock: an install that finishes
/// just before it got the lock (a stand-in `flock` runs one) is rolled back from.
#[test]
fn a_rollback_reads_the_releases_under_the_lock() {
    let s = Sandbox::new("lock-order");
    write_tree(&s.source(), "A");
    let (a, b, c) = (
        s.fake("rusty-impulse-a", "A"),
        s.fake("rusty-impulse-b", "A"),
        s.fake("rusty-impulse-c", "A"),
    );
    ok(&s.install(&a));
    ok(&s.install(&b));
    let real = Command::new("sh")
        .args(["-c", "command -v flock"])
        .output()
        .unwrap();
    let real = text(&real.stdout).trim().to_string();
    assert!(!real.is_empty(), "no flock");
    let script = script();
    let hooks = s.dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    s.executable(
        "hooks/flock",
        &format!(
            "#!/bin/sh\nif [ -n \"$RI_TEST_INSTALL\" ]; then\n\
             \x20   BIN=\"$RI_TEST_INSTALL\" RI_TEST_INSTALL= bash '{}' >&2 || exit 9\n\
             fi\nexec '{real}' \"$@\"\n",
            script.display()
        ),
    );
    let out = s
        .command(&["--rollback"])
        .env(
            "PATH",
            format!("{}:{}", hooks.display(), std::env::var("PATH").unwrap()),
        )
        .env("RI_TEST_INSTALL", &c)
        .output()
        .unwrap();
    ok(&out);
    assert_eq!(
        s.installed(),
        std::fs::read(&b).unwrap(),
        "{}",
        text(&out.stdout)
    );
    let current = s.release("current").unwrap();
    assert!(current.join("rusty-impulse").is_file());
    let previous = s.release("previous").unwrap();
    assert_eq!(
        std::fs::read(previous.join("rusty-impulse")).unwrap(),
        std::fs::read(&c).unwrap()
    );
}

/// The command line is checked before anything runs.
#[test]
fn the_command_line_is_checked_before_anything_runs() {
    let s = Sandbox::new("args");
    let help = s.script(&["--help"], None);
    ok(&help);
    assert!(text(&help.stdout).contains("tools/install.sh --rollback"));
    refused(
        &s.script(&["--rollback", "now"], None),
        "unexpected argument 'now'",
    );
    refused(&s.script(&["--roll"], None), "unknown argument '--roll'");
    refused(&s.script(&["--uninstall"], None), "nothing is installed");
    write_tree(&s.source(), "A");
    let out = s
        .command(&[])
        .env("BIN", s.dir.join("missing"))
        .output()
        .unwrap();
    refused(&out, "no rusty-impulse binary");
    let broken = s.executable("rusty-impulse-broken", "#!/nonexistent/interpreter\n");
    refused(&s.install(&broken), "cannot run");
    assert!(!s.prefix().exists());
}

/// What the stand-in `ln`, `mv` and `tar` of [`hooked`] do at the first call whose command
/// line (`ln -sfn TARGET LINK`, `mv -T FROM TO`, `tar -C DIR ...`) matches a glob.
enum Hook<'a> {
    /// That call and every later one that matches fail (the disk is full, sudo refused).
    Fail(&'a str),
    /// That call sends the script the signal and fails; later ones run.
    Kill(&'a str, &'a str),
    /// That call sends the script the signal and runs, like the later ones: the signal of a
    /// `kill PID`, which the script's children do not get.
    Signal(&'a str, &'a str),
    /// Every call runs.
    Log,
    /// Both: the calls that match the first glob fail, the first one that matches the second
    /// sends the signal.
    FailThenKill(&'a str, &'a str, &'a str),
    /// That call says [`WAITING`] on its standard output, which is the script's, reads the
    /// FIFO [`resume`] to its end, and runs.
    Pause(&'a str),
}

/// The switch's first step: `current` is made to name the switch.
const TO_THE_SWITCH: &str = "ln * releases/.switch */.current.new";
/// Between the two renames that replace the config: the old one is aside, the new one not
/// yet in its place.
const THE_NEW_CONFIG: &str = "mv -T */.ri-restore.?????? */rustshell/rusty-impulse";
/// The config is replaced, no link is switched.
const PREVIOUS: &str = "ln */.previous.new";
/// `current` is made to name a release: the switch's last step (and, before there was a
/// switch, the step that changed the binary).
const TO_A_RELEASE: &str = "ln * releases/[0-9]* */.current.new";

/// The copy of the QML tree into a new release.
const THE_RELEASE: &str = "tar -C */source --exclude=.git -chf - .";
/// The copy of an edited config into the state directory.
const THE_EDITED_CONFIG: &str = "tar -C */rustshell/rusty-impulse --exclude=.git -c*f - .";
/// A binary from before releases gives its name to the link to its release.
const THE_NAME: &str = "mv -Tf */.rusty-impulse.new */bin/rusty-impulse";
/// The undo's first step: the new config goes back beside the config's place.
const THE_CONFIG_BACK: &str = "mv -T */rustshell/rusty-impulse */.ri-restore.??????";

/// `tools/install.sh ARGS` with stand-ins for `ln`, `mv` and `tar` that do what `hook` says.
fn hooked(s: &Sandbox, hook: &Hook, args: &[&str], bin: Option<&Path>) -> Output {
    let mut cmd = hooked_command(s, hook, args, &std::env::var("PATH").unwrap());
    if let Some(bin) = bin {
        cmd.env("BIN", bin);
    }
    cmd.output().unwrap()
}

/// The file the stand-ins of [`hooked`] log their command lines to, one a line, in order.
fn steps_log(s: &Sandbox) -> PathBuf {
    s.dir.join("steps")
}

/// That command, with `path` as the PATH the stand-ins and the script find the real tools on.
/// The stand-ins are `ln`, `mv` and `sync`; each logs its command line to [`steps_log`].
fn hooked_command(s: &Sandbox, hook: &Hook, args: &[&str], path: &str) -> Command {
    hooked_script(s, hook, s.command(args), path)
}

/// The same for `script`, a command that runs a copy of tools/install.sh on `s`.
fn hooked_script(s: &Sandbox, hook: &Hook, script: Command, path: &str) -> Command {
    const STAND_IN: &str = r#"#!/bin/sh
name=${0##*/}
echo "$name $*" >> "$RI_TEST_STEPS"
if [ -n "$RI_TEST_PAUSE_AT" ]; then
    case "$name $*" in
    $RI_TEST_PAUSE_AT) echo "$RI_TEST_WAITING"; cat "$RI_TEST_RESUME" ;;
    esac
fi
if [ -n "$RI_TEST_KILL_AT" ] && [ ! -e "$RI_TEST_KILLED" ]; then
    case "$name $*" in
    $RI_TEST_KILL_AT)
        : > "$RI_TEST_KILLED"; kill -s "$RI_TEST_SIGNAL" "$RI_TEST_PID"
        [ -n "$RI_TEST_GOES_ON" ] || exit 1 ;;
    esac
fi
if [ -n "$RI_TEST_FAIL_AT" ]; then
    case "$name $*" in
    $RI_TEST_FAIL_AT) echo "$name: $*: No space left on device" >&2; exit 1 ;;
    esac
fi
PATH=$RI_TEST_PATH exec "$name" "$@"
"#;
    let hooks = s.dir.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for tool in ["ln", "mv", "tar", "sync"] {
        s.executable(&format!("hooks/{tool}"), STAND_IN);
    }
    let _ = std::fs::remove_file(killed(s));
    let _ = std::fs::remove_file(steps_log(s));
    let (fail, kill, signal, pause) = match *hook {
        Hook::Fail(at) => (at, "", "", ""),
        Hook::Kill(at, signal) | Hook::Signal(at, signal) => ("", at, signal, ""),
        Hook::Log => ("", "", "", ""),
        Hook::FailThenKill(fail, at, signal) => (fail, at, signal, ""),
        Hook::Pause(at) => ("", "", "", at),
    };
    if !pause.is_empty() {
        let fifo = resume(s);
        let _ = std::fs::remove_file(&fifo);
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
    }
    // The script's pid, for the stand-in that sends it a signal: the script runs its
    // pipelines in a subshell, which is then that stand-in's parent.
    let mut cmd = Command::new("sh");
    cmd.args(["-c", r#"export RI_TEST_PID=$$; exec "$@""#, "sh"])
        .arg(script.get_program())
        .args(script.get_args());
    for (name, value) in script.get_envs() {
        match value {
            Some(value) => cmd.env(name, value),
            None => cmd.env_remove(name),
        };
    }
    cmd.env("PATH", format!("{}:{path}", hooks.display()))
        .env("RI_TEST_PATH", path)
        .env("RI_TEST_FAIL_AT", fail)
        .env("RI_TEST_KILL_AT", kill)
        .env("RI_TEST_SIGNAL", signal)
        .env(
            "RI_TEST_GOES_ON",
            if matches!(hook, Hook::Signal(..)) {
                "1"
            } else {
                ""
            },
        )
        .env("RI_TEST_KILLED", killed(s))
        .env("RI_TEST_STEPS", steps_log(s))
        .env("RI_TEST_PAUSE_AT", pause)
        .env("RI_TEST_WAITING", WAITING)
        .env("RI_TEST_RESUME", resume(s));
    cmd
}

/// The file a [`Hook::Kill`] makes when it sends its signal.
fn killed(s: &Sandbox) -> PathBuf {
    s.dir.join("killed")
}

/// What a [`Hook::Pause`] prints when the script is at its step.
const WAITING: &str = "the stand-in waits";

/// The FIFO a [`Hook::Pause`] reads before it lets the script go on: opening it for writing
/// and closing it does that.
fn resume(s: &Sandbox) -> PathBuf {
    s.dir.join("resume")
}

/// `tools/install.sh ARGS` with a stand-in `ln` that cannot make the link `new`.
fn without_the_link(s: &Sandbox, new: &str, args: &[&str], bin: Option<&Path>) -> Output {
    hooked(s, &Hook::Fail(&format!("ln */{new}")), args, bin)
}

/// Whether the installed binary runs the rusty-impulse config.
fn runs_the_config(s: &Sandbox) -> bool {
    s.starts()
}

/// The links are switched after the config is replaced. An install that
/// cannot switch one puts the config back: the installed release still runs it, and the
/// release before it can still be rolled back to.
#[test]
fn an_install_that_cannot_switch_the_links_leaves_the_installed_release_running() {
    for link in [".current.new", ".previous.new"] {
        let s = Sandbox::new(&format!("links{link}"));
        write_tree(&s.source(), "A");
        let a = s.fake("rusty-impulse-a", "A");
        ok(&s.install(&a));
        write_tree(&s.source(), "B");
        let b = s.fake("rusty-impulse-b", "B");
        ok(&s.install(&b));
        std::fs::write(s.config().join("note.txt"), "mine").unwrap();
        let before = (listing(&s.prefix()), listing(&s.dir.join("config")));

        write_tree(&s.source(), "C");
        let out = without_the_link(&s, link, &[], Some(&s.fake("rusty-impulse-c", "C")));
        assert_eq!(out.status.code(), Some(1), "{link}: {}", text(&out.stderr));
        assert_eq!(s.installed(), std::fs::read(&b).unwrap(), "{link}");
        assert!(
            runs_the_config(&s),
            "{link}: the installed binary does not run the config the install left"
        );
        assert_eq!(
            (listing(&s.prefix()), listing(&s.dir.join("config"))),
            before,
            "{link}"
        );
        assert_eq!(names(&s.state().join("rusty-impulse")), [""; 0], "{link}");
        refused(&out, "nothing was installed");

        ok(&s.rollback());
        assert_eq!(s.installed(), std::fs::read(&a).unwrap(), "{link}");
        assert!(runs_the_config(&s), "{link}");
    }
}

/// The same for a first install: it leaves no config without a binary to run it.
#[test]
fn a_first_install_that_cannot_switch_the_links_installs_nothing() {
    let s = Sandbox::new("first-links");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    let out = without_the_link(&s, ".current.new", &[], Some(&a));
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(!s.dir.join("config").exists(), "{:?}", listing(&s.config()));
    assert_eq!(s.releases(), [""; 0]);
    assert_eq!(s.release("current"), None);
    refused(&out, "nothing was installed");
    ok(&s.install(&a));
    assert!(runs_the_config(&s));
}

/// The modes of everything under `dir`, the links aside.
fn modes(dir: &Path) -> Vec<(PathBuf, u32)> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let (p, kind) = (e.path(), e.file_type().unwrap());
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                dirs.push(p.clone());
            }
            let mode = e.metadata().unwrap().permissions().mode() & 0o777;
            out.push((p.strip_prefix(dir).unwrap().to_path_buf(), mode));
        }
    }
    out.sort();
    out
}

/// A release is read by the user who runs rusty-impulse, who need not be its owner
/// (a prefix of root's). A user with umask 077, whose checkout has files of mode 600, gets a
/// release everyone can read; the config, the user's own, keeps the user's modes.
#[test]
fn a_release_installed_with_umask_077_is_readable_by_everyone() {
    let s = Sandbox::new("umask");
    write_tree(&s.source(), "A");
    for (p, mode) in modes(&s.dir) {
        let private = std::fs::Permissions::from_mode(mode & 0o700);
        std::fs::set_permissions(s.dir.join(p), private).unwrap();
    }
    let a = s.fake("rusty-impulse-a", "A");
    let out = s
        .command_of(Path::new("-c"), &["umask 077 && exec bash \"$0\""])
        .arg(script())
        .env("BIN", &a)
        .output()
        .unwrap();
    ok(&out);
    let closed: Vec<_> = modes(&s.prefix())
        .into_iter()
        .filter(|(p, mode)| {
            let needed = if s.prefix().join(p).is_dir() || p.ends_with("rusty-impulse") {
                0o555
            } else {
                0o444
            };
            mode & needed != needed
        })
        .collect();
    assert!(closed.is_empty(), "not readable by everyone: {closed:?}");
    let stored = s.release("current").unwrap().join("tree");
    assert_eq!(listing(&stored), listing(&s.source()));
    let open: Vec<_> = modes(&s.config())
        .into_iter()
        .filter(|(_, mode)| mode & 0o077 != 0)
        .collect();
    assert!(open.is_empty(), "more open than the umask: {open:?}");
    assert!(runs_the_config(&s));
}

/// A rollback with no rusty-impulse config makes one from the release it switches to,
/// as an install does; one that cannot switch the links removes it again.
#[test]
fn a_rollback_without_a_rusty_impulse_config_makes_one_from_its_release() {
    let s = Sandbox::new("rollback-no-config");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let tree_a = listing(&s.config());
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    std::fs::remove_dir_all(s.dir.join("config")).unwrap();
    let before = listing(&s.prefix());

    refused(
        &without_the_link(&s, ".current.new", &["--rollback"], None),
        "nothing changed",
    );
    assert_eq!(listing(&s.prefix()), before);
    assert!(!s.dir.join("config").exists());

    let out = s.rollback();
    ok(&out);
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), tree_a);
    assert!(runs_the_config(&s));
    let said = text(&out.stdout);
    assert!(
        said.contains(&format!("made {} from that release", s.config().display())),
        "{said}"
    );
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));
}

/// Cargo reads its extra rustc flags from one place only, the first that is
/// set of CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS, target.*.rustflags and build.rustflags (Cargo
/// Book, Configuration, `build.rustflags`). The flags that keep this machine's paths out of
/// the binary go where cargo reads: after the user's in the variable when one is set, in
/// build.rustflags otherwise.
#[test]
fn the_build_hides_its_paths_whatever_rustflags_the_user_set() {
    let Some(b) = Building::new("rustflags") else {
        return;
    };
    write_tree(&b.s.source(), "A");
    // The checkout, Cargo's home and the target directory, the last of which wins where one
    // is inside another.
    let remapped = |flags: &[String]| {
        let names: Vec<&str> = flags
            .iter()
            .map(|f| {
                let dir = f.strip_prefix("--remap-path-prefix=/");
                dir.unwrap_or_else(|| panic!("{flags:?}"))
                    .rsplit_once('=')
                    .unwrap()
                    .1
            })
            .collect();
        assert_eq!(names, [".", "cargo", "target"], "{flags:?}");
    };

    ok(&b.install());
    let (args, flags) = b.last_build();
    assert_eq!(flags, ["unset"]);
    assert!(
        args.contains(" --config build.rustflags=[\"--remap-path-prefix=/"),
        "{args}"
    );

    ok(&b.install_with(&[("RUSTFLAGS", "-C  link-arg=-fuse-ld=mold")]));
    let (args, flags) = b.last_build();
    assert!(!args.contains("--config"), "{args}");
    assert_eq!(flags[..2], ["-C", "link-arg=-fuse-ld=mold"], "{flags:?}");
    remapped(&flags[2..]);

    // The encoded variable comes first, and a flag in it may hold a space.
    ok(&b.install_with(&[
        ("CARGO_ENCODED_RUSTFLAGS", "--cfg\x1ffeature=\"a b\""),
        ("RUSTFLAGS", "-C debuginfo=2"),
    ]));
    let (args, flags) = b.last_build();
    assert!(!args.contains("--config"), "{args}");
    assert_eq!(flags[..2], ["--cfg", "feature=\"a b\""], "{flags:?}");
    remapped(&flags[2..]);

    ok(&b.install_with(&[("RUSTFLAGS", "")]));
    let (args, flags) = b.last_build();
    assert!(!args.contains("--config"), "{args}");
    remapped(&flags);
}

/// The installed binary names the commit it was built from, and a copy of the sources that
/// cannot tell its commit (no .git, no RI_COMMIT) is not built: nothing is installed, rather
/// than a binary that says `commit unknown`.
#[test]
fn a_build_that_cannot_tell_its_commit_installs_nothing() {
    let Some(b) = Building::new("commit") else {
        return;
    };
    write_tree(&b.s.source(), "A");
    let out = b.script().env_remove("RI_COMMIT").output().unwrap();
    refused(&out, "cannot tell the commit");
    assert!(!b.s.dir.join("cargo/calls").exists(), "cargo ran");
    let bin = b.s.prefix().join("bin/rusty-impulse");
    assert!(!bin.exists());
    let says = |commit: &str| {
        let version = Command::new(&bin).arg("--version").output().unwrap();
        let version = String::from_utf8_lossy(&version.stdout).into_owned();
        assert!(version.contains(&format!("(commit {commit})")), "{version}");
        version
    };

    // The copy's own record of the commit it was made from (bench/vm.sh writes it).
    let record = b.s.dir.join("checkout/.build-commit");
    std::fs::write(&record, "89abcdef+changes\n").unwrap();
    ok(&b.script().env_remove("RI_COMMIT").output().unwrap());
    says("89abcdef+changes");
    std::fs::remove_file(&record).unwrap();

    ok(&b.install());
    let version = says(COMMIT);
    let sum = Command::new("bash")
        .arg(script().with_file_name("build.sh"))
        .arg("--sum")
        .arg(b.s.source())
        .output()
        .unwrap();
    let sum = String::from_utf8_lossy(&sum.stdout).into_owned();
    assert_eq!(sum.trim().len(), 64, "{sum}");
    assert!(
        version.contains(&format!("QML tree: sum {sum}")),
        "{version}"
    );
}

/// A binary that names a directory of the machine that built it is another file in every
/// other directory: such a build fails, and nothing is installed.
#[test]
fn a_binary_that_names_the_build_directories_installs_nothing() {
    let Some(b) = Building::new("paths") else {
        return;
    };
    let dirs = ["cargo/target/release/build", "checkout/crates/shell/src"];
    for (tree, dir) in ["A", "B"].into_iter().zip(dirs) {
        // Another tree each time: the stand-in compiles again.
        write_tree(&b.s.source(), tree);
        std::fs::write(
            b.s.dir.join("cargo/names"),
            format!("{}/x.rs\n", b.s.dir.join(dir).display()),
        )
        .unwrap();
        refused(&b.install(), "names this machine's directory");
        assert!(!b.s.prefix().join("bin/rusty-impulse").exists(), "{dir}");
    }
    std::fs::remove_file(b.s.dir.join("cargo/names")).unwrap();
    write_tree(&b.s.source(), "C");
    ok(&b.install());
}

/// The copies of replaced configs the script keeps.
fn kept(s: &Sandbox) -> Vec<String> {
    names(&s.state().join("rusty-impulse"))
}

/// The release `c` is installed over `b` whole: the binary, its tree as the config, `b` to
/// roll back to, one copy of the config edited before, and nothing an interrupted run left.
#[track_caller]
fn c_is_installed_over_b(s: &Sandbox, b: &Path, c: &Path) {
    assert_eq!(s.installed(), std::fs::read(c).unwrap());
    let previous = s.release("previous").unwrap().join("rusty-impulse");
    assert_eq!(std::fs::read(previous).unwrap(), std::fs::read(b).unwrap());
    assert_eq!(listing(&s.config()), listing(&s.source()));
    assert!(runs_the_config(s));
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
    assert_eq!(s.releases().len(), 2, "{:?}", s.releases());
    assert_eq!(kept(s).len(), 1, "{:?}", kept(s));
}

/// How a killed script ended: by the signal itself, or, where it handles the signal, with
/// status 1 after saying so.
#[track_caller]
fn was_killed(s: &Sandbox, out: &Output, signal: &str) {
    assert!(killed(s).exists(), "the script never got to that step");
    if signal == "KILL" {
        assert_eq!(out.status.signal(), Some(9), "{}", text(&out.stderr));
    } else {
        assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
        let said = text(&out.stderr);
        assert!(said.contains("install.sh: interrupted"), "{said}");
    }
}

/// SIGKILL, which no script handles, and the three signals tools/install.sh handles.
const SIGNALS: [&str; 4] = ["KILL", "TERM", "INT", "HUP"];

/// An install cannot replace the config and the links in one step. One that is killed
/// between them (SIGKILL: no handler runs; SIGTERM, SIGINT, SIGHUP: the script's handler does)
/// leaves a shell that starts: the installed name runs a release with the config that release
/// runs. After a signal it handles nothing has changed. After SIGKILL the next install
/// finishes or undoes the killed one first, and then installs.
#[test]
fn an_install_killed_while_it_switches_leaves_a_shell_that_starts() {
    let steps = [PREVIOUS, THE_NEW_CONFIG, TO_A_RELEASE, TO_THE_SWITCH];
    for signal in SIGNALS {
        for (i, at) in steps.iter().enumerate() {
            let step = format!("SIG{signal} at `{at}`");
            let s = Sandbox::new(&format!("kill-install-{signal}-{i}"));
            write_tree(&s.source(), "A");
            ok(&s.install(&s.fake("rusty-impulse-a", "A")));
            write_tree(&s.source(), "B");
            let b = s.fake("rusty-impulse-b", "B");
            ok(&s.install(&b));
            std::fs::write(s.config().join("note.txt"), "mine").unwrap();
            let before = (listing(&s.prefix()), listing(&s.dir.join("config")));

            write_tree(&s.source(), "C");
            let c = s.fake("rusty-impulse-c", "C");
            let out = hooked(&s, &Hook::Kill(at, signal), &[], Some(&c));
            was_killed(&s, &out, signal);
            assert!(
                runs_the_config(&s),
                "{step}: the installed binary does not run the config that is left"
            );
            if signal != "KILL" {
                let after = (listing(&s.prefix()), listing(&s.dir.join("config")));
                assert_eq!(after, before, "{step}");
                assert_eq!(kept(&s), [""; 0], "{step}");
                assert!(text(&out.stderr).contains("interrupted"), "{step}");
            } else if s.release("current").unwrap().ends_with(".switch") {
                let status = s.script(&["--status"], None);
                ok(&status);
                let status = text(&status.stdout);
                assert!(status.contains("was interrupted"), "{step}: {status}");
            }

            let out = s.install(&c);
            ok(&out);
            c_is_installed_over_b(&s, &b, &c);
        }
    }
}

/// The same for a rollback. After SIGKILL the next rollback finishes the killed one if the
/// config was already the older release's, and then rolls forward from it; otherwise it
/// undoes the killed one and rolls back.
#[test]
fn a_rollback_killed_while_it_switches_leaves_a_shell_that_starts() {
    let steps = [TO_A_RELEASE, THE_NEW_CONFIG, PREVIOUS, TO_THE_SWITCH];
    for signal in SIGNALS {
        for (i, at) in steps.iter().enumerate() {
            let step = format!("SIG{signal} at `{at}`");
            let s = Sandbox::new(&format!("kill-rollback-{signal}-{i}"));
            write_tree(&s.source(), "A");
            let a = s.fake("rusty-impulse-a", "A");
            ok(&s.install(&a));
            write_tree(&s.source(), "B");
            let b = s.fake("rusty-impulse-b", "B");
            ok(&s.install(&b));
            std::fs::write(s.config().join("note.txt"), "mine").unwrap();
            let before = (listing(&s.prefix()), listing(&s.dir.join("config")));

            let out = hooked(&s, &Hook::Kill(at, signal), &["--rollback"], None);
            was_killed(&s, &out, signal);
            assert!(
                runs_the_config(&s),
                "{step}: the installed binary does not run the config that is left"
            );
            // Which release the next rollback ends on.
            let mut ends_on = &a;
            if signal != "KILL" {
                let after = (listing(&s.prefix()), listing(&s.dir.join("config")));
                assert_eq!(after, before, "{step}");
                assert_eq!(kept(&s), [""; 0], "{step}");
            } else if !s.config().join("note.txt").exists() {
                // The config was the older release's when the rollback was killed.
                ends_on = &b;
            }

            ok(&s.rollback());
            assert_eq!(s.installed(), std::fs::read(ends_on).unwrap(), "{step}");
            let current = s.release("current").unwrap();
            assert_eq!(
                listing(&s.config()),
                listing(&current.join("tree")),
                "{step}"
            );
            assert!(runs_the_config(&s), "{step}");
            assert_ne!(s.release("previous").unwrap(), current, "{step}");
            assert_eq!(
                names(s.config().parent().unwrap()),
                ["rusty-impulse"],
                "{step}"
            );
            assert_eq!(s.releases().len(), 2, "{step}: {:?}", s.releases());
            assert_eq!(kept(&s).len(), 1, "{step}: {:?}", kept(&s));
        }
    }
}

/// A first install that is killed has no release to keep running; the next install installs,
/// and leaves nothing of the killed one.
#[test]
fn a_first_install_killed_while_it_switches_is_installed_by_the_next() {
    for signal in SIGNALS {
        for (i, at) in [THE_NEW_CONFIG, TO_A_RELEASE].iter().enumerate() {
            let step = format!("SIG{signal} at `{at}`");
            let s = Sandbox::new(&format!("kill-first-{signal}-{i}"));
            write_tree(&s.config(), "theirs");
            let theirs = listing(&s.config());
            write_tree(&s.source(), "A");
            let a = s.fake("rusty-impulse-a", "A");
            let out = hooked(&s, &Hook::Kill(at, signal), &[], Some(&a));
            was_killed(&s, &out, signal);
            if signal != "KILL" {
                assert_eq!(listing(&s.config()), theirs, "{step}");
                assert_eq!(names(&s.prefix().join("bin")), [""; 0], "{step}");
                assert_eq!(s.releases(), [""; 0], "{step}");
                assert_eq!(kept(&s), [""; 0], "{step}");
            }
            ok(&s.install(&a));
            assert_eq!(s.installed(), std::fs::read(&a).unwrap(), "{step}");
            assert_eq!(listing(&s.config()), listing(&s.source()), "{step}");
            assert_eq!(
                names(s.config().parent().unwrap()),
                ["rusty-impulse"],
                "{step}"
            );
            assert_eq!(s.releases().len(), 1, "{step}: {:?}", s.releases());
            let copies = kept(&s);
            assert_eq!(copies.len(), 1, "{step}: {copies:?}");
            let copy = s.state().join("rusty-impulse").join(&copies[0]);
            assert_eq!(listing(&copy), theirs, "{step}");
        }
    }
}

/// An install that can neither switch the links nor put them back (every link to a release
/// fails) leaves the switch in place: the shell starts on the release that was installed, and
/// the next install undoes the failed one first.
#[test]
fn an_install_that_cannot_put_the_links_back_leaves_a_shell_that_starts() {
    let s = Sandbox::new("links-stuck");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let config = listing(&s.config());

    write_tree(&s.source(), "C");
    let c = s.fake("rusty-impulse-c", "C");
    refused(
        &hooked(&s, &Hook::Fail(TO_A_RELEASE), &[], Some(&c)),
        "nor put back what was replaced",
    );
    assert_eq!(listing(&s.config()), config);
    assert!(runs_the_config(&s));
    let status = s.script(&["--status"], None);
    ok(&status);
    assert!(text(&status.stdout).contains("was interrupted"));

    let out = s.install(&c);
    ok(&out);
    assert!(text(&out.stdout).contains("undid the install"));
    c_is_installed_over_b(&s, &b, &c);
}

/// The name `$PREFIX/bin/rusty-impulse`.
fn bin_name(s: &Sandbox) -> PathBuf {
    s.prefix().join("bin/rusty-impulse")
}

/// Makes the name of the binary a link to `target`, as someone who runs a build from where
/// it was built does.
fn link_bin_to(s: &Sandbox, target: &Path) {
    std::fs::create_dir_all(s.prefix().join("bin")).unwrap();
    let _ = std::fs::remove_file(bin_name(s));
    std::os::unix::fs::symlink(target, bin_name(s)).unwrap();
}

/// `$PREFIX/bin/rusty-impulse` is a link to a binary built elsewhere, not
/// one an install made. That binary is kept as a release with the config it ran, like a plain
/// file installed before releases existed, and a rollback goes back to it.
#[test]
fn a_binary_the_name_links_to_is_kept_and_rolled_back_to() {
    let s = Sandbox::new("foreign-link");
    write_tree(&s.config(), "L");
    let config = listing(&s.config());
    let theirs = s.fake("built-elsewhere", "L");
    link_bin_to(&s, &theirs);
    assert!(s.starts());
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert!(s.starts());
    let previous = s
        .release("previous")
        .expect("the binary that ran was not kept");
    assert_eq!(
        std::fs::read(previous.join("rusty-impulse")).unwrap(),
        std::fs::read(&theirs).unwrap()
    );
    assert_eq!(listing(&previous.join("tree")), config);
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&theirs).unwrap());
    assert_eq!(listing(&s.config()), config);
    assert!(s.starts());
}

/// The same when the install then fails at a step or is signalled there: the name still runs
/// that binary with its config, and the next install installs with it to roll back to.
#[test]
fn a_first_install_over_a_linked_binary_that_fails_leaves_that_binary_running() {
    let hooks = [
        Hook::Fail(TO_A_RELEASE),
        Hook::Fail(TO_THE_SWITCH),
        Hook::Kill(TO_A_RELEASE, "TERM"),
        Hook::Kill(THE_NEW_CONFIG, "TERM"),
        Hook::Kill(TO_A_RELEASE, "KILL"),
        Hook::Kill(THE_NEW_CONFIG, "KILL"),
    ];
    for (i, hook) in hooks.iter().enumerate() {
        let s = Sandbox::new(&format!("foreign-link-fails-{i}"));
        write_tree(&s.config(), "L");
        let config = listing(&s.config());
        let theirs = s.fake("built-elsewhere", "L");
        link_bin_to(&s, &theirs);
        write_tree(&s.source(), "A");
        let a = s.fake("rusty-impulse-a", "A");
        let out = hooked(&s, hook, &[], Some(&a));
        assert!(!out.status.success(), "step {i}: {}", text(&out.stdout));
        assert!(
            s.starts(),
            "step {i}: no rusty-impulse that starts is left (the name: {:?}):\n{}",
            std::fs::read_link(bin_name(&s)),
            text(&out.stderr)
        );
        if !matches!(hook, Hook::Kill(_, "KILL")) {
            // Undone: the name runs the binary that ran before, with the config it ran.
            assert_eq!(s.installed(), std::fs::read(&theirs).unwrap(), "step {i}");
            assert_eq!(listing(&s.config()), config, "step {i}");
            assert_eq!(
                names(s.config().parent().unwrap()),
                ["rusty-impulse"],
                "step {i}"
            );
        }
        ok(&s.install(&a));
        assert_eq!(s.installed(), std::fs::read(&a).unwrap(), "step {i}");
        assert!(s.starts(), "step {i}");
        ok(&s.rollback());
        assert_eq!(s.installed(), std::fs::read(&theirs).unwrap(), "step {i}");
        assert!(s.starts(), "step {i}");
    }
}

/// The name is a link to the file the build writes: a build of this checkout that was run in
/// place. The binary that ran is kept as it was before the build wrote over it.
#[test]
fn a_binary_the_build_writes_over_is_kept_as_it_was() {
    let Some(b) = Building::new("built-in-place") else {
        return;
    };
    let s = &b.s;
    write_tree(&s.source(), "A");
    ok(&b.install());
    let built = s.dir.join("cargo/rusty-impulse");
    let binary_a = std::fs::read(&built).unwrap();
    let config = listing(&s.config());
    ok(&s.script(&["--uninstall"], None));
    link_bin_to(s, &built);
    assert!(s.starts());

    write_tree(&s.source(), "B");
    // Where the script keeps the binary for the time of the build.
    let tmp = s.dir.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    // The copy of the scripts that built A builds B: its commit is named, where this checkout
    // may be unable to tell its own (a worktree whose .git is elsewhere, a copy without one).
    let out = hooked_script(s, &Hook::Fail(TO_A_RELEASE), b.script(), &b.path)
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    refused(&out, "nothing was installed");
    assert_ne!(
        std::fs::read(&built).unwrap(),
        binary_a,
        "nothing was built:\n{}",
        text(&out.stderr)
    );
    assert_eq!(s.installed(), binary_a);
    assert_eq!(listing(&s.config()), config);
    assert!(s.starts(), "{}", text(&out.stderr));
    assert_eq!(names(&tmp), [""; 0]);

    ok(&b.install());
    assert_eq!(s.installed(), std::fs::read(&built).unwrap());
    assert!(s.starts());
    ok(&s.rollback());
    assert_eq!(s.installed(), binary_a);
    assert_eq!(listing(&s.config()), config);
    assert!(s.starts());
}

/// A link that leads to no binary has nothing to keep. An install that fails or is signalled
/// puts the link back as it was.
#[test]
fn a_failed_first_install_puts_back_a_link_that_led_nowhere() {
    let hooks = [
        Hook::Fail(TO_A_RELEASE),
        Hook::Kill(TO_A_RELEASE, "TERM"),
        Hook::Kill(TO_A_RELEASE, "KILL"),
    ];
    for (i, hook) in hooks.iter().enumerate() {
        let s = Sandbox::new(&format!("dangling-link-{i}"));
        let nowhere = s.dir.join("gone/rusty-impulse");
        link_bin_to(&s, &nowhere);
        write_tree(&s.source(), "A");
        let a = s.fake("rusty-impulse-a", "A");
        let out = hooked(&s, hook, &[], Some(&a));
        assert!(!out.status.success(), "step {i}: {}", text(&out.stdout));
        if matches!(hook, Hook::Kill(_, "KILL")) {
            // The config is the new release's: the next run finishes the install.
            ok(&s.script(&["--status"], None));
            ok(&s.install(&a));
            assert!(s.starts(), "step {i}");
            continue;
        }
        assert_eq!(
            std::fs::read_link(bin_name(&s)).ok(),
            Some(nowhere),
            "step {i}: {}",
            text(&out.stderr)
        );
        assert_eq!(s.releases(), [""; 0], "step {i}");
        assert!(!s.dir.join("config").exists(), "step {i}");
    }
}

/// A plain file put at the name after an install (a build copied there by hand) is kept by a
/// rollback too: one that fails leaves it running, and a second rollback goes back to it.
#[test]
fn a_rollback_keeps_a_binary_put_at_the_name_since_the_install() {
    let s = Sandbox::new("rollback-foreign");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    write_tree(&s.source(), "B");
    ok(&s.install(&s.fake("rusty-impulse-b", "B")));
    let copied = s.fake("copied-by-hand", "B");
    std::fs::remove_file(bin_name(&s)).unwrap();
    std::fs::copy(&copied, bin_name(&s)).unwrap();
    assert!(s.starts());

    // The link to the older release fails; the one back to the kept binary does not.
    let out = hooked(&s, &Hook::Fail(TO_A_RELEASE), &["--rollback"], None);
    refused(&out, "nothing changed");
    assert_eq!(s.installed(), std::fs::read(&copied).unwrap());
    assert!(s.starts(), "{}", text(&out.stderr));

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert!(s.starts());
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&copied).unwrap());
    assert!(s.starts());
}

/// The previous release put back by hand after an install (a watcher's rollback: its binary
/// copied to the name as a plain file, its tree to the config) is that release: a rollback
/// stores no second copy of it and goes forward to the newer release, and the next one comes
/// back.
#[test]
fn a_rollback_after_the_previous_release_was_put_back_by_hand_goes_to_the_newer_one() {
    let s = Sandbox::new("rollback-by-hand");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let tree_a = listing(&s.config());
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    let tree_b = listing(&s.config());
    let (release_a, release_b) = (
        s.release("previous").unwrap(),
        s.release("current").unwrap(),
    );
    std::fs::remove_file(bin_name(&s)).unwrap();
    std::fs::copy(&a, bin_name(&s)).unwrap();
    write_tree(&s.config(), "A");
    assert!(s.starts());

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    assert_eq!(listing(&s.config()), tree_b);
    assert_eq!(s.release("current").unwrap(), release_b);
    assert_eq!(s.release("previous").unwrap(), release_a);
    assert_eq!(s.releases().len(), 2, "{:?}", s.releases());
    assert!(
        std::fs::symlink_metadata(bin_name(&s))
            .unwrap()
            .is_symlink()
    );
    assert!(s.starts());

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), tree_a);
    assert_eq!(s.release("current").unwrap(), release_a);
    assert_eq!(s.release("previous").unwrap(), release_b);
    assert!(s.starts());
}

/// That rollback first makes the links say what runs, in three steps. Killed or failed at
/// any of them, the binary at the name is the one that was there and starts, and the next
/// rollback still goes to the newer release.
#[test]
fn a_rollback_killed_while_it_takes_the_release_put_back_by_hand_still_goes_to_the_newer_one() {
    let steps = [PREVIOUS, TO_A_RELEASE, "ln */.rusty-impulse.new", THE_NAME];
    for signal in SIGNALS {
        for (i, at) in steps.iter().enumerate() {
            let step = format!("SIG{signal} at `{at}`");
            let s = Sandbox::new(&format!("kill-rollback-by-hand-{signal}-{i}"));
            write_tree(&s.source(), "A");
            let a = s.fake("rusty-impulse-a", "A");
            ok(&s.install(&a));
            write_tree(&s.source(), "B");
            let b = s.fake("rusty-impulse-b", "B");
            ok(&s.install(&b));
            let tree_b = listing(&s.config());
            std::fs::remove_file(bin_name(&s)).unwrap();
            std::fs::copy(&a, bin_name(&s)).unwrap();
            write_tree(&s.config(), "A");

            let out = hooked(&s, &Hook::Kill(at, signal), &["--rollback"], None);
            was_killed(&s, &out, signal);
            assert_eq!(s.installed(), std::fs::read(&a).unwrap(), "{step}");
            assert!(s.starts(), "{step}");

            ok(&s.rollback());
            assert_eq!(s.installed(), std::fs::read(&b).unwrap(), "{step}");
            assert_eq!(listing(&s.config()), tree_b, "{step}");
            assert!(s.starts(), "{step}");
            assert_ne!(s.release("previous"), s.release("current"), "{step}");
        }
    }
}

/// The same binary at the name with a config that is not that release's tree is not the
/// release put back: it is kept as a release of its own, as any binary put there by hand.
#[test]
fn a_copy_of_the_previous_binary_with_another_config_is_kept_as_a_release() {
    let s = Sandbox::new("rollback-by-hand-other-config");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    write_tree(&s.source(), "B");
    ok(&s.install(&s.fake("rusty-impulse-b", "B")));
    let release_a = s.release("previous").unwrap();
    std::fs::remove_file(bin_name(&s)).unwrap();
    std::fs::copy(&a, bin_name(&s)).unwrap();

    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(s.release("current").unwrap(), release_a);
    assert_eq!(s.releases().len(), 3, "{:?}", s.releases());
}

/// The user's own Quickshell config, `shell.qml` right in the quickshell directory.
fn their_shell_qml(s: &Sandbox) -> PathBuf {
    s.dir.join("config/rustshell/shell.qml")
}

/// A `shell.qml` of the user's own in `~/.config/rustshell` is
/// their default config, and with it there neither Quickshell nor rusty-impulse looks up a named
/// config in that directory: `rusty-impulse -c rusty-impulse`, how Hyprland starts the shell, would not find the
/// rusty-impulse config. A first install says so and installs nothing; with the file moved to where
/// Quickshell also finds a default config, it installs.
#[test]
fn a_first_install_the_shell_would_not_start_from_installs_nothing() {
    let s = Sandbox::new("own-config-first");
    std::fs::create_dir_all(their_shell_qml(&s).parent().unwrap()).unwrap();
    std::fs::write(their_shell_qml(&s), "// mine\n").unwrap();
    let config_dir = listing(&s.dir.join("config"));
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    let out = s.install(&a);
    refused(
        &out,
        "would not start as `rusty-impulse -c rusty-impulse`; nothing was installed",
    );
    let err = text(&out.stderr);
    assert!(
        err.contains(&format!("{} is a config", their_shell_qml(&s).display())),
        "{err}"
    );
    assert_eq!(listing(&s.dir.join("config")), config_dir);
    assert_eq!(names(&s.prefix().join("bin")), [""; 0]);
    assert_eq!(s.releases(), [""; 0]);
    assert_eq!(s.release("current"), None);
    assert!(!s.state().exists(), "{:?}", listing(&s.state()));

    let default = s.dir.join("config/rustshell/default");
    std::fs::create_dir(&default).unwrap();
    std::fs::rename(their_shell_qml(&s), default.join("shell.qml")).unwrap();
    ok(&s.install(&a));
    assert!(s.starts());
    assert_eq!(
        std::fs::read_to_string(default.join("shell.qml")).unwrap(),
        "// mine\n"
    );
}

/// The same once a release is installed: an install, a rollback and an install of the
/// installed build change nothing and say why, and `--status` says it too.
#[test]
fn an_install_or_rollback_the_shell_would_not_start_from_changes_nothing() {
    let s = Sandbox::new("own-config");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    ok(&s.script(&["--status"], None));
    std::fs::write(their_shell_qml(&s), "// mine\n").unwrap();
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let before = (listing(&s.prefix()), listing(&s.dir.join("config")));
    const WHY: &str = "would not start as `rusty-impulse -c rusty-impulse`";

    write_tree(&s.source(), "C");
    refused(
        &s.install(&s.fake("rusty-impulse-c", "C")),
        &format!("{WHY}; nothing was installed"),
    );
    let after = (listing(&s.prefix()), listing(&s.dir.join("config")));
    assert_eq!(after, before);
    assert_eq!(kept(&s), [""; 0]);

    refused(&s.rollback(), &format!("{WHY}; nothing changed"));
    let after = (listing(&s.prefix()), listing(&s.dir.join("config")));
    assert_eq!(after, before);
    assert_eq!(kept(&s), [""; 0]);

    // The installed build again: neither the config (but for the note) nor a link changes.
    std::fs::remove_file(s.config().join("note.txt")).unwrap();
    let before = (listing(&s.prefix()), listing(&s.dir.join("config")));
    write_tree(&s.source(), "B");
    refused(&s.install(&b), &format!("{WHY}; nothing changed"));
    let after = (listing(&s.prefix()), listing(&s.dir.join("config")));
    assert_eq!(after, before);

    let status = s.script(&["--status"], None);
    assert_eq!(status.status.code(), Some(1), "{}", text(&status.stdout));
    let said = text(&status.stderr);
    assert!(
        said.contains("does not run the rusty-impulse config"),
        "{said}"
    );

    std::fs::remove_file(their_shell_qml(&s)).unwrap();
    ok(&s.script(&["--status"], None));
    ok(&s.rollback());
    assert!(s.starts());
}

/// The real binary finds the config named rusty-impulse as the stand-ins of these tests do: in
/// `$XDG_CONFIG_HOME/rustshell/rusty-impulse`, and not when that directory has a `shell.qml` of its own,
/// nor through a relative `XDG_CONFIG_HOME`, which it ignores.
#[test]
fn the_shell_finds_the_rusty_impulse_config_as_it_is_started() {
    let s = Sandbox::new("real-lookup");
    let asked = |config_home: &Path, cwd: &Path| {
        Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .args(["-c", "rusty-impulse", "--check-tree"])
            .current_dir(cwd)
            .env("HOME", s.dir.join("home"))
            .env("XDG_CONFIG_HOME", config_home)
            .env_remove("XDG_CONFIG_DIRS")
            .output()
            .unwrap()
    };
    let none = asked(&s.dir.join("config"), &s.dir);
    assert_eq!(none.status.code(), Some(255), "{}", text(&none.stderr));
    std::fs::create_dir_all(s.config().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(ricompiled::ROOT, s.config()).unwrap();
    let found = asked(&s.dir.join("config"), &s.dir);
    assert!(found.status.success(), "{}", text(&found.stderr));
    let tree = std::fs::canonicalize(ricompiled::ROOT).unwrap();
    assert_eq!(
        text(&found.stdout),
        format!(
            "rusty-impulse: {} has the scripts this binary was compiled from\n",
            tree.display()
        ),
        "tools/install.sh compares this line"
    );

    std::fs::write(their_shell_qml(&s), "// mine\n").unwrap();
    let hidden = asked(&s.dir.join("config"), &s.dir);
    assert_eq!(hidden.status.code(), Some(255), "{}", text(&hidden.stdout));
    let err = text(&hidden.stderr);
    assert!(
        err.contains("Could not find \"rusty-impulse\" config directory"),
        "{err}"
    );
    std::fs::remove_file(their_shell_qml(&s)).unwrap();

    // A relative XDG_CONFIG_HOME names this very directory from the parent of `config`, and is
    // not used: the config is looked up under $HOME/.config, where there is none.
    let relative = asked(Path::new("config"), &s.dir);
    assert_eq!(
        relative.status.code(),
        Some(255),
        "{}",
        text(&relative.stdout)
    );
    let home_config = s.dir.join("home/.config/rustshell");
    std::fs::create_dir_all(&home_config).unwrap();
    std::fs::rename(s.config(), home_config.join("rusty-impulse")).unwrap();
    let relative = asked(Path::new("config"), &s.dir);
    assert!(relative.status.success(), "{}", text(&relative.stderr));
}

/// A relative `XDG_CONFIG_HOME` or `XDG_STATE_HOME` is not valid
/// (XDG Base Directory Specification) and rusty-impulse ignores it. The script does too, and says so:
/// the config is made where rusty-impulse looks for it, not in a directory under the one the script
/// was started in.
#[test]
fn relative_xdg_directories_are_ignored_as_the_shell_ignores_them() {
    let s = Sandbox::new("relative-xdg");
    let cwd = s.dir.join("cwd");
    std::fs::create_dir(&cwd).unwrap();
    let relative = |cmd: &mut Command| {
        cmd.current_dir(&cwd)
            .env("XDG_CONFIG_HOME", "rel")
            .env("XDG_STATE_HOME", "rel-state");
    };
    let install = |bin: &Path| {
        let mut cmd = s.command(&[]);
        relative(&mut cmd);
        cmd.env("BIN", bin).output().unwrap()
    };
    let config = s.dir.join("home/.config/rustshell/rusty-impulse");
    write_tree(&s.source(), "A");
    let out = install(&s.fake("rusty-impulse-a", "A"));
    ok(&out);
    let err = text(&out.stderr);
    for var in ["XDG_CONFIG_HOME=rel ", "XDG_STATE_HOME=rel-state "] {
        assert!(
            err.contains(&format!("{var}is not an absolute path")),
            "{err}"
        );
    }
    assert_eq!(listing(&config), listing(&s.source()));
    let mut started = s.installed_command(&["-c", "rusty-impulse", "--check-tree"]);
    relative(&mut started);
    let started = started.output().unwrap();
    assert!(started.status.success(), "{}", text(&started.stderr));

    // An edited config is kept in the state directory rusty-impulse would use.
    std::fs::write(config.join("note.txt"), "mine").unwrap();
    write_tree(&s.source(), "B");
    ok(&install(&s.fake("rusty-impulse-b", "B")));
    assert_eq!(
        names(&s.dir.join("home/.local/state/rusty-impulse")).len(),
        1
    );
    assert_eq!(names(&cwd), [""; 0], "made under the starting directory");

    let mut status = s.command(&["--status"]);
    relative(&mut status);
    let status = status.output().unwrap();
    ok(&status);
    let said = text(&status.stdout);
    assert!(
        said.contains(&format!(
            "rusty-impulse config: {} is the tree",
            config.display()
        )),
        "{said}"
    );
}

/// The lines of [`steps_log`].
fn steps(s: &Sandbox) -> Vec<String> {
    let log = std::fs::read_to_string(steps_log(s)).unwrap();
    log.lines().map(str::to_string).collect()
}

/// The first step from `from` on that `is` holds for.
#[track_caller]
fn step_at(steps: &[String], from: usize, what: &str, is: impl Fn(&str) -> bool) -> usize {
    match steps[from..].iter().position(|l| is(l)) {
        Some(i) => from + i,
        None => panic!("no step `{what}` from {from} on in:\n{}", steps.join("\n")),
    }
}

/// Whether one of `steps` has the file system of a path under `dir` written to the disk.
fn flushes(steps: &[String], dir: &Path) -> bool {
    let dir = dir.display().to_string();
    steps.iter().any(|l| {
        let mut words = l.split(' ');
        words.next() == Some("sync")
            && words.next() == Some("-f")
            && words.any(|w| w.starts_with(&dir))
    })
}

/// `$PREFIX` and the config need not be on one file system, and
/// after a power cut one file system must not be ahead of the other. So each step of a switch
/// is written to the disk before the next one on the other side is made: `current` names the
/// switch before the config is replaced, and the config is replaced before a link names the
/// new release. The same on the way back, when the last link cannot be switched, and when the
/// next run finishes a switch that was killed.
#[test]
fn each_step_of_a_switch_is_on_the_disk_before_the_next() {
    let s = Sandbox::new("flushed");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    let (prefix, config_dir) = (s.prefix(), s.dir.join("config"));
    let current = prefix
        .join("lib/rusty-impulse/current")
        .display()
        .to_string();
    let names_current = |l: &str| l.starts_with("mv -Tf ") && l.ends_with(&current);
    let config = s.config().display().to_string();
    let config_in_place = |l: &str| l.starts_with("mv -T ") && l.ends_with(&config);
    let links_a_release = |l: &str| l.starts_with("ln -sfn releases/") && !l.contains(".switch");

    // An install.
    ok(&hooked(&s, &Hook::Log, &[], Some(&b)));
    let log = steps(&s);
    let switch = step_at(&log, 0, "current names the switch", names_current);
    let replaced = step_at(&log, switch, "the new config", config_in_place);
    let linked = step_at(&log, replaced, "a link to a release", links_a_release);
    assert!(
        flushes(&log[switch..replaced], &prefix),
        "the config is replaced before `current` names the switch on the disk:\n{}",
        log.join("\n")
    );
    assert!(
        flushes(&log[replaced..linked], &config_dir),
        "a link names the new release before the config is replaced on the disk:\n{}",
        log.join("\n")
    );

    // One that cannot switch its last link: the config goes back, then the links.
    write_tree(&s.source(), "C");
    let c = s.fake("rusty-impulse-c", "C");
    let out = without_the_link(&s, ".previous.new", &[], Some(&c));
    refused(&out, "nothing was installed");
    let log = steps(&s);
    let failed = step_at(&log, 0, "the link that fails", |l| {
        links_a_release(l) && l.ends_with("/.previous.new")
    });
    let back = step_at(&log, failed, "the config that was there", config_in_place);
    let linked = step_at(&log, back, "current as it was", names_current);
    assert!(
        flushes(&log[back..linked], &config_dir),
        "the links are put back before the config is back on the disk:\n{}",
        log.join("\n")
    );

    // One killed after it replaced the config: the next run finishes it.
    let out = hooked(&s, &Hook::Kill(PREVIOUS, "KILL"), &[], Some(&c));
    was_killed(&s, &out, "KILL");
    let out = hooked(&s, &Hook::Log, &[], Some(&c));
    ok(&out);
    assert!(text(&out.stdout).contains("finished the install"));
    let log = steps(&s);
    let linked = step_at(&log, 0, "a link to a release", links_a_release);
    assert!(
        flushes(&log[..linked], &config_dir),
        "the next run names the release before the config is on the disk:\n{}",
        log.join("\n")
    );
    assert!(s.starts());
}

/// A binary kept as a release is on the disk before `current` names it, and
/// `current` is before the name of the binary leads to it.
#[test]
fn a_kept_binary_is_on_the_disk_before_the_name_leads_to_it() {
    let s = Sandbox::new("flushed-kept");
    write_tree(&s.config(), "L");
    link_bin_to(&s, &s.fake("built-elsewhere", "L"));
    write_tree(&s.source(), "A");
    ok(&hooked(
        &s,
        &Hook::Log,
        &[],
        Some(&s.fake("rusty-impulse-a", "A")),
    ));
    let log = steps(&s);
    let lib = s.lib();
    let stored = step_at(&log, 0, "the kept binary's release", |l| {
        l.starts_with("mv -T ") && l.contains("/releases/legacy-")
    });
    let current = step_at(&log, stored, "current names it", |l| {
        l.starts_with("mv -Tf ") && l.ends_with("/current")
    });
    let name = step_at(&log, current, "the name of the binary", |l| {
        l.starts_with("mv -Tf ") && l.ends_with("/bin/rusty-impulse")
    });
    assert!(flushes(&log[stored..current], &lib), "{}", log.join("\n"));
    assert!(flushes(&log[current..name], &lib), "{}", log.join("\n"));
}

/// The releases A then B are installed and the config is edited since: what the tests of an
/// install of C that is interrupted start from. Returns the binaries of A and B.
fn b_is_installed_and_its_config_edited(s: &Sandbox) -> (PathBuf, PathBuf) {
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&s.install(&b));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    (a, b)
}

/// What [`listing`] returns.
type Listing = Vec<(PathBuf, Vec<u8>)>;

/// The prefix and the config directory: what a run that changes nothing leaves as it was.
fn installed_state(s: &Sandbox) -> (Listing, Listing) {
    (listing(&s.prefix()), listing(&s.dir.join("config")))
}

fn set_mode(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Whether its owner may run the file.
fn executable(p: &Path) -> bool {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o100 != 0
}

/// The rusty-impulse config is a copy of the tree with the files' executable bits.
/// A tree that differs from the installed one only in that (a commit that makes a script
/// executable, or takes that away) is installed: the same binary with another tree. --status
/// names a file of the config whose mode alone is not the release's.
#[test]
fn a_tree_that_differs_only_in_a_file_mode_is_installed() {
    let s = Sandbox::new("mode-only");
    write_tree(&s.source(), "A");
    let script = Path::new("modules/run.sh");
    std::fs::write(s.source().join(script), "#!/bin/sh\n").unwrap();
    set_mode(&s.source().join(script), 0o644);
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let b = s.fake("rusty-impulse-b", "A");
    ok(&s.install(&b));
    assert!(!executable(&s.config().join(script)));

    for now in [true, false] {
        set_mode(&s.source().join(script), if now { 0o755 } else { 0o644 });
        let out = s.install(&b);
        ok(&out);
        let said = text(&out.stdout);
        assert!(!said.contains("is already this build"), "{said}");
        assert_eq!(executable(&s.config().join(script)), now);
        let current = s.release("current").unwrap();
        assert_eq!(executable(&current.join("tree").join(script)), now);
        // The same binary: the release to roll back to stays the one before it.
        let previous = s.release("previous").unwrap().join("rusty-impulse");
        assert_eq!(std::fs::read(previous).unwrap(), std::fs::read(&a).unwrap());
        assert_eq!(s.releases().len(), 2, "{:?}", s.releases());
        let status = s.script(&["--status"], None);
        ok(&status);
        let status = text(&status.stdout);
        assert!(status.contains("is the tree stored with"), "{status}");
        // The config was the release's tree, not an edited one: no copy of it is kept.
        assert!(!s.state().exists(), "{:?}", listing(&s.state()));
        let before = listing(&s.prefix());
        let again = s.install(&b);
        ok(&again);
        assert!(text(&again.stdout).contains("is already this build"));
        assert_eq!(listing(&s.prefix()), before);
    }

    set_mode(&s.config().join(script), 0o755);
    let status = s.script(&["--status"], None);
    ok(&status);
    let status = text(&status.stdout);
    assert!(
        status.contains("differs from the tree stored with"),
        "{status}"
    );
    let named = "Executable in only one of the two: ./modules/run.sh";
    assert!(status.contains(named), "{status}");
}

/// On a signal the script undoes what it has switched, also when it
/// cannot print. SIGHUP: the terminal was closed, and writing to it fails (/dev/full fails
/// every write). SIGINT, SIGTERM: what read the script's output through a pipe is gone too
/// (Ctrl-C ended both), and a write to that pipe is SIGPIPE.
#[test]
fn a_signal_undoes_the_switch_when_nothing_can_be_printed() {
    for (signal, closed_pipe) in [("HUP", false), ("INT", true), ("TERM", true)] {
        for (i, at) in [PREVIOUS, TO_A_RELEASE].iter().enumerate() {
            let step = format!("SIG{signal} at `{at}`");
            let s = Sandbox::new(&format!("mute-{signal}-{i}"));
            b_is_installed_and_its_config_edited(&s);
            let before = installed_state(&s);
            write_tree(&s.source(), "C");
            let c = s.fake("rusty-impulse-c", "C");
            let path = std::env::var("PATH").unwrap();
            let mut cmd = hooked_command(&s, &Hook::Kill(at, signal), &[], &path);
            cmd.env("BIN", &c);
            if closed_pipe {
                let (reader, writer) = std::io::pipe().unwrap();
                drop(reader);
                cmd.stdout(writer.try_clone().unwrap()).stderr(writer);
            } else {
                let full = || File::options().write(true).open("/dev/full").unwrap();
                cmd.stdout(full()).stderr(full());
            }
            let status = cmd.status().unwrap();
            assert!(killed(&s).exists(), "{step}: the script never got there");
            assert_eq!(status.code(), Some(1), "{step}");
            assert_eq!(installed_state(&s), before, "{step}");
            assert_eq!(kept(&s), [""; 0], "{step}");
            assert!(runs_the_config(&s), "{step}");
        }
    }
}

/// The copy kept of an edited config holds what the config's directory
/// held, a link as the link: one that leads nowhere does not stop an install or a rollback.
#[test]
fn an_edited_config_with_a_link_that_leads_nowhere_is_kept_and_replaced() {
    let s = Sandbox::new("dangling");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let add_links = || {
        let link = std::os::unix::fs::symlink;
        link("/nonexistent/wallpaper.png", s.config().join("wallpaper")).unwrap();
        link("modules/Bar.qml", s.config().join("bar")).unwrap();
        listing(&s.config())
    };
    let copy = |name: &String| listing(&s.state().join("rusty-impulse").join(name));

    let edited = add_links();
    write_tree(&s.source(), "B");
    ok(&s.install(&s.fake("rusty-impulse-b", "B")));
    assert_eq!(listing(&s.config()), listing(&s.source()));
    let first = kept(&s);
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(copy(&first[0]), edited);

    let edited = add_links();
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    let current = s.release("current").unwrap();
    assert_eq!(listing(&s.config()), listing(&current.join("tree")));
    let both = kept(&s);
    assert_eq!(both.len(), 2, "{both:?}");
    let second = both.iter().find(|name| **name != first[0]).unwrap();
    assert_eq!(copy(second), edited);
    assert_eq!(names(s.config().parent().unwrap()), ["rusty-impulse"]);
}

/// replaced-* in the state directory are the configs that were replaced.
/// A run that is interrupted while it copies the edited config leaves none: on a signal it
/// handles nothing is left, and what SIGKILL leaves is not counted by --status and is
/// removed by the next run.
#[test]
fn a_signal_while_the_edited_config_is_copied_leaves_no_copy() {
    for signal in SIGNALS {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("kill-copy-{signal}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        let before = installed_state(&s);
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Kill(THE_EDITED_CONFIG, signal), &[], Some(&c));
        was_killed(&s, &out, signal);
        assert!(runs_the_config(&s), "{step}");
        if signal != "KILL" {
            assert_eq!(installed_state(&s), before, "{step}");
            assert_eq!(kept(&s), [""; 0], "{step}");
        }
        let status = s.script(&["--status"], None);
        ok(&status);
        let status = text(&status.stdout);
        assert!(
            !status.contains("replaced rusty-impulse configs"),
            "{step}: {status}"
        );

        ok(&s.install(&c));
        c_is_installed_over_b(&s, &b, &c);
    }
}

/// The copy of the edited config gets its name when the config is replaced.
const THE_COPYS_NAME: &str =
    "mv -T */state/rusty-impulse/.replaced-* */state/rusty-impulse/replaced-*";

/// The run that finishes a killed install says where the config that
/// install replaced is kept: the killed one did not get to.
#[test]
fn the_run_after_a_killed_install_says_where_the_edited_config_is_kept() {
    for (i, at) in [PREVIOUS, THE_COPYS_NAME].iter().enumerate() {
        let s = Sandbox::new(&format!("kept-said-{i}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        let edited = listing(&s.config());
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Kill(at, "KILL"), &[], Some(&c));
        was_killed(&s, &out, "KILL");

        let out = s.install(&c);
        ok(&out);
        c_is_installed_over_b(&s, &b, &c);
        let copy = s.state().join("rusty-impulse").join(&kept(&s)[0]);
        assert_eq!(listing(&copy), edited, "`{at}`");
        let said = text(&out.stdout);
        assert!(said.contains("finished the install"), "`{at}`: {said}");
        let wher = format!("had changes since its install: kept at {}", copy.display());
        assert!(said.contains(&wher), "`{at}`: {said}");
    }
}

/// --status tells an install that is switching releases at
/// that moment (it holds the lock) from one that was interrupted. The install waits at its
/// last link until --status has answered.
#[test]
fn status_tells_a_running_switch_from_an_interrupted_one() {
    let s = Sandbox::new("status-running");
    b_is_installed_and_its_config_edited(&s);
    write_tree(&s.source(), "C");
    let c = s.fake("rusty-impulse-c", "C");
    let path = std::env::var("PATH").unwrap();
    let mut running = hooked_command(&s, &Hook::Pause(TO_A_RELEASE), &[], &path)
        .env("BIN", &c)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The script's output up to the stand-in's line, or all of it if the script ended.
    let mut said = BufReader::new(running.stdout.take().unwrap());
    let mut line = String::new();
    while !line.ends_with(&format!("{WAITING}\n")) && said.read_line(&mut line).unwrap() > 0 {}
    let at_its_last_link = line.ends_with(&format!("{WAITING}\n"));
    let status = at_its_last_link.then(|| s.script(&["--status"], None));
    if at_its_last_link {
        drop(File::options().write(true).open(resume(&s)).unwrap());
    }
    let out = running.wait_with_output().unwrap();
    assert!(at_its_last_link, "{line}{}", text(&out.stderr));
    ok(&out);
    let status = status.unwrap();
    ok(&status);
    let status = text(&status.stdout);
    assert!(status.contains("is switching releases now"), "{status}");
    assert!(!status.contains("was interrupted"), "{status}");

    let status = s.script(&["--status"], None);
    ok(&status);
    let status = text(&status.stdout);
    assert!(status.contains("current:  "), "{status}");
    assert_eq!(s.installed(), std::fs::read(&c).unwrap());
}

/// BIN names a file by any path: one without a slash is the file in the
/// current directory (not a command looked for in $PATH), and a path may hold `=` (here the
/// whole sandbox does, so the stored releases' paths do too).
#[test]
fn a_binary_is_named_by_a_path_of_any_form() {
    let s = Sandbox::new("bin=path");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse", "A");
    let out = s
        .command(&[])
        .env("BIN", "rusty-impulse")
        .current_dir(&s.dir)
        .output()
        .unwrap();
    ok(&out);
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert!(runs_the_config(&s));
    let b = s.fake("rusty-impulse-b", "A");
    let out = s
        .command(&[])
        .env("BIN", "./rusty-impulse-b")
        .current_dir(&s.dir)
        .output()
        .unwrap();
    ok(&out);
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    ok(&s.rollback());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    let status = s.script(&["--status"], None);
    ok(&status);
}

/// --uninstall removes the link an install made, and no rusty-impulse of
/// another origin: it says that it leaves that one.
#[test]
fn uninstall_leaves_a_rusty_impulse_it_did_not_install() {
    let s = Sandbox::new("uninstall-foreign");
    let name = s.prefix().join("bin/rusty-impulse");
    std::fs::create_dir_all(name.parent().unwrap()).unwrap();
    let theirs = s.executable("theirs", "#!/bin/sh\n# built elsewhere\n");

    // Nothing is installed: a file, then a link to a binary built elsewhere.
    std::fs::copy(&theirs, &name).unwrap();
    let before = listing(&s.prefix());
    refused(
        &s.script(&["--uninstall"], None),
        "is not the link an install makes",
    );
    assert_eq!(listing(&s.prefix()), before);
    std::fs::remove_file(&name).unwrap();
    std::os::unix::fs::symlink(&theirs, &name).unwrap();
    let before = listing(&s.prefix());
    refused(
        &s.script(&["--uninstall"], None),
        "is not the link an install makes",
    );
    assert_eq!(listing(&s.prefix()), before);
    std::fs::remove_file(&name).unwrap();

    // Releases are installed and the name was replaced since.
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    std::fs::remove_file(&name).unwrap();
    std::fs::copy(&theirs, &name).unwrap();
    let out = s.script(&["--uninstall"], None);
    ok(&out);
    assert!(!s.lib().exists());
    assert_eq!(
        std::fs::read(&name).unwrap(),
        std::fs::read(&theirs).unwrap()
    );
    let said = text(&out.stdout);
    assert!(
        said.contains(&format!("left: {}", name.display())),
        "{said}"
    );
    assert!(
        !said.contains(&format!("removed {}", name.display())),
        "{said}"
    );
}

/// As root for another user, $HOME and the rusty-impulse config are root's. sudo,
/// doas and pkexec name that user in the environment; after su the login name does
/// (logname). Root with none of them (a login as root) is the user whose shell it is. A
/// stand-in `id` says the script runs as root, a stand-in `logname` gives $RI_TEST_LOGNAME.
#[test]
fn nothing_runs_as_root_for_another_user() {
    let s = Sandbox::new("root");
    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&s.install(&a));
    let before = (installed_state(&s), listing(&s.state()));
    let stand_ins = s.dir.join("root-bin");
    std::fs::create_dir(&stand_ins).unwrap();
    s.executable(
        "root-bin/id",
        "#!/bin/sh\nif [ \"$1\" = -un ]; then echo \"user$2\"; else echo 0; fi\n",
    );
    s.executable(
        "root-bin/logname",
        "#!/bin/sh\n[ -n \"$RI_TEST_LOGNAME\" ] || { echo 'logname: no login name' >&2; exit 1; }\n\
         echo \"$RI_TEST_LOGNAME\"\n",
    );
    let path = format!("{}:{}", stand_ins.display(), std::env::var("PATH").unwrap());
    let as_root = |args: &[&str], var: &str, value: &str| {
        let mut cmd = s.command(args);
        cmd.env("BIN", &a).env("PATH", &path);
        for other in ["SUDO_USER", "DOAS_USER", "PKEXEC_UID", "RI_TEST_LOGNAME"] {
            cmd.env_remove(other);
        }
        cmd.env(var, value).output().unwrap()
    };
    let cases = [
        (
            "SUDO_USER",
            "someone",
            "run it as someone, not through sudo",
        ),
        (
            "DOAS_USER",
            "someone",
            "run it as someone, not through doas",
        ),
        (
            "PKEXEC_UID",
            "1000",
            "run it as user1000, not through pkexec",
        ),
        (
            "RI_TEST_LOGNAME",
            "someone",
            "run it as someone, not as root",
        ),
    ];
    for (var, value, why) in cases {
        for args in [&[][..], &["--rollback"], &["--status"], &["--uninstall"]] {
            refused(&as_root(args, var, value), why);
        }
    }
    assert_eq!((installed_state(&s), listing(&s.state())), before);
    // Root's own shell: no other user is named.
    for (var, value) in [
        ("SUDO_USER", "root"),
        ("RI_TEST_LOGNAME", "root"),
        ("RI_TEST_LOGNAME", ""),
    ] {
        ok(&as_root(&["--status"], var, value));
    }
}

/// NOBUILD=1 installs the build that is there. With a target named in
/// Cargo's configuration (build.target) the build is in <target>/release of the target
/// directory (Cargo Book, Build cache). Of two builds the script does not pick one.
#[test]
fn nobuild_installs_the_build_of_a_named_target() {
    let Some(b) = Building::new("nobuild-target") else {
        return;
    };
    let s = &b.s;
    write_tree(&s.source(), "A");
    let target = s.dir.join("cargo/target");
    let named = target.join("x86_64-unknown-linux-gnu/release");
    std::fs::create_dir_all(&named).unwrap();
    std::fs::create_dir_all(target.join("release")).unwrap();
    std::fs::create_dir_all(target.join("debug")).unwrap();
    let nobuild = || b.install_with(&[("NOBUILD", "1")]);

    refused(&nobuild(), "no rusty-impulse binary at");
    let built = s.fake("rusty-impulse-named", "A");
    std::fs::copy(&built, named.join("rusty-impulse")).unwrap();
    ok(&nobuild());
    assert_eq!(b.calls(), "metadata\nmetadata\n");
    assert_eq!(s.installed(), std::fs::read(&built).unwrap());

    let host = s.fake("rusty-impulse-host", "A");
    std::fs::copy(&host, target.join("release/rusty-impulse")).unwrap();
    let before = listing(&s.prefix());
    refused(&nobuild(), "more than one build of rusty-impulse");
    assert_eq!(listing(&s.prefix()), before);

    std::fs::remove_file(named.join("rusty-impulse")).unwrap();
    ok(&nobuild());
    assert_eq!(s.installed(), std::fs::read(&host).unwrap());
}

/// A signal while the new release is stored, before anything is
/// switched: the installed release and its config stay, and the next install installs.
#[test]
fn a_signal_while_the_release_is_stored_changes_nothing() {
    for signal in SIGNALS {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("kill-store-{signal}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        let before = installed_state(&s);
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Kill(THE_RELEASE, signal), &[], Some(&c));
        was_killed(&s, &out, signal);
        assert_eq!(s.installed(), std::fs::read(&b).unwrap(), "{step}");
        assert_eq!(listing(&s.dir.join("config")), before.1, "{step}");
        assert!(runs_the_config(&s), "{step}");
        if signal != "KILL" {
            assert_eq!(installed_state(&s), before, "{step}");
        }
        assert_eq!(kept(&s), [""; 0], "{step}");

        ok(&s.install(&c));
        c_is_installed_over_b(&s, &b, &c);
    }
}

/// The same signal sent to the script alone (`kill PID`): the tar that reads the tree does not
/// get it and goes on writing, more than a pipe holds. Bash can run the script's handler when
/// that tar runs and the tar that reads the pipe does not yet; the handler then waited for the
/// first tar, which waited for a reader. The script ends, and nothing has changed.
#[test]
fn a_signal_to_the_script_alone_while_the_release_is_stored_changes_nothing() {
    for signal in &SIGNALS[1..] {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("signal-store-{signal}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        let before = installed_state(&s);
        write_tree(&s.source(), "C");
        std::fs::write(s.source().join("modules/Big.qml"), vec![b'/'; 1 << 20]).unwrap();
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Signal(THE_RELEASE, signal), &[], Some(&c));
        was_killed(&s, &out, signal);
        assert_eq!(installed_state(&s), before, "{step}");
        assert!(runs_the_config(&s), "{step}");
        assert_eq!(kept(&s), [""; 0], "{step}");

        ok(&s.install(&c));
        c_is_installed_over_b(&s, &b, &c);
    }
}

/// What both tests above do, with `path` as the PATH of the script and its stand-ins:
/// `signal` at the copy into the release, which fails ([`Hook::Kill`]) or goes on
/// ([`Hook::Signal`], with a file larger than a pipe). `install` is an install without a hook.
#[track_caller]
fn a_signal_at_the_copy_changes_nothing(s: &Sandbox, path: &str, signal: &str, goes_on: bool) {
    let step = format!("SIG{signal}, the copy goes on: {goes_on}");
    let install = |hook: &Hook, bin: &Path| {
        hooked_command(s, hook, &[], path)
            .env("BIN", bin)
            .output()
            .unwrap()
    };
    let plain = Hook::Fail("");
    write_tree(&s.source(), "A");
    ok(&install(&plain, &s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&install(&plain, &b));
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    let before = installed_state(s);
    write_tree(&s.source(), "C");
    std::fs::write(s.source().join("modules/Big.qml"), vec![b'/'; 1 << 20]).unwrap();
    let c = s.fake("rusty-impulse-c", "C");
    let hook = if goes_on {
        Hook::Signal(THE_RELEASE, signal)
    } else {
        Hook::Kill(THE_RELEASE, signal)
    };
    let out = install(&hook, &c);
    was_killed(s, &out, signal);
    assert_eq!(installed_state(s), before, "{step}");
    assert!(runs_the_config(s), "{step}");
    assert_eq!(kept(s), [""; 0], "{step}");

    ok(&install(&plain, &c));
    c_is_installed_over_b(s, &b, &c);
}

/// The same two under a prefix the user may not write: the tar that writes the release runs
/// through sudo, and the script could not send it a signal. It does not have to.
#[test]
fn a_signal_while_the_release_is_stored_through_sudo_changes_nothing() {
    use std::os::unix::fs::MetadataExt;
    for goes_on in [false, true] {
        for signal in &SIGNALS[1..] {
            let s = Sandbox::new(&format!("signal-sudo-{signal}-{goes_on}"));
            if std::fs::metadata(&s.dir).unwrap().uid() == 0 {
                testenv::unavailable(
                    "a user other than root: root writes every prefix without sudo",
                );
                return;
            }
            let path = a_prefix_for_sudo(&s);
            a_signal_at_the_copy_changes_nothing(&s, &path, signal, goes_on);
            let calls = std::fs::read_to_string(s.dir.join("sudo/calls")).unwrap();
            assert!(calls.contains("tar -C "), "{calls}");
        }
    }
}

/// The same two with a `pkill` on the PATH that ends nothing and writes down that it was
/// run: the script ends nothing by name or by parent, so it runs no such program. (A script
/// that ran `pkill -P $$` first in its handler left the same state in these runs; that it
/// ran the program is what this test tells it by.)
#[test]
fn a_signal_while_the_release_is_stored_changes_nothing_and_runs_no_pkill() {
    for goes_on in [false, true] {
        for signal in &SIGNALS[1..] {
            let s = Sandbox::new(&format!("signal-no-pkill-{signal}-{goes_on}"));
            let tools = s.dir.join("tools");
            std::fs::create_dir(&tools).unwrap();
            let run = tools.join("run");
            let pkill = format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", run.display());
            s.executable("tools/pkill", &pkill);
            let path = format!("{}:{}", tools.display(), std::env::var("PATH").unwrap());
            a_signal_at_the_copy_changes_nothing(&s, &path, signal, goes_on);
            let run = std::fs::read_to_string(&run).unwrap_or_default();
            assert_eq!(run, "", "SIG{signal}, the copy goes on: {goes_on}");
        }
    }
}

/// A signal while a binary from before releases gives its name to the
/// link to its release: the name still runs that binary, the next install installs, and a
/// rollback goes back to it.
#[test]
fn a_signal_while_a_binary_from_before_releases_becomes_a_release_keeps_it() {
    for signal in SIGNALS {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("kill-legacy-{signal}"));
        write_tree(&s.config(), "L");
        write_tree(&s.source(), "A");
        let config = listing(&s.config());
        let legacy = "#!/bin/sh\n# legacy\n";
        std::fs::create_dir_all(s.prefix().join("bin")).unwrap();
        let installed = s.prefix().join("bin/rusty-impulse");
        std::fs::write(&installed, legacy).unwrap();
        set_mode(&installed, 0o755);
        let a = s.fake("rusty-impulse-a", "A");
        let out = hooked(&s, &Hook::Kill(THE_NAME, signal), &[], Some(&a));
        was_killed(&s, &out, signal);
        assert_eq!(s.installed(), legacy.as_bytes(), "{step}");
        assert_eq!(listing(&s.config()), config, "{step}");
        assert_eq!(kept(&s), [""; 0], "{step}");

        ok(&s.install(&a));
        assert_eq!(s.installed(), std::fs::read(&a).unwrap(), "{step}");
        assert_eq!(listing(&s.config()), listing(&s.source()), "{step}");
        assert_eq!(s.releases().len(), 2, "{step}: {:?}", s.releases());
        ok(&s.rollback());
        assert_eq!(s.installed(), legacy.as_bytes(), "{step}");
        assert_eq!(listing(&s.config()), config, "{step}");
    }
}

/// A signal during the undo. The last link cannot be switched (and
/// cannot be for the rest of that run), and the signal comes while the config is put back:
/// the shell starts, and the next install undoes the rest and installs.
#[test]
fn a_signal_while_a_failed_switch_is_undone_leaves_a_shell_that_starts() {
    for signal in SIGNALS {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("kill-undo-{signal}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let hook = Hook::FailThenKill(TO_A_RELEASE, THE_CONFIG_BACK, signal);
        let out = hooked(&s, &hook, &[], Some(&c));
        was_killed(&s, &out, signal);
        assert!(
            runs_the_config(&s),
            "{step}: the installed binary does not run the config that is left"
        );

        ok(&s.install(&c));
        c_is_installed_over_b(&s, &b, &c);
    }
}

/// A signal inside the run that finishes a killed install, at the last
/// link of that: the shell starts, and the run after it installs.
#[test]
fn a_signal_while_a_killed_install_is_finished_leaves_a_shell_that_starts() {
    for signal in SIGNALS {
        let step = format!("SIG{signal}");
        let s = Sandbox::new(&format!("kill-recover-{signal}"));
        let (_, b) = b_is_installed_and_its_config_edited(&s);
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Kill(PREVIOUS, "KILL"), &[], Some(&c));
        was_killed(&s, &out, "KILL");
        assert!(s.release("current").unwrap().ends_with(".switch"), "{step}");

        let out = hooked(&s, &Hook::Kill(TO_A_RELEASE, signal), &[], Some(&c));
        was_killed(&s, &out, signal);
        assert!(
            runs_the_config(&s),
            "{step}: the installed binary does not run the config that is left"
        );

        ok(&s.install(&c));
        c_is_installed_over_b(&s, &b, &c);
    }
}

/// A stand-in for sudo, the one who may write under the prefix: it lets the user write the
/// prefix's directories for the time of the command and logs the command. What the script
/// does under the prefix without it fails. The tree stored in a release keeps its modes (a
/// config is a copy of it, modes included). Like a command that runs as root, it takes none of
/// the script's three signals from the user.
const SUDO: &str = r#"#!/bin/sh
trap '' INT TERM HUP
printf '%s\n' "$*" >> '@DIR@/calls'
chmod -R u+w "$PREFIX"
"$@"
rc=$?
find "$PREFIX" -type d ! -path '*/releases/[!.]*/tree' ! -path '*/releases/[!.]*/tree/*' \
    -exec chmod u-w {} +
exit $rc
"#;

/// Makes the prefix one the user may not write and returns the PATH with the stand-in [`SUDO`]
/// first, which logs to `sudo/calls` in the sandbox.
fn a_prefix_for_sudo(s: &Sandbox) -> String {
    let sudo = s.dir.join("sudo");
    std::fs::create_dir(&sudo).unwrap();
    s.executable(
        "sudo/sudo",
        &SUDO.replace("@DIR@", &sudo.display().to_string()),
    );
    std::fs::create_dir(s.prefix()).unwrap();
    set_mode(&s.prefix(), 0o555);
    format!("{}:{}", sudo.display(), std::env::var("PATH").unwrap())
}

/// A prefix the user may not write. The script does everything under it
/// through sudo: an install, a second one, a rollback, --status, an install killed between
/// the two renames of the config and the one after it, and --uninstall.
#[test]
fn a_prefix_the_user_cannot_write_is_installed_through_sudo() {
    use std::os::unix::fs::MetadataExt;
    let s = Sandbox::new("through-sudo");
    if std::fs::metadata(&s.dir).unwrap().uid() == 0 {
        testenv::unavailable("a user other than root: root writes every prefix without sudo");
        return;
    }
    let sudo = s.dir.join("sudo");
    let path = a_prefix_for_sudo(&s);
    let run = |hook: &Hook, args: &[&str], bin: Option<&Path>| {
        let mut cmd = hooked_command(&s, hook, args, &path);
        if let Some(bin) = bin {
            cmd.env("BIN", bin);
        }
        cmd.output().unwrap()
    };
    let plain = Hook::Fail("");
    let calls = || std::fs::read_to_string(sudo.join("calls")).unwrap_or_default();
    let read_only = |what: &str| {
        let open: Vec<_> = modes(&s.prefix())
            .into_iter()
            .filter(|(p, mode)| s.prefix().join(p).is_dir() && mode & 0o200 != 0)
            .filter(|(p, _)| !p.components().any(|part| part.as_os_str() == "tree"))
            .collect();
        assert!(open.is_empty(), "after {what}: {open:?}");
        assert_eq!(std::fs::metadata(s.prefix()).unwrap().mode() & 0o200, 0);
    };

    write_tree(&s.source(), "A");
    let a = s.fake("rusty-impulse-a", "A");
    ok(&run(&plain, &[], Some(&a)));
    assert!(calls().contains("mkdir -p"), "{}", calls());
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert!(runs_the_config(&s));
    read_only("the first install");

    write_tree(&s.source(), "B");
    let b = s.fake("rusty-impulse-b", "B");
    ok(&run(&plain, &[], Some(&b)));
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    ok(&run(&plain, &["--rollback"], None));
    assert_eq!(s.installed(), std::fs::read(&a).unwrap());
    assert_eq!(listing(&s.config()), {
        let current = s.release("current").unwrap();
        listing(&current.join("tree"))
    });
    ok(&run(&plain, &["--rollback"], None));
    assert_eq!(s.installed(), std::fs::read(&b).unwrap());
    let status = run(&plain, &["--status"], None);
    ok(&status);
    assert!(text(&status.stdout).contains("previous: "));
    read_only("a rollback");

    // The config is the user's and is renamed without sudo: the stand-in `mv` kills the
    // script itself.
    std::fs::write(s.config().join("note.txt"), "mine").unwrap();
    write_tree(&s.source(), "C");
    let c = s.fake("rusty-impulse-c", "C");
    let out = run(&Hook::Kill(THE_NEW_CONFIG, "KILL"), &[], Some(&c));
    was_killed(&s, &out, "KILL");
    assert!(s.release("current").unwrap().ends_with(".switch"));
    assert!(runs_the_config(&s));
    let out = run(&plain, &[], Some(&c));
    ok(&out);
    assert!(text(&out.stdout).contains("undid the install"));
    c_is_installed_over_b(&s, &b, &c);
    read_only("the install after a killed one");

    let before = calls().len();
    ok(&run(&plain, &["--uninstall"], None));
    assert!(calls()[before..].contains("rm -rf"), "{}", calls());
    assert!(!s.lib().exists());
    assert_eq!(names(&s.prefix().join("bin")), [""; 0]);
    read_only("the uninstall");
}

/// --uninstall after an install that was killed inside its switch
/// first finishes or undoes that one, as an install does: the config it leaves is whole, the
/// one that was there or the new release's, and nothing of the switch is left beside it.
#[test]
fn uninstall_after_a_killed_switch_leaves_a_whole_config() {
    for (i, at) in [THE_NEW_CONFIG, PREVIOUS].iter().enumerate() {
        let s = Sandbox::new(&format!("kill-uninstall-{i}"));
        b_is_installed_and_its_config_edited(&s);
        let edited = listing(&s.config());
        write_tree(&s.source(), "C");
        let c = s.fake("rusty-impulse-c", "C");
        let out = hooked(&s, &Hook::Kill(at, "KILL"), &[], Some(&c));
        was_killed(&s, &out, "KILL");

        let out = s.script(&["--uninstall"], None);
        ok(&out);
        assert!(!s.lib().exists(), "`{at}`");
        assert_eq!(names(&s.prefix().join("bin")), [""; 0], "`{at}`");
        assert_eq!(
            names(s.config().parent().unwrap()),
            ["rusty-impulse"],
            "`{at}`"
        );
        let copies = kept(&s);
        if i == 0 {
            // Killed between the two renames: the config that was there is back.
            assert_eq!(listing(&s.config()), edited, "`{at}`");
            assert_eq!(copies, [""; 0], "`{at}`");
        } else {
            assert_eq!(listing(&s.config()), listing(&s.source()), "`{at}`");
            assert_eq!(copies.len(), 1, "`{at}`: {copies:?}");
            let copy = s.state().join("rusty-impulse").join(&copies[0]);
            assert_eq!(listing(&copy), edited, "`{at}`");
        }
    }
}

/// A reloading shell passes descriptors on to the binary it runs (the pipe of its signals,
/// by its number). The switch's script, which that binary is while a switch was interrupted,
/// passes them on to the release it starts: its own lock is on a descriptor that was free.
#[test]
fn the_switch_script_passes_the_descriptors_it_was_started_with_on() {
    let s = Sandbox::new("switch-fds");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    write_tree(&s.source(), "B");
    let out = hooked(
        &s,
        &Hook::Kill(PREVIOUS, "KILL"),
        &[],
        Some(&s.fake("rusty-impulse-b", "B")),
    );
    was_killed(&s, &out, "KILL");
    assert!(s.release("current").unwrap().ends_with(".switch"));
    for r in s.releases().iter().filter(|r| *r != ".switch") {
        let release = s.lib().join("releases").join(r).join("rusty-impulse");
        let says = "#!/bin/sh\nfor fd in 3 4 5 6 7 8 9; do [ ! -p /dev/fd/$fd ] || echo \"pipe $fd\"; done\n";
        std::fs::write(release, says).unwrap();
    }
    for fd in 3..=9 {
        let (rd, _wr) = std::io::pipe().unwrap();
        let out = Command::new("bash")
            .args(["-c", &format!("exec \"$0\" {fd}<&0")])
            .arg(s.prefix().join("bin/rusty-impulse"))
            .stdin(rd)
            .output()
            .unwrap();
        ok(&out);
        assert_eq!(text(&out.stdout), format!("pipe {fd}\n"));
    }
}

/// The running shell `rusty-impulse -p DIR` of [`a_shell_started_through_the_switch_reloads_into_the_release_installed_since`].
struct Shell {
    child: std::process::Child,
    log: std::sync::Arc<testenv::Log>,
    /// Its tree.
    dir: PathBuf,
    /// The file of its own window.
    window: PathBuf,
    /// Its own run dir, and the compositor's socket by its path.
    env: [(&'static str, PathBuf); 2],
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Its own window is no child of the test's.
        for pid in self.own_windows() {
            testenv::kill_pid_and_wait(pid);
        }
    }
}

impl Shell {
    /// The processes started with the file of its own window.
    fn own_windows(&self) -> Vec<i32> {
        let file = self.window.to_string_lossy().into_owned();
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .filter(|pid| {
                std::fs::read(format!("/proc/{pid}/cmdline"))
                    .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&file))
            })
            .collect()
    }

    /// `rusty-impulse -p DIR ipc call t FUNCTION`.
    fn call(&self, function: &str) {
        let out = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(&self.dir)
            .args(["ipc", "call", "t", function])
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        assert!(out.status.success(), "{function}: {}", text(&out.stderr));
    }

    /// The numbers its log has after `mark`, once there are `n` of them.
    fn logged(&self, mark: &str, n: usize) -> Vec<i32> {
        let found = |l: &str| -> Vec<i32> {
            l.lines()
                .filter_map(|l| l.split_once(mark)?.1.trim().parse().ok())
                .collect()
        };
        self.log
            .wait(|l| found(l).len() >= n)
            .unwrap_or_else(|l| panic!("the shell ended before `{mark}` number {n}:\n{l}"));
        found(&self.log.text())
    }
}

/// The binary the process `pid` runs.
fn exe(pid: i32) -> PathBuf {
    std::fs::read_link(format!("/proc/{pid}/exe")).unwrap()
}

/// A shell started while a switch was interrupted is
/// started by the switch's script, which runs a release by its path. Its reload and its own
/// windows still run the installed name, as those of a shell started from the link do: after
/// the next install that is the new release, not the one the script chose, which is kept to
/// roll back to. The installs are of the stand-ins; the releases the shell runs then get the
/// real binary, which the script would refuse to install (it has the interpreter the test's
/// tree needs).
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_shell_started_through_the_switch_reloads_into_the_release_installed_since() {
    let Some(display) = std::env::var_os("WAYLAND_DISPLAY") else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let real = |release: &Path| {
        std::fs::copy(
            env!("CARGO_BIN_EXE_rusty-impulse"),
            release.join("rusty-impulse"),
        )
        .unwrap();
        release.join("rusty-impulse")
    };
    let s = Sandbox::new("reload-after-switch");
    write_tree(&s.source(), "A");
    ok(&s.install(&s.fake("rusty-impulse-a", "A")));
    let a = s.release("current").unwrap();
    // Killed once the config is B's and no link is switched: the script starts B.
    write_tree(&s.source(), "B");
    let out = hooked(
        &s,
        &Hook::Kill(PREVIOUS, "KILL"),
        &[],
        Some(&s.fake("rusty-impulse-b", "B")),
    );
    was_killed(&s, &out, "KILL");
    assert!(s.release("current").unwrap().ends_with(".switch"));
    let mut b = s
        .releases()
        .into_iter()
        .map(|r| s.lib().join("releases").join(r));
    let b = real(&b.find(|r| *r != a && !r.ends_with(".switch")).unwrap());

    let tmp = tempfile::tempdir().unwrap();
    let (dir, run) = (tmp.path().join("shell"), tmp.path().join("run"));
    for d in [&dir, &run] {
        std::fs::create_dir_all(d).unwrap();
    }
    let window = dir.join("window.qml");
    // Shown, the window says so to the shell.
    let shown = r#"import QtQuick
import QtQuick.Controls
import Impulse
ApplicationWindow {
    visible: true
    width: 240
    height: 120
    Component.onCompleted: ShellControl.ipcCall("t", "shown")
}
"#;
    std::fs::write(&window, shown).unwrap();
    let fixture = r#"import QtQuick
import Rustshell
import Rustshell.Io
import Impulse
ShellRoot {
    Process {
        running: true
        command: ["sleep", "infinity"]
        onStarted: console.log("STARTED " + processId)
    }
    IpcHandler {
        target: "t"
        function ownWindow(): void { ShellControl.openWindow(Rustshell.shellPath("window.qml")) }
        function shown(): void { console.log("OWN WINDOW 1") }
        function reload(): void { Rustshell.reload(true) }
    }
}
"#;
    std::fs::write(dir.join("shell.qml"), fixture).unwrap();
    // The run dir is the test's own: the compositor's socket stays reachable by its path.
    let display = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default()).join(display);
    let env = [("XDG_RUNTIME_DIR", run), ("WAYLAND_DISPLAY", display)];
    let (rd, wr) = std::io::pipe().unwrap();
    let child = Command::new(s.prefix().join("bin/rusty-impulse"))
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(rd);
    let shell = Shell {
        child,
        log,
        dir,
        window,
        env,
    };
    let pid = shell.child.id() as i32;
    shell.logged("STARTED ", 1);
    assert_eq!(exe(pid), b, "the switch's script did not start B");

    write_tree(&s.source(), "C");
    ok(&s.install(&s.fake("rusty-impulse-c", "C")));
    let c = real(&s.release("current").unwrap());
    assert_eq!(s.release("previous").unwrap().join("rusty-impulse"), b);
    assert_eq!(
        std::fs::canonicalize(s.prefix().join("bin/rusty-impulse")).unwrap(),
        c
    );

    shell.call("ownWindow");
    shell.logged("OWN WINDOW ", 1);
    let own = shell.own_windows();
    assert_eq!(own.len(), 1, "{own:?}");
    let own = exe(own[0]);
    shell.call("reload");
    shell.logged("STARTED ", 2);
    assert_eq!(
        (own, exe(pid)),
        (c.clone(), c),
        "the shell's own window and its reload run the installed name"
    );
}
